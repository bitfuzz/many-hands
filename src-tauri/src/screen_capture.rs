#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use std::ffi::CStr;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use std::os::raw::{c_char, c_int};

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[repr(C)]
struct SystemAudioCaptureResponse {
    samples: *mut f32,
    sample_count: u64,
    success: c_int,
    error_message: *mut c_char,
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
extern "C" {
    fn preflight_screen_capture_access() -> c_int;
    fn request_screen_capture_access() -> c_int;
    fn start_system_audio_capture() -> c_int;
    fn stop_system_audio_capture() -> *mut SystemAudioCaptureResponse;
    fn free_system_audio_capture_response(response: *mut SystemAudioCaptureResponse);
}

pub fn is_supported() -> bool {
    cfg!(all(target_os = "macos", target_arch = "aarch64"))
}

pub fn preflight_access() -> bool {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        unsafe { preflight_screen_capture_access() == 1 }
    }

    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        false
    }
}

pub fn request_access() -> bool {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        unsafe { request_screen_capture_access() == 1 }
    }

    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        false
    }
}

pub fn start_capture() -> Result<(), String> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        let started = unsafe { start_system_audio_capture() == 1 };
        if started {
            Ok(())
        } else {
            Err("Failed to start system audio capture".to_string())
        }
    }

    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        Err("System audio capture is not supported on this platform".to_string())
    }
}

pub fn stop_capture() -> Result<Vec<f32>, String> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        let response_ptr = unsafe { stop_system_audio_capture() };
        if response_ptr.is_null() {
            return Err("System audio capture returned a null response".to_string());
        }

        let response = unsafe { &*response_ptr };

        let result = if response.success == 1 {
            if response.samples.is_null() || response.sample_count == 0 {
                Ok(Vec::new())
            } else {
                let len = response.sample_count as usize;
                let samples = unsafe { std::slice::from_raw_parts(response.samples, len) }.to_vec();
                Ok(samples)
            }
        } else if !response.error_message.is_null() {
            let error = unsafe { CStr::from_ptr(response.error_message) }
                .to_string_lossy()
                .into_owned();
            Err(error)
        } else {
            Err("System audio capture failed".to_string())
        };

        unsafe { free_system_audio_capture_response(response_ptr) };

        result
    }

    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        Err("System audio capture is not supported on this platform".to_string())
    }
}
