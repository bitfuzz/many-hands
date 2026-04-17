#[cfg(target_os = "windows")]
use windows::core::PWSTR;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::CloseHandle;

#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
};

fn title_indicates_zoom_meeting(title: &str) -> bool {
    let normalized = title.to_ascii_lowercase();

    if normalized.contains("zoom meeting")
        || normalized.contains("meeting controls")
        || normalized.contains("joining meeting")
        || normalized.contains("waiting for host")
        || (normalized.contains("zoom") && normalized.contains("meeting"))
    {
        return true;
    }

    let has_zoom_workplace_context =
        normalized.contains("zoom workplace -") || normalized.contains(" - zoom workplace");
    if !has_zoom_workplace_context {
        return false;
    }

    const NON_MEETING_CONTEXT_MARKERS: &[&str] = &[
        "chat",
        "contacts",
        "mail",
        "calendar",
        "whiteboard",
        "phone",
        "settings",
        "home",
    ];

    let has_non_meeting_marker = NON_MEETING_CONTEXT_MARKERS
        .iter()
        .any(|marker| normalized.contains(marker));
    let has_meeting_marker = normalized.contains("meeting") || normalized.contains("webinar");

    !(has_non_meeting_marker && !has_meeting_marker)
}

#[cfg(target_os = "windows")]
fn process_name_from_pid(pid: u32) -> Option<String> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()? };

    let mut buffer = vec![0u16; 512];
    let mut length = buffer.len() as u32;

    let query_result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    }
    .ok();

    let _ = unsafe { CloseHandle(process) };

    query_result?;

    if length == 0 {
        return None;
    }

    Some(String::from_utf16_lossy(&buffer[..length as usize]).to_ascii_lowercase())
}

#[cfg(target_os = "windows")]
pub fn is_zoom_meeting_active() -> bool {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return false;
    }

    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    if pid == 0 {
        return false;
    }

    let process_name = match process_name_from_pid(pid) {
        Some(name) => name,
        None => return false,
    };

    if !process_name.ends_with("\\zoom.exe") && !process_name.ends_with("/zoom.exe") {
        return false;
    }

    let title_length = unsafe { GetWindowTextLengthW(hwnd) };
    if title_length <= 0 {
        return false;
    }

    let mut title_buffer = vec![0u16; (title_length + 1) as usize];
    let copied = unsafe { GetWindowTextW(hwnd, PWSTR(title_buffer.as_mut_ptr()), title_length + 1) };
    if copied <= 0 {
        return false;
    }

    let title = String::from_utf16_lossy(&title_buffer[..copied as usize]);
    title_indicates_zoom_meeting(&title)
}

#[cfg(not(target_os = "windows"))]
pub fn is_zoom_meeting_active() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::title_indicates_zoom_meeting;

    #[test]
    fn detects_zoom_meeting_titles() {
        assert!(title_indicates_zoom_meeting("Zoom Meeting - Project Sync"));
        assert!(title_indicates_zoom_meeting("Meeting Controls"));
        assert!(title_indicates_zoom_meeting("Zoom Workplace - Quarterly Review"));
        assert!(title_indicates_zoom_meeting("Quarterly Review - Zoom Workplace"));
        assert!(title_indicates_zoom_meeting("Waiting for host - Zoom"));
    }

    #[test]
    fn ignores_non_meeting_titles() {
        assert!(!title_indicates_zoom_meeting("Zoom Workplace"));
        assert!(!title_indicates_zoom_meeting("Chat - Zoom Workplace"));
        assert!(!title_indicates_zoom_meeting("Calendar - Zoom Workplace"));
        assert!(!title_indicates_zoom_meeting("Spotify"));
    }
}
