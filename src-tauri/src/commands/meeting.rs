use crate::managers::meeting::{MeetingRecordingManager, MeetingRecordingStatus};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;
use tauri::State;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum MeetingPermissionAccess {
    Allowed,
    Denied,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct MeetingPermissionStatus {
    pub supported: bool,
    pub microphone: MeetingPermissionAccess,
    pub screen_capture: MeetingPermissionAccess,
    pub overall_access: MeetingPermissionAccess,
}

#[tauri::command]
#[specta::specta]
pub fn get_meeting_recording_status(
    meeting_manager: State<'_, Arc<MeetingRecordingManager>>,
) -> MeetingRecordingStatus {
    meeting_manager.status()
}

#[tauri::command]
#[specta::specta]
pub fn start_meeting_recording(
    meeting_manager: State<'_, Arc<MeetingRecordingManager>>,
) -> Result<MeetingRecordingStatus, String> {
    meeting_manager.start()
}

#[tauri::command]
#[specta::specta]
pub fn stop_meeting_recording(
    meeting_manager: State<'_, Arc<MeetingRecordingManager>>,
) -> Result<MeetingRecordingStatus, String> {
    meeting_manager.stop()
}

#[tauri::command]
#[specta::specta]
pub fn toggle_meeting_recording(
    meeting_manager: State<'_, Arc<MeetingRecordingManager>>,
) -> Result<MeetingRecordingStatus, String> {
    meeting_manager.toggle()
}

#[tauri::command]
#[specta::specta]
pub fn toggle_meeting_recording_pause(
    meeting_manager: State<'_, Arc<MeetingRecordingManager>>,
) -> Result<MeetingRecordingStatus, String> {
    meeting_manager.toggle_pause()
}

#[tauri::command]
#[specta::specta]
pub fn get_meeting_permission_status() -> MeetingPermissionStatus {
    let supported = crate::screen_capture::is_supported();
    if !supported {
        return MeetingPermissionStatus {
            supported: false,
            microphone: MeetingPermissionAccess::Unknown,
            screen_capture: MeetingPermissionAccess::Unknown,
            overall_access: MeetingPermissionAccess::Unknown,
        };
    }

    let screen_capture = if crate::screen_capture::preflight_access() {
        MeetingPermissionAccess::Allowed
    } else {
        MeetingPermissionAccess::Denied
    };

    let overall_access = if screen_capture == MeetingPermissionAccess::Denied {
        MeetingPermissionAccess::Denied
    } else {
        MeetingPermissionAccess::Unknown
    };

    MeetingPermissionStatus {
        supported: true,
        microphone: MeetingPermissionAccess::Unknown,
        screen_capture,
        overall_access,
    }
}
