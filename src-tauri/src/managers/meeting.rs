use chrono::Utc;
use log::{error, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::managers::audio::AudioRecordingManager;
use crate::managers::history::HistoryManager;
use crate::managers::transcription::TranscriptionManager;
use crate::settings::{self, MeetingAudioSource, MeetingTranscriptMergePolicy};

const MEETING_RECORDING_BINDING_ID: &str = "toggle_meeting_recording";
const TRANSCRIPT_CHUNK_SECONDS: usize = 4;
const TRANSCRIPT_SAMPLE_RATE_HZ: usize = 16_000;
const SILENCE_RMS_THRESHOLD: f32 = 0.0025;
const SYSTEM_LEVEL_POLL_INTERVAL: Duration = Duration::from_millis(50);
#[cfg(target_os = "windows")]
const ZOOM_DETECTION_POLL_INTERVAL: Duration = Duration::from_millis(1500);
#[cfg(target_os = "windows")]
const ZOOM_DETECTION_ACTIVE_POLLS_REQUIRED: u8 = 2;
#[cfg(target_os = "windows")]
const ZOOM_DETECTION_INACTIVE_POLLS_REQUIRED: u8 = 4;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum MeetingRecordingState {
    Idle,
    Recording,
    Paused,
    Processing,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct MeetingRecordingStatus {
    pub state: MeetingRecordingState,
    pub started_at_unix_ms: Option<i64>,
}

struct MeetingRecordingRuntime {
    state: MeetingRecordingState,
    started_at_unix_ms: Option<i64>,
    meeting_audio_source: MeetingAudioSource,
    meeting_transcript_merge_policy: MeetingTranscriptMergePolicy,
    system_capture_active: bool,
    zoom_auto_started: bool,
    accumulated_microphone_samples: Vec<f32>,
    accumulated_system_samples: Vec<f32>,
    system_level_poll_stop: Option<Arc<AtomicBool>>,
    system_level_poll_handle: Option<JoinHandle<()>>,
}

#[derive(Debug, Serialize)]
struct MeetingTranscriptSources {
    merge_policy: MeetingTranscriptMergePolicy,
    segment_count: usize,
    merged_text: String,
    system_text: Option<String>,
    microphone_text: Option<String>,
}

#[derive(Clone)]
pub struct MeetingRecordingManager {
    runtime: Arc<Mutex<MeetingRecordingRuntime>>,
    app_handle: AppHandle,
}

impl MeetingRecordingManager {
    pub fn new(app_handle: &AppHandle) -> Self {
        Self {
            runtime: Arc::new(Mutex::new(MeetingRecordingRuntime {
                state: MeetingRecordingState::Idle,
                started_at_unix_ms: None,
                meeting_audio_source: MeetingAudioSource::MicrophoneAndSystem,
                meeting_transcript_merge_policy: MeetingTranscriptMergePolicy::SystemPriority,
                system_capture_active: false,
                zoom_auto_started: false,
                accumulated_microphone_samples: Vec::new(),
                accumulated_system_samples: Vec::new(),
                system_level_poll_stop: None,
                system_level_poll_handle: None,
            })),
            app_handle: app_handle.clone(),
        }
    }

    fn source_uses_microphone(source: MeetingAudioSource) -> bool {
        matches!(
            source,
            MeetingAudioSource::MicrophoneOnly | MeetingAudioSource::MicrophoneAndSystem
        )
    }

    fn source_uses_system_audio(source: MeetingAudioSource) -> bool {
        matches!(
            source,
            MeetingAudioSource::SystemOnly | MeetingAudioSource::MicrophoneAndSystem
        )
    }

    #[cfg(target_os = "windows")]
    fn set_zoom_auto_started(&self, value: bool) {
        let mut runtime = self.runtime.lock().expect("meeting runtime lock poisoned");
        runtime.zoom_auto_started = value;
    }

    #[cfg(target_os = "windows")]
    fn is_zoom_auto_started_active(&self) -> bool {
        let runtime = self.runtime.lock().expect("meeting runtime lock poisoned");
        runtime.zoom_auto_started
            && matches!(
                runtime.state,
                MeetingRecordingState::Recording | MeetingRecordingState::Paused
            )
    }

    pub fn start_zoom_auto_detection_loop(&self) {
        #[cfg(not(target_os = "windows"))]
        {
            return;
        }

        #[cfg(target_os = "windows")]
        {
            let manager = self.clone();
            let app_handle = self.app_handle.clone();

            std::thread::spawn(move || {
                let mut active_streak = 0u8;
                let mut inactive_streak = 0u8;

                loop {
                    let settings = settings::get_settings(&app_handle);
                    if !settings.experimental_enabled {
                        active_streak = 0;
                        inactive_streak = 0;
                        std::thread::sleep(ZOOM_DETECTION_POLL_INTERVAL);
                        continue;
                    }

                    let zoom_active = crate::zoom_detection::is_zoom_meeting_active();

                    if zoom_active {
                        active_streak = active_streak.saturating_add(1);
                        inactive_streak = 0;
                    } else {
                        inactive_streak = inactive_streak.saturating_add(1);
                        active_streak = 0;
                    }

                    if active_streak >= ZOOM_DETECTION_ACTIVE_POLLS_REQUIRED
                        && manager.status().state == MeetingRecordingState::Idle
                    {
                        if manager.start().is_ok() {
                            manager.set_zoom_auto_started(true);
                        }
                    }

                    if inactive_streak >= ZOOM_DETECTION_INACTIVE_POLLS_REQUIRED
                        && manager.is_zoom_auto_started_active()
                    {
                        let _ = manager.stop();
                    }

                    std::thread::sleep(ZOOM_DETECTION_POLL_INTERVAL);
                }
            });
        }
    }

    fn mix_audio_streams(microphone: &[f32], system: &[f32]) -> Vec<f32> {
        if microphone.is_empty() {
            return system.to_vec();
        }
        if system.is_empty() {
            return microphone.to_vec();
        }

        let out_len = microphone.len().max(system.len());
        let mut mixed = Vec::with_capacity(out_len);

        for i in 0..out_len {
            let mic = microphone.get(i).copied().unwrap_or(0.0);
            let sys = system.get(i).copied().unwrap_or(0.0);
            mixed.push(((mic + sys) * 0.5).clamp(-1.0, 1.0));
        }

        mixed
    }

    fn normalize_for_compare(text: &str) -> String {
        text.chars()
            .map(|ch| {
                if ch.is_alphanumeric() || ch.is_whitespace() {
                    ch.to_ascii_lowercase()
                } else {
                    ' '
                }
            })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn filtered_token_set(normalized_text: &str) -> HashSet<String> {
        const COMMON_TOKENS: &[&str] = &[
            "a", "an", "the", "and", "or", "to", "of", "in", "on", "for", "is", "it", "i",
            "you", "we", "uh", "um", "yeah", "okay",
        ];

        normalized_text
            .split_whitespace()
            .filter(|token| !COMMON_TOKENS.contains(token))
            .map(str::to_string)
            .collect()
    }

    fn bigram_set(normalized_text: &str) -> HashSet<(String, String)> {
        let tokens: Vec<&str> = normalized_text.split_whitespace().collect();
        if tokens.len() < 2 {
            return HashSet::new();
        }

        tokens
            .windows(2)
            .map(|pair| (pair[0].to_string(), pair[1].to_string()))
            .collect()
    }

    fn transcripts_overlap(system_text: &str, microphone_text: &str) -> bool {
        let normalized_system = Self::normalize_for_compare(system_text);
        let normalized_microphone = Self::normalize_for_compare(microphone_text);

        if normalized_system.is_empty() || normalized_microphone.is_empty() {
            return false;
        }

        if normalized_system == normalized_microphone
            || normalized_system.contains(&normalized_microphone)
            || normalized_microphone.contains(&normalized_system)
        {
            return true;
        }

        let system_tokens = Self::filtered_token_set(&normalized_system);
        let microphone_tokens = Self::filtered_token_set(&normalized_microphone);

        if system_tokens.len() < 2 || microphone_tokens.len() < 2 {
            return false;
        }

        let overlap_count = system_tokens.intersection(&microphone_tokens).count();
        if overlap_count == 0 {
            return false;
        }

        let min_tokens = system_tokens.len().min(microphone_tokens.len()) as f32;
        let union_count = system_tokens.union(&microphone_tokens).count() as f32;
        let coverage = overlap_count as f32 / min_tokens;
        let jaccard = overlap_count as f32 / union_count;

        if coverage >= 0.6 || (coverage >= 0.45 && jaccard >= 0.35) {
            return true;
        }

        let system_bigrams = Self::bigram_set(&normalized_system);
        let microphone_bigrams = Self::bigram_set(&normalized_microphone);
        system_bigrams
            .intersection(&microphone_bigrams)
            .next()
            .is_some()
            && min_tokens >= 5.0
    }

    fn segment_has_speech(samples: &[f32]) -> bool {
        if samples.is_empty() {
            return false;
        }

        let mut sum_sq = 0.0f64;
        let mut peak = 0.0f32;

        for sample in samples {
            let abs = sample.abs();
            peak = peak.max(abs);
            sum_sq += (sample * sample) as f64;
        }

        let rms = (sum_sq / samples.len() as f64).sqrt() as f32;
        rms >= SILENCE_RMS_THRESHOLD || peak >= 0.01
    }

    fn transcribe_samples(
        transcription_manager: &TranscriptionManager,
        samples: &[f32],
        source_label: &str,
    ) -> String {
        if samples.is_empty() {
            return String::new();
        }

        match transcription_manager.transcribe(samples.to_vec()) {
            Ok(text) => text.trim().to_string(),
            Err(err) => {
                error!(
                    "Meeting transcription failed for {}: {}",
                    source_label, err
                );
                String::new()
            }
        }
    }

    fn transcribe_segment_if_speech(
        transcription_manager: &TranscriptionManager,
        samples: &[f32],
        source_label: &str,
    ) -> String {
        if !Self::segment_has_speech(samples) {
            return String::new();
        }

        Self::transcribe_samples(transcription_manager, samples, source_label)
    }

    fn merge_segment_transcripts(
        merge_policy: MeetingTranscriptMergePolicy,
        system_text: &str,
        microphone_text: &str,
    ) -> String {
        let system_trimmed = system_text.trim();
        let microphone_trimmed = microphone_text.trim();

        if system_trimmed.is_empty() {
            return microphone_trimmed.to_string();
        }

        if microphone_trimmed.is_empty() {
            return system_trimmed.to_string();
        }

        let overlap = Self::transcripts_overlap(system_trimmed, microphone_trimmed);

        match merge_policy {
            MeetingTranscriptMergePolicy::SystemPriority => {
                if overlap {
                    system_trimmed.to_string()
                } else {
                    format!("{}\n{}", system_trimmed, microphone_trimmed)
                }
            }
            MeetingTranscriptMergePolicy::Balanced => {
                if overlap {
                    if system_trimmed.len() >= microphone_trimmed.len() {
                        system_trimmed.to_string()
                    } else {
                        microphone_trimmed.to_string()
                    }
                } else {
                    format!("{}\n{}", system_trimmed, microphone_trimmed)
                }
            }
            MeetingTranscriptMergePolicy::KeepBoth => {
                format!("{}\n{}", system_trimmed, microphone_trimmed)
            }
        }
    }

    fn write_transcript_sources_sidecar(
        wav_path: &std::path::Path,
        sources: &MeetingTranscriptSources,
    ) {
        let sidecar_path = wav_path.with_extension("transcript_sources.json");
        match serde_json::to_vec_pretty(sources) {
            Ok(data) => {
                if let Err(err) = std::fs::write(&sidecar_path, data) {
                    warn!(
                        "Failed to write meeting transcript sources sidecar at {:?}: {}",
                        sidecar_path, err
                    );
                }
            }
            Err(err) => {
                warn!(
                    "Failed to serialize meeting transcript sources sidecar for {:?}: {}",
                    sidecar_path, err
                );
            }
        }
    }

    fn append_samples_to_runtime(
        &self,
        microphone_samples: Vec<f32>,
        system_samples: Vec<f32>,
    ) {
        let mut runtime = self.runtime.lock().expect("meeting runtime lock poisoned");
        runtime
            .accumulated_microphone_samples
            .extend(microphone_samples);
        runtime.accumulated_system_samples.extend(system_samples);
    }

    fn stop_system_level_polling(&self) {
        let (stop_signal, handle) = {
            let mut runtime = self.runtime.lock().expect("meeting runtime lock poisoned");
            (
                runtime.system_level_poll_stop.take(),
                runtime.system_level_poll_handle.take(),
            )
        };

        if let Some(stop_signal) = stop_signal {
            stop_signal.store(true, Ordering::Relaxed);
        }

        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }

    fn start_system_level_polling(&self) {
        self.stop_system_level_polling();

        let stop_signal = Arc::new(AtomicBool::new(false));
        let thread_stop_signal = stop_signal.clone();
        let app_handle = self.app_handle.clone();

        let handle = thread::spawn(move || {
            while !thread_stop_signal.load(Ordering::Relaxed) {
                let levels = crate::screen_capture::current_levels();
                if !levels.is_empty() {
                    crate::overlay::emit_levels(&app_handle, &levels);
                }

                thread::sleep(SYSTEM_LEVEL_POLL_INTERVAL);
            }
        });

        let mut runtime = self.runtime.lock().expect("meeting runtime lock poisoned");
        runtime.system_level_poll_stop = Some(stop_signal);
        runtime.system_level_poll_handle = Some(handle);
    }

    fn collect_active_segment(
        &self,
        source: MeetingAudioSource,
        system_capture_active: bool,
    ) -> (Vec<f32>, Vec<f32>) {
        let audio_manager = self.app_handle.state::<Arc<AudioRecordingManager>>();

        let microphone_samples = if Self::source_uses_microphone(source) {
            let samples = audio_manager
                .stop_recording(MEETING_RECORDING_BINDING_ID)
                .unwrap_or_default();
            audio_manager.remove_mute();
            samples
        } else {
            Vec::new()
        };

        let mut system_samples = Vec::new();
        if system_capture_active {
            self.stop_system_level_polling();
            match crate::screen_capture::stop_capture() {
                Ok(samples) => system_samples = samples,
                Err(err) => {
                    warn!("Failed to stop system audio capture: {}", err);
                }
            }
        }

        (microphone_samples, system_samples)
    }

    fn emit_status_change(&self, status: &MeetingRecordingStatus) {
        let _ = self
            .app_handle
            .emit("meeting-recording-status-changed", status.clone());

        match status.state {
            MeetingRecordingState::Recording | MeetingRecordingState::Paused => {
                crate::overlay::show_meeting_overlay(
                    &self.app_handle,
                    status.state == MeetingRecordingState::Paused,
                );
            }
            MeetingRecordingState::Idle | MeetingRecordingState::Processing => {
                crate::overlay::hide_recording_overlay(&self.app_handle);
            }
        }
    }

    fn set_idle(&self) {
        self.stop_system_level_polling();

        let mut runtime = self.runtime.lock().expect("meeting runtime lock poisoned");
        runtime.state = MeetingRecordingState::Idle;
        runtime.started_at_unix_ms = None;
        runtime.meeting_audio_source = MeetingAudioSource::MicrophoneAndSystem;
        runtime.meeting_transcript_merge_policy = MeetingTranscriptMergePolicy::SystemPriority;
        runtime.system_capture_active = false;
        runtime.zoom_auto_started = false;
        runtime.accumulated_microphone_samples.clear();
        runtime.accumulated_system_samples.clear();
        runtime.system_level_poll_stop = None;
        runtime.system_level_poll_handle = None;

        let status = MeetingRecordingStatus {
            state: runtime.state,
            started_at_unix_ms: runtime.started_at_unix_ms,
        };

        drop(runtime);
        self.emit_status_change(&status);
    }

    fn finalize_recording(&self) {
        let history_manager = self.app_handle.state::<Arc<HistoryManager>>();
        let transcription_manager = self.app_handle.state::<Arc<TranscriptionManager>>();
        let settings = settings::get_settings(&self.app_handle);
        let (source, merge_policy, mut microphone_samples, mut system_samples) = {
            let mut runtime = self.runtime.lock().expect("meeting runtime lock poisoned");
            (
                runtime.meeting_audio_source,
                runtime.meeting_transcript_merge_policy,
                std::mem::take(&mut runtime.accumulated_microphone_samples),
                std::mem::take(&mut runtime.accumulated_system_samples),
            )
        };

        if !Self::source_uses_microphone(source) {
            microphone_samples.clear();
        }
        if !Self::source_uses_system_audio(source) {
            system_samples.clear();
        }

        let samples = match source {
            MeetingAudioSource::MicrophoneOnly => microphone_samples.clone(),
            MeetingAudioSource::SystemOnly => system_samples.clone(),
            MeetingAudioSource::MicrophoneAndSystem => {
                Self::mix_audio_streams(&microphone_samples, &system_samples)
            }
        };

        if samples.is_empty() {
            self.set_idle();
            return;
        }

        let file_name = format!("handy-meeting-{}.wav", Utc::now().timestamp());
        let wav_path = history_manager.recordings_dir().join(&file_name);

        if let Err(err) = crate::audio_toolkit::save_wav_file(&wav_path, &samples) {
            error!("Failed to save meeting recording WAV file: {}", err);
            self.set_idle();
            return;
        }

        let transcription_text = if settings.meeting_transcribe_on_stop {
            transcription_manager.initiate_model_load();
            if source == MeetingAudioSource::MicrophoneAndSystem
                && !system_samples.is_empty()
                && !microphone_samples.is_empty()
            {
                let segment_size = TRANSCRIPT_SAMPLE_RATE_HZ * TRANSCRIPT_CHUNK_SECONDS;
                let max_len = system_samples.len().max(microphone_samples.len());
                let segment_count = max_len.div_ceil(segment_size);

                let mut merged_segments = Vec::new();
                let mut system_segments = Vec::new();
                let mut microphone_segments = Vec::new();

                for segment_index in 0..segment_count {
                    let start = segment_index * segment_size;
                    let end = ((segment_index + 1) * segment_size).min(max_len);

                    let system_chunk = if start < system_samples.len() {
                        &system_samples[start..end.min(system_samples.len())]
                    } else {
                        &[]
                    };

                    let microphone_chunk = if start < microphone_samples.len() {
                        &microphone_samples[start..end.min(microphone_samples.len())]
                    } else {
                        &[]
                    };

                    let system_text = Self::transcribe_segment_if_speech(
                        transcription_manager.as_ref(),
                        system_chunk,
                        "system audio segment",
                    );
                    let microphone_text = Self::transcribe_segment_if_speech(
                        transcription_manager.as_ref(),
                        microphone_chunk,
                        "microphone audio segment",
                    );

                    if !system_text.is_empty() {
                        system_segments.push(system_text.clone());
                    }
                    if !microphone_text.is_empty() {
                        microphone_segments.push(microphone_text.clone());
                    }

                    let merged =
                        Self::merge_segment_transcripts(merge_policy, &system_text, &microphone_text);

                    if !merged.is_empty() {
                        let is_duplicate = merged_segments
                            .last()
                            .map(|existing: &String| existing == &merged)
                            .unwrap_or(false);

                        if !is_duplicate {
                            merged_segments.push(merged);
                        }
                    }
                }

                let system_transcription_text = system_segments.join("\n").trim().to_string();
                let microphone_transcription_text =
                    microphone_segments.join("\n").trim().to_string();

                let mut merged_text = merged_segments.join("\n").trim().to_string();

                if merged_text.is_empty() {
                    merged_text = Self::transcribe_samples(
                        transcription_manager.as_ref(),
                        &samples,
                        "meeting audio fallback",
                    );
                }

                Self::write_transcript_sources_sidecar(
                    &wav_path,
                    &MeetingTranscriptSources {
                        merge_policy,
                        segment_count,
                        merged_text: merged_text.clone(),
                        system_text: if system_transcription_text.is_empty() {
                            None
                        } else {
                            Some(system_transcription_text.clone())
                        },
                        microphone_text: if microphone_transcription_text.is_empty() {
                            None
                        } else {
                            Some(microphone_transcription_text.clone())
                        },
                    },
                );

                merged_text
            } else {
                let text = Self::transcribe_samples(
                    transcription_manager.as_ref(),
                    &samples,
                    "meeting audio",
                );

                Self::write_transcript_sources_sidecar(
                    &wav_path,
                    &MeetingTranscriptSources {
                        merge_policy,
                        segment_count: 1,
                        merged_text: text.clone(),
                        system_text: if source == MeetingAudioSource::SystemOnly {
                            if text.is_empty() {
                                None
                            } else {
                                Some(text.clone())
                            }
                        } else {
                            None
                        },
                        microphone_text: if source == MeetingAudioSource::MicrophoneOnly {
                            if text.is_empty() {
                                None
                            } else {
                                Some(text.clone())
                            }
                        } else {
                            None
                        },
                    },
                );

                text
            }
        } else {
            String::new()
        };

        if let Err(err) = history_manager.save_entry(file_name, transcription_text, false, None, None)
        {
            error!("Failed to save meeting entry to history: {}", err);
        }

        self.set_idle();
    }

    pub fn status(&self) -> MeetingRecordingStatus {
        let runtime = self.runtime.lock().expect("meeting runtime lock poisoned");
        MeetingRecordingStatus {
            state: runtime.state,
            started_at_unix_ms: runtime.started_at_unix_ms,
        }
    }

    pub fn start(&self) -> Result<MeetingRecordingStatus, String> {
        {
            let runtime = self.runtime.lock().expect("meeting runtime lock poisoned");
            if runtime.state == MeetingRecordingState::Recording {
                return Err("Meeting recording is already active".to_string());
            }
            if runtime.state == MeetingRecordingState::Paused {
                return Err("Meeting recording is paused. Resume or stop it first".to_string());
            }
            if runtime.state == MeetingRecordingState::Processing {
                return Err("Meeting recording is currently processing".to_string());
            }
        }

        let settings = settings::get_settings(&self.app_handle);
        let requested_source = settings.meeting_audio_source;
        let merge_policy = settings.meeting_transcript_merge_policy;
        let mut active_source = requested_source;
        let mut system_capture_active = false;

        if Self::source_uses_system_audio(requested_source) {
            if !crate::screen_capture::is_supported() {
                if requested_source == MeetingAudioSource::SystemOnly {
                    return Err("System audio capture is not supported on this build".to_string());
                }

                warn!(
                    "System audio capture is unavailable on this build. Falling back to microphone-only meeting capture"
                );
                active_source = MeetingAudioSource::MicrophoneOnly;
            } else {
                if !crate::screen_capture::preflight_access() && !crate::screen_capture::request_access()
                {
                    if requested_source == MeetingAudioSource::SystemOnly {
                        return Err("Screen capture permission denied".to_string());
                    }

                    warn!(
                        "Screen capture permission denied for meeting capture. Falling back to microphone-only"
                    );
                    active_source = MeetingAudioSource::MicrophoneOnly;
                } else if let Err(err) = crate::screen_capture::start_capture() {
                    if requested_source == MeetingAudioSource::SystemOnly {
                        return Err(format!(
                            "Failed to start system audio capture for meeting recording: {}",
                            err
                        ));
                    }

                    warn!(
                        "System audio capture failed to start ({}). Falling back to microphone-only",
                        err
                    );
                    active_source = MeetingAudioSource::MicrophoneOnly;
                } else {
                    system_capture_active = true;
                }
            }
        }

        let audio_manager = self.app_handle.state::<Arc<AudioRecordingManager>>();
        if Self::source_uses_microphone(active_source) {
            if let Err(err) = audio_manager.try_start_recording(MEETING_RECORDING_BINDING_ID) {
                if system_capture_active {
                    let _ = crate::screen_capture::stop_capture();
                }
                return Err(format!("Failed to start meeting recording: {}", err));
            }
            audio_manager.apply_mute();
        }

        // Start loading the selected transcription model in the background
        // so post-stop transcription has less startup latency.
        let transcription_manager = self.app_handle.state::<Arc<TranscriptionManager>>();
        transcription_manager.initiate_model_load();

        let mut runtime = self.runtime.lock().expect("meeting runtime lock poisoned");
        runtime.state = MeetingRecordingState::Recording;
        runtime.started_at_unix_ms = Some(Utc::now().timestamp_millis());
        runtime.meeting_audio_source = active_source;
        runtime.meeting_transcript_merge_policy = merge_policy;
        runtime.system_capture_active = system_capture_active;
        runtime.accumulated_microphone_samples.clear();
        runtime.accumulated_system_samples.clear();

        let status = MeetingRecordingStatus {
            state: runtime.state,
            started_at_unix_ms: runtime.started_at_unix_ms,
        };

        drop(runtime);
        self.emit_status_change(&status);

        if system_capture_active {
            self.start_system_level_polling();
        }

        Ok(status)
    }

    pub fn stop(&self) -> Result<MeetingRecordingStatus, String> {
        let (source, system_capture_active, should_collect_active, status) = {
            let mut runtime = self.runtime.lock().expect("meeting runtime lock poisoned");
            let should_collect_active = match runtime.state {
                MeetingRecordingState::Recording => true,
                MeetingRecordingState::Paused => false,
                _ => return Err("No active meeting recording to stop".to_string()),
            };

            let source = runtime.meeting_audio_source;
            let system_capture_active = runtime.system_capture_active;
            runtime.system_capture_active = false;
            runtime.zoom_auto_started = false;
            runtime.state = MeetingRecordingState::Processing;

            let status = MeetingRecordingStatus {
                state: runtime.state,
                started_at_unix_ms: runtime.started_at_unix_ms,
            };

            (source, system_capture_active, should_collect_active, status)
        };

        self.emit_status_change(&status);
        self.stop_system_level_polling();

        if should_collect_active {
            let (microphone_samples, system_samples) =
                self.collect_active_segment(source, system_capture_active);
            self.append_samples_to_runtime(microphone_samples, system_samples);
        }

        let manager = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            manager.finalize_recording();
        });

        Ok(status)
    }

    pub fn pause(&self) -> Result<MeetingRecordingStatus, String> {
        let (source, system_capture_active, status) = {
            let mut runtime = self.runtime.lock().expect("meeting runtime lock poisoned");
            if runtime.state != MeetingRecordingState::Recording {
                return Err("No active meeting recording to pause".to_string());
            }

            let source = runtime.meeting_audio_source;
            let system_capture_active = runtime.system_capture_active;
            runtime.system_capture_active = false;
            runtime.state = MeetingRecordingState::Paused;

            let status = MeetingRecordingStatus {
                state: runtime.state,
                started_at_unix_ms: runtime.started_at_unix_ms,
            };

            (source, system_capture_active, status)
        };

        self.emit_status_change(&status);

        let (microphone_samples, system_samples) =
            self.collect_active_segment(source, system_capture_active);
        self.append_samples_to_runtime(microphone_samples, system_samples);

        Ok(status)
    }

    pub fn resume(&self) -> Result<MeetingRecordingStatus, String> {
        let (requested_source, merge_policy) = {
            let runtime = self.runtime.lock().expect("meeting runtime lock poisoned");
            if runtime.state != MeetingRecordingState::Paused {
                return Err("No paused meeting recording to resume".to_string());
            }

            (
                runtime.meeting_audio_source,
                runtime.meeting_transcript_merge_policy,
            )
        };

        let mut active_source = requested_source;
        let mut system_capture_active = false;

        if Self::source_uses_system_audio(requested_source) {
            if !crate::screen_capture::is_supported() {
                if requested_source == MeetingAudioSource::SystemOnly {
                    return Err("System audio capture is not supported on this build".to_string());
                }

                warn!(
                    "System audio capture is unavailable on this build. Falling back to microphone-only meeting capture"
                );
                active_source = MeetingAudioSource::MicrophoneOnly;
            } else {
                if !crate::screen_capture::preflight_access() && !crate::screen_capture::request_access()
                {
                    if requested_source == MeetingAudioSource::SystemOnly {
                        return Err("Screen capture permission denied".to_string());
                    }

                    warn!(
                        "Screen capture permission denied for meeting capture. Falling back to microphone-only"
                    );
                    active_source = MeetingAudioSource::MicrophoneOnly;
                } else if let Err(err) = crate::screen_capture::start_capture() {
                    if requested_source == MeetingAudioSource::SystemOnly {
                        return Err(format!(
                            "Failed to start system audio capture for meeting recording: {}",
                            err
                        ));
                    }

                    warn!(
                        "System audio capture failed to start ({}). Falling back to microphone-only",
                        err
                    );
                    active_source = MeetingAudioSource::MicrophoneOnly;
                } else {
                    system_capture_active = true;
                }
            }
        }

        let audio_manager = self.app_handle.state::<Arc<AudioRecordingManager>>();
        if Self::source_uses_microphone(active_source) {
            if let Err(err) = audio_manager.try_start_recording(MEETING_RECORDING_BINDING_ID) {
                if system_capture_active {
                    let _ = crate::screen_capture::stop_capture();
                }
                return Err(format!("Failed to resume meeting recording: {}", err));
            }
            audio_manager.apply_mute();
        }

        let mut runtime = self.runtime.lock().expect("meeting runtime lock poisoned");
        if runtime.state != MeetingRecordingState::Paused {
            return Err("Meeting recording state changed before resume".to_string());
        }

        runtime.state = MeetingRecordingState::Recording;
        runtime.meeting_audio_source = active_source;
        runtime.meeting_transcript_merge_policy = merge_policy;
        runtime.system_capture_active = system_capture_active;

        let status = MeetingRecordingStatus {
            state: runtime.state,
            started_at_unix_ms: runtime.started_at_unix_ms,
        };

        drop(runtime);
        self.emit_status_change(&status);

        if system_capture_active {
            self.start_system_level_polling();
        }

        Ok(status)
    }

    pub fn toggle_pause(&self) -> Result<MeetingRecordingStatus, String> {
        match self.status().state {
            MeetingRecordingState::Recording => self.pause(),
            MeetingRecordingState::Paused => self.resume(),
            MeetingRecordingState::Idle => {
                Err("No active meeting recording to pause or resume".to_string())
            }
            MeetingRecordingState::Processing => {
                Err("Meeting recording is currently processing".to_string())
            }
        }
    }

    pub fn toggle(&self) -> Result<MeetingRecordingStatus, String> {
        match self.status().state {
            MeetingRecordingState::Recording => self.stop(),
            MeetingRecordingState::Paused => self.stop(),
            MeetingRecordingState::Idle => self.start(),
            MeetingRecordingState::Processing => {
                Err("Meeting recording is currently processing".to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MeetingRecordingManager, MeetingTranscriptMergePolicy};

    #[test]
    fn prefers_system_when_transcripts_overlap() {
        let merged = MeetingRecordingManager::merge_segment_transcripts(
            MeetingTranscriptMergePolicy::SystemPriority,
            "Hey team we should ship tomorrow",
            "hey team we should ship tomorrow morning",
        );

        assert_eq!(merged, "Hey team we should ship tomorrow");
    }

    #[test]
    fn keeps_both_when_transcripts_do_not_overlap() {
        let merged = MeetingRecordingManager::merge_segment_transcripts(
            MeetingTranscriptMergePolicy::SystemPriority,
            "System participant: can everyone hear me?",
            "Local note: I am muted on Zoom",
        );

        assert_eq!(
            merged,
            "System participant: can everyone hear me?\nLocal note: I am muted on Zoom"
        );
    }

    #[test]
    fn keep_both_policy_keeps_overlap_sources() {
        let merged = MeetingRecordingManager::merge_segment_transcripts(
            MeetingTranscriptMergePolicy::KeepBoth,
            "yes we are shipping now",
            "yes we are shipping now",
        );

        assert_eq!(merged, "yes we are shipping now\nyes we are shipping now");
    }

    #[test]
    fn falls_back_to_microphone_when_system_is_empty() {
        let merged = MeetingRecordingManager::merge_segment_transcripts(
            MeetingTranscriptMergePolicy::SystemPriority,
            "",
            "microphone only text",
        );

        assert_eq!(merged, "microphone only text");
    }

    #[test]
    fn short_common_words_do_not_trigger_overlap() {
        assert!(!MeetingRecordingManager::transcripts_overlap(
            "okay yes",
            "yes okay"
        ));
    }

    #[test]
    fn detects_speech_segments_from_non_silent_audio() {
        let samples = vec![0.0f32, 0.005, -0.004, 0.003, 0.0];
        assert!(MeetingRecordingManager::segment_has_speech(&samples));
    }
}
