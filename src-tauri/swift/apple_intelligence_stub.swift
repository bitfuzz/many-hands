import Foundation

// Stub implementation when FoundationModels is not available
// This file is compiled via Cargo build script when the build environment
// does not support Apple Intelligence (e.g. older Xcode/SDK).

private typealias ResponsePointer = UnsafeMutablePointer<AppleLLMResponse>
private typealias SystemAudioResponsePointer = UnsafeMutablePointer<SystemAudioCaptureResponse>

private func duplicateCString(_ text: String) -> UnsafeMutablePointer<CChar>? {
    return text.withCString { basePointer in
        guard let duplicated = strdup(basePointer) else {
            return nil
        }
        return duplicated
    }
}

private func makeSystemAudioErrorResponse(_ message: String) -> SystemAudioResponsePointer {
    let responsePtr = SystemAudioResponsePointer.allocate(capacity: 1)
    responsePtr.initialize(
        to: SystemAudioCaptureResponse(
            samples: nil,
            sample_count: 0,
            success: 0,
            error_message: duplicateCString(message)
        )
    )
    return responsePtr
}

@_cdecl("is_apple_intelligence_available")
public func isAppleIntelligenceAvailable() -> Int32 {
    return 0
}

@_cdecl("process_text_with_system_prompt_apple")
public func processTextWithSystemPrompt(
    _ systemPrompt: UnsafePointer<CChar>,
    _ userContent: UnsafePointer<CChar>,
    maxTokens: Int32
) -> UnsafeMutablePointer<AppleLLMResponse> {
    let responsePtr = ResponsePointer.allocate(capacity: 1)
    // Initialize with safe defaults
    responsePtr.initialize(to: AppleLLMResponse(response: nil, success: 0, error_message: nil))
    
    let msg = "Apple Intelligence is not available in this build (SDK requirement not met)."
    
    // Duplicate the string for the C caller to own
    responsePtr.pointee.error_message = strdup(msg)
    
    return responsePtr
}

@_cdecl("free_apple_llm_response")
public func freeAppleLLMResponse(_ response: UnsafeMutablePointer<AppleLLMResponse>?) {
    guard let response = response else { return }
    
    if let responseStr = response.pointee.response {
        free(UnsafeMutablePointer(mutating: responseStr))
    }
    
    if let errorStr = response.pointee.error_message {
        free(UnsafeMutablePointer(mutating: errorStr))
    }
    
    response.deallocate()
}

@_cdecl("preflight_screen_capture_access")
public func preflightScreenCaptureAccess() -> Int32 {
    return 0
}

@_cdecl("request_screen_capture_access")
public func requestScreenCaptureAccess() -> Int32 {
    return 0
}

@_cdecl("start_system_audio_capture")
public func startSystemAudioCapture() -> Int32 {
    return 0
}

@_cdecl("stop_system_audio_capture")
public func stopSystemAudioCapture() -> UnsafeMutablePointer<SystemAudioCaptureResponse> {
    return makeSystemAudioErrorResponse("System audio capture is unavailable in this build.")
}

@_cdecl("free_system_audio_capture_response")
public func freeSystemAudioCaptureResponse(
    _ response: UnsafeMutablePointer<SystemAudioCaptureResponse>?
) {
    guard let response else { return }

    if let samples = response.pointee.samples {
        samples.deallocate()
    }

    if let errorStr = response.pointee.error_message {
        free(UnsafeMutablePointer(mutating: errorStr))
    }

    response.deallocate()
}
