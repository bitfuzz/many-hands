// Stub implementation when FoundationModels is not available.
// Keep this file free of framework imports so it can compile on minimal
// CommandLineTools setups where Foundation module resolution can fail.

@frozen
private struct StubAppleLLMResponse {
    var response: UnsafeMutablePointer<CChar>?
    var success: Int32
    var error_message: UnsafeMutablePointer<CChar>?
}

@frozen
private struct StubSystemAudioCaptureResponse {
    var samples: UnsafeMutablePointer<Float>?
    var sample_count: UInt64
    var success: Int32
    var error_message: UnsafeMutablePointer<CChar>?
}

private typealias ResponsePointer = UnsafeMutablePointer<StubAppleLLMResponse>
private typealias SystemAudioResponsePointer = UnsafeMutablePointer<StubSystemAudioCaptureResponse>

private func duplicateCString(_ text: String) -> UnsafeMutablePointer<CChar>? {
    let utf8 = text.utf8CString
    let ptr = UnsafeMutablePointer<CChar>.allocate(capacity: utf8.count)
    utf8.withUnsafeBufferPointer { buffer in
        guard let baseAddress = buffer.baseAddress else { return }
        ptr.initialize(from: baseAddress, count: utf8.count)
    }
    return ptr
}

private func makeSystemAudioErrorResponse(_ message: String) -> SystemAudioResponsePointer {
    let responsePtr = SystemAudioResponsePointer.allocate(capacity: 1)
    responsePtr.initialize(
        to: StubSystemAudioCaptureResponse(
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
    _ systemPrompt: UnsafePointer<CChar>?,
    _ userContent: UnsafePointer<CChar>?,
    _ maxTokens: Int32
) -> UnsafeMutableRawPointer? {
    let responsePtr = ResponsePointer.allocate(capacity: 1)
    responsePtr.initialize(to: StubAppleLLMResponse(response: nil, success: 0, error_message: nil))

    let msg = "Apple Intelligence is not available in this build (SDK requirement not met)."
    responsePtr.pointee.error_message = duplicateCString(msg)

    return UnsafeMutableRawPointer(responsePtr)
}

@_cdecl("free_apple_llm_response")
public func freeAppleLLMResponse(_ response: UnsafeMutableRawPointer?) {
    guard let response = response else { return }
    let typed = response.assumingMemoryBound(to: StubAppleLLMResponse.self)

    if let responseStr = typed.pointee.response {
        responseStr.deallocate()
    }

    if let errorStr = typed.pointee.error_message {
        errorStr.deallocate()
    }

    typed.deinitialize(count: 1)
    typed.deallocate()
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
public func stopSystemAudioCapture() -> UnsafeMutableRawPointer? {
    UnsafeMutableRawPointer(
        makeSystemAudioErrorResponse("System audio capture is unavailable in this build.")
    )
}

@_cdecl("free_system_audio_capture_response")
public func freeSystemAudioCaptureResponse(
    _ response: UnsafeMutableRawPointer?
) {
    guard let response = response else { return }
    let typed = response.assumingMemoryBound(to: StubSystemAudioCaptureResponse.self)

    if let samples = typed.pointee.samples {
        samples.deallocate()
    }

    if let errorStr = typed.pointee.error_message {
        errorStr.deallocate()
    }

    typed.deinitialize(count: 1)
    typed.deallocate()
}
