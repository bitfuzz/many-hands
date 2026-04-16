// Stub implementation when FoundationModels is not available.
// Keep this file free of framework imports so it can compile on minimal
// CommandLineTools setups where Foundation module resolution can fail.

@frozen
public struct AppleLLMResponse {
    public var response: UnsafeMutablePointer<CChar>?
    public var success: Int32
    public var error_message: UnsafeMutablePointer<CChar>?
}

@frozen
public struct SystemAudioCaptureResponse {
    public var samples: UnsafeMutablePointer<Float>?
    public var sample_count: UInt64
    public var success: Int32
    public var error_message: UnsafeMutablePointer<CChar>?
}

private typealias ResponsePointer = UnsafeMutablePointer<AppleLLMResponse>
private typealias SystemAudioResponsePointer = UnsafeMutablePointer<SystemAudioCaptureResponse>

private func duplicateCString(_ text: String) -> UnsafeMutablePointer<CChar>? {
    let utf8 = text.utf8CString
    let ptr = UnsafeMutablePointer<CChar>.allocate(capacity: utf8.count)
    ptr.initialize(from: utf8, count: utf8.count)
    return ptr
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
    0
}

@_cdecl("process_text_with_system_prompt_apple")
public func processTextWithSystemPrompt(
    _ systemPrompt: UnsafePointer<CChar>,
    _ userContent: UnsafePointer<CChar>,
    maxTokens: Int32
) -> UnsafeMutablePointer<AppleLLMResponse> {
    let responsePtr = ResponsePointer.allocate(capacity: 1)
    responsePtr.initialize(to: AppleLLMResponse(response: nil, success: 0, error_message: nil))

    let msg = "Apple Intelligence is not available in this build (SDK requirement not met)."
    responsePtr.pointee.error_message = duplicateCString(msg)

    return responsePtr
}

@_cdecl("free_apple_llm_response")
public func freeAppleLLMResponse(_ response: UnsafeMutablePointer<AppleLLMResponse>?) {
    guard let response = response else { return }

    if let responseStr = response.pointee.response {
        responseStr.deallocate()
    }

    if let errorStr = response.pointee.error_message {
        errorStr.deallocate()
    }

    response.deallocate()
}

@_cdecl("preflight_screen_capture_access")
public func preflightScreenCaptureAccess() -> Int32 {
    0
}

@_cdecl("request_screen_capture_access")
public func requestScreenCaptureAccess() -> Int32 {
    0
}

@_cdecl("start_system_audio_capture")
public func startSystemAudioCapture() -> Int32 {
    0
}

@_cdecl("stop_system_audio_capture")
public func stopSystemAudioCapture() -> UnsafeMutablePointer<SystemAudioCaptureResponse> {
    makeSystemAudioErrorResponse("System audio capture is unavailable in this build.")
}

@_cdecl("free_system_audio_capture_response")
public func freeSystemAudioCaptureResponse(
    _ response: UnsafeMutablePointer<SystemAudioCaptureResponse>?
) {
    guard let response = response else { return }

    if let samples = response.pointee.samples {
        samples.deallocate()
    }

    if let errorStr = response.pointee.error_message {
        errorStr.deallocate()
    }

    response.deallocate()
}
