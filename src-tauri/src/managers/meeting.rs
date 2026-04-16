use chrono::Utc;
use log::{error, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

use crate::managers::audio::AudioRecordingManager;
use crate::managers::history::HistoryManager;
use crate::managers::transcription::TranscriptionManager;
use crate::settings::{self, MeetingAudioSource};

const MEETING_RECORDING_BINDING_ID: &str = "toggle_meeting_recording";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum MeetingRecordingState {
    Idle,
    Recording,
    Processing,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct MeetingRecordingStatus {
    pub state: MeetingRecordingState,
    pub started_at_unix_ms: Option<i64>,
}

#[derive(Debug, Clone)]
struct MeetingRecordingRuntime {
    state: MeetingRecordingState,
    started_at_unix_ms: Option<i64>,
    meeting_audio_source: MeetingAudioSource,
    system_capture_active: bool,
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
                system_capture_active: false,
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

    fn emit_status_change(&self, status: &MeetingRecordingStatus) {
        let _ = self
            .app_handle
            .emit("meeting-recording-status-changed", status.clone());
    }

    fn set_idle(&self) {
        let mut runtime = self.runtime.lock().expect("meeting runtime lock poisoned");
        runtime.state = MeetingRecordingState::Idle;
        runtime.started_at_unix_ms = None;
        runtime.meeting_audio_source = MeetingAudioSource::MicrophoneAndSystem;
        runtime.system_capture_active = false;

        let status = MeetingRecordingStatus {
            state: runtime.state,
            started_at_unix_ms: runtime.started_at_unix_ms,
        };

        drop(runtime);
        self.emit_status_change(&status);
    }

    fn finalize_recording(&self) {
        let audio_manager = self.app_handle.state::<Arc<AudioRecordingManager>>();
        let history_manager = self.app_handle.state::<Arc<HistoryManager>>();
        let transcription_manager = self.app_handle.state::<Arc<TranscriptionManager>>();
        let settings = settings::get_settings(&self.app_handle);
        let (source, system_capture_active) = {
            let runtime = self.runtime.lock().expect("meeting runtime lock poisoned");
            (runtime.meeting_audio_source, runtime.system_capture_active)
        };

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
            match crate::screen_capture::stop_capture() {
                Ok(samples) => system_samples = samples,
                Err(err) => {
                    warn!("Failed to stop system audio capture: {}", err);
                }
            }
        }

        let samples = match source {
            MeetingAudioSource::MicrophoneOnly => microphone_samples,
            MeetingAudioSource::SystemOnly => system_samples,
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
            match transcription_manager.transcribe(samples.clone()) {
                Ok(text) => text,
                Err(err) => {
                    error!("Meeting transcription failed: {}", err);
                    String::new()
                }
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
            if runtime.state == MeetingRecordingState::Processing {
                return Err("Meeting recording is currently processing".to_string());
            }
        }

        let settings = settings::get_settings(&self.app_handle);
        let requested_source = settings.meeting_audio_source;
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
        runtime.system_capture_active = system_capture_active;

        let status = MeetingRecordingStatus {
            state: runtime.state,
            started_at_unix_ms: runtime.started_at_unix_ms,
        };

        drop(runtime);
        self.emit_status_change(&status);

        Ok(status)
    }

    pub fn stop(&self) -> Result<MeetingRecordingStatus, String> {
        let mut runtime = self.runtime.lock().expect("meeting runtime lock poisoned");
        if runtime.state != MeetingRecordingState::Recording {
            return Err("No active meeting recording to stop".to_string());
        }

        runtime.state = MeetingRecordingState::Processing;

        let status = MeetingRecordingStatus {
            state: runtime.state,
            started_at_unix_ms: runtime.started_at_unix_ms,
        };

        drop(runtime);
        self.emit_status_change(&status);

        let manager = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            manager.finalize_recording();
        });

        Ok(status)
    }

    pub fn toggle(&self) -> Result<MeetingRecordingStatus, String> {
        match self.status().state {
            MeetingRecordingState::Recording => self.stop(),
            MeetingRecordingState::Idle => self.start(),
            MeetingRecordingState::Processing => {
                Err("Meeting recording is currently processing".to_string())
            }
        }
    }
}
