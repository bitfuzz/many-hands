import AudioToolbox
import CoreGraphics
import CoreMedia
import Dispatch
import Foundation

#if canImport(FoundationModels)
import FoundationModels
#endif

#if canImport(ScreenCaptureKit)
import ScreenCaptureKit
#endif

#if canImport(FoundationModels)
@available(macOS 26.0, *)
@Generable
private struct CleanedTranscript: Sendable {
    let cleanedText: String
}
#endif

// MARK: - Swift implementation for Apple LLM integration
// This file is compiled via Cargo build script for Apple Silicon targets

private typealias ResponsePointer = UnsafeMutablePointer<AppleLLMResponse>

private func duplicateCString(_ text: String) -> UnsafeMutablePointer<CChar>? {
    return text.withCString { basePointer in
        guard let duplicated = strdup(basePointer) else {
            return nil
        }
        return duplicated
    }
}

private func truncatedText(_ text: String, limit: Int) -> String {
    guard limit > 0 else { return text }
    let words = text.split(
        maxSplits: .max,
        omittingEmptySubsequences: true,
        whereSeparator: { $0.isWhitespace || $0.isNewline }
    )
    if words.count <= limit {
        return text
    }
    return words.prefix(limit).joined(separator: " ")
}

@_cdecl("is_apple_intelligence_available")
public func isAppleIntelligenceAvailable() -> Int32 {
#if canImport(FoundationModels)
    guard #available(macOS 26.0, *) else {
        return 0
    }

    let model = SystemLanguageModel.default
    switch model.availability {
    case .available:
        return 1
    case .unavailable:
        return 0
    }
#else
    return 0
#endif
}

@_cdecl("process_text_with_system_prompt_apple")
public func processTextWithSystemPrompt(
    _ systemPrompt: UnsafePointer<CChar>,
    _ userContent: UnsafePointer<CChar>,
    maxTokens: Int32
) -> UnsafeMutablePointer<AppleLLMResponse> {
    let swiftSystemPrompt = String(cString: systemPrompt)
    let swiftUserContent = String(cString: userContent)
    let responsePtr = ResponsePointer.allocate(capacity: 1)
    responsePtr.initialize(to: AppleLLMResponse(response: nil, success: 0, error_message: nil))

#if canImport(FoundationModels)
    guard #available(macOS 26.0, *) else {
        responsePtr.pointee.error_message = duplicateCString(
            "Apple Intelligence requires macOS 26 or newer."
        )
        return responsePtr
    }

    let model = SystemLanguageModel.default
    guard model.availability == .available else {
        responsePtr.pointee.error_message = duplicateCString(
            "Apple Intelligence is not currently available on this device."
        )
        return responsePtr
    }

    let tokenLimit = max(0, Int(maxTokens))
    let semaphore = DispatchSemaphore(value: 0)

    // Thread-safe container to pass results from async task back to calling thread
    final class ResultBox: @unchecked Sendable {
        var response: String?
        var error: String?
    }
    let box = ResultBox()

    Task.detached(priority: .userInitiated) {
        defer { semaphore.signal() }
        do {
            let session = LanguageModelSession(
                model: model,
                instructions: swiftSystemPrompt
            )
            var output: String

            do {
                let structured = try await session.respond(
                    to: swiftUserContent,
                    generating: CleanedTranscript.self
                )
                output = structured.content.cleanedText
            } catch {
                let fallbackGeneration = try await session.respond(to: swiftUserContent)
                output = fallbackGeneration.content
            }

            if tokenLimit > 0 {
                output = truncatedText(output, limit: tokenLimit)
            }
            box.response = output
        } catch {
            box.error = error.localizedDescription
        }
    }

    semaphore.wait()

    // Write to responsePtr on the calling thread after task completes
    if let response = box.response {
        responsePtr.pointee.response = duplicateCString(response)
        responsePtr.pointee.success = 1
    } else {
        responsePtr.pointee.error_message = duplicateCString(box.error ?? "Unknown error")
    }

    return responsePtr
#else
    let msg = "Apple Intelligence is not available in this build (SDK requirement not met)."
    responsePtr.pointee.error_message = duplicateCString(msg)
    return responsePtr
#endif
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

#if canImport(ScreenCaptureKit)
@available(macOS 13.0, *)
private final class SystemAudioCaptureSession: NSObject, SCStreamOutput, SCStreamDelegate {
    private static let targetSampleRate = 16_000.0
    private static let levelBucketCount = 16
    private let stateLock = NSLock()
    private var stream: SCStream?
    private var isCapturing = false
    private var capturedSamples: [Float] = []
    private var latestLevels = Array(repeating: Float(0), count: levelBucketCount)
    private var lastErrorMessage: String?
    private let outputQueue = DispatchQueue(
        label: "com.handy.system-audio-capture-output",
        qos: .userInitiated
    )

    func startCapture() -> Bool {
        stateLock.lock()
        if isCapturing {
            stateLock.unlock()
            return true
        }
        capturedSamples.removeAll(keepingCapacity: true)
        latestLevels = Array(repeating: Float(0), count: Self.levelBucketCount)
        lastErrorMessage = nil
        stateLock.unlock()

        final class StartResultBox: @unchecked Sendable {
            var started = false
            var error: String?
        }
        let box = StartResultBox()
        let semaphore = DispatchSemaphore(value: 0)

        Task.detached(priority: .userInitiated) { [weak self] in
            defer { semaphore.signal() }

            guard let self else {
                box.error = "System audio capture session is unavailable."
                return
            }

            do {
                let shareableContent = try await SCShareableContent.excludingDesktopWindows(
                    false,
                    onScreenWindowsOnly: true
                )

                guard let display = shareableContent.displays.first else {
                    box.error = "No display available for system audio capture."
                    return
                }

                let filter = SCContentFilter(display: display, excludingWindows: [])
                let configuration = SCStreamConfiguration()
                configuration.width = display.width
                configuration.height = display.height
                configuration.minimumFrameInterval = CMTime(value: 1, timescale: 60)
                configuration.queueDepth = 3
                configuration.capturesAudio = true
                configuration.excludesCurrentProcessAudio = false

                let stream = SCStream(filter: filter, configuration: configuration, delegate: self)
                try stream.addStreamOutput(self, type: .audio, sampleHandlerQueue: outputQueue)
                try await stream.startCapture()

                self.stateLock.lock()
                self.stream = stream
                self.isCapturing = true
                self.lastErrorMessage = nil
                self.stateLock.unlock()

                box.started = true
            } catch {
                box.error = error.localizedDescription
            }
        }

        semaphore.wait()

        if !box.started {
            stateLock.lock()
            isCapturing = false
            stream = nil
            if let startError = box.error {
                lastErrorMessage = startError
            }
            stateLock.unlock()
        }

        return box.started
    }

    func stopCapture() -> Result<[Float], String> {
        stateLock.lock()
        let stream = self.stream
        let wasCapturing = isCapturing
        isCapturing = false
        self.stream = nil
        stateLock.unlock()

        guard wasCapturing else {
            return .failure("System audio capture is not active.")
        }

        if let stream {
            final class StopResultBox: @unchecked Sendable {
                var error: String?
            }
            let box = StopResultBox()
            let semaphore = DispatchSemaphore(value: 0)

            Task.detached(priority: .userInitiated) {
                defer { semaphore.signal() }
                do {
                    try await stream.stopCapture()
                } catch {
                    box.error = error.localizedDescription
                }
            }

            semaphore.wait()

            if let stopError = box.error {
                return .failure(stopError)
            }
        }

        stateLock.lock()
        let samples = capturedSamples
        capturedSamples.removeAll(keepingCapacity: false)
        latestLevels = Array(repeating: Float(0), count: Self.levelBucketCount)
        let streamError = lastErrorMessage
        lastErrorMessage = nil
        stateLock.unlock()

        if samples.isEmpty, let streamError {
            return .failure(streamError)
        }

        return .success(samples)
    }

    private func appendSamples(_ samples: [Float]) {
        guard !samples.isEmpty else { return }

        updateLevels(from: samples)

        stateLock.lock()
        capturedSamples.append(contentsOf: samples)
        stateLock.unlock()
    }

    private func updateLevels(from samples: [Float]) {
        guard !samples.isEmpty else { return }

        let bucketCount = Self.levelBucketCount
        let bucketSize = max(1, samples.count / bucketCount)
        var nextLevels = Array(repeating: Float(0), count: bucketCount)

        for bucketIndex in 0..<bucketCount {
            let start = bucketIndex * bucketSize
            if start >= samples.count {
                break
            }

            let end = min(samples.count, start + bucketSize)
            var sumSquares: Float = 0
            var peak: Float = 0

            for sample in samples[start..<end] {
                let absValue = Swift.abs(sample)
                peak = max(peak, absValue)
                sumSquares += sample * sample
            }

            let frameCount = Float(max(1, end - start))
            let rms = sqrt(sumSquares / frameCount)
            let energy = max(rms * 2.8, peak)
            nextLevels[bucketIndex] = min(max(energy, 0), 1)
        }

        stateLock.lock()
        for index in 0..<bucketCount {
            latestLevels[index] = max(nextLevels[index], latestLevels[index] * 0.78)
        }
        stateLock.unlock()
    }

    func currentLevels() -> [Float] {
        stateLock.lock()
        let levels = latestLevels
        stateLock.unlock()
        return levels
    }

    private func normalizeSampleRate(_ samples: [Float], inputSampleRate: Double) -> [Float] {
        guard !samples.isEmpty else {
            return samples
        }

        guard inputSampleRate > 0 else {
            return samples
        }

        let targetRate = Self.targetSampleRate
        if abs(inputSampleRate - targetRate) < 0.5 {
            return samples
        }

        let outputCount = Int((Double(samples.count) * targetRate / inputSampleRate).rounded())
        guard outputCount > 1 else {
            return samples
        }

        let step = inputSampleRate / targetRate
        var resampled = Array(repeating: Float(0), count: outputCount)

        for index in 0..<outputCount {
            let sourcePosition = Double(index) * step
            let lowerIndex = Int(sourcePosition)
            let upperIndex = min(lowerIndex + 1, samples.count - 1)
            let fraction = Float(sourcePosition - Double(lowerIndex))

            if lowerIndex >= samples.count {
                resampled[index] = samples[samples.count - 1]
                continue
            }

            let lowerValue = samples[lowerIndex]
            let upperValue = samples[upperIndex]
            resampled[index] = lowerValue + (upperValue - lowerValue) * fraction
        }

        return resampled
    }

    private func extractSamples(from sampleBuffer: CMSampleBuffer) -> [Float]? {
        guard CMSampleBufferIsValid(sampleBuffer) else {
            return nil
        }

        guard
            let formatDescription = CMSampleBufferGetFormatDescription(sampleBuffer),
            let asbdPointer = CMAudioFormatDescriptionGetStreamBasicDescription(formatDescription)
        else {
            return nil
        }

        let asbd = asbdPointer.pointee
        guard asbd.mFormatID == kAudioFormatLinearPCM else {
            return nil
        }

        let frameCount = Int(CMSampleBufferGetNumSamples(sampleBuffer))
        guard frameCount > 0 else {
            return []
        }

        let declaredChannels = Int(max(asbd.mChannelsPerFrame, 1))
        let bufferListSize = MemoryLayout<AudioBufferList>.size
            + max(declaredChannels - 1, 0) * MemoryLayout<AudioBuffer>.size
        let rawBufferList = UnsafeMutableRawPointer.allocate(
            byteCount: bufferListSize,
            alignment: MemoryLayout<AudioBufferList>.alignment
        )
        defer { rawBufferList.deallocate() }

        let audioBufferList = rawBufferList.assumingMemoryBound(to: AudioBufferList.self)
        var blockBuffer: CMBlockBuffer?
        let status = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
            sampleBuffer,
            bufferListSizeNeededOut: nil,
            bufferListOut: audioBufferList,
            bufferListSize: bufferListSize,
            blockBufferAllocator: nil,
            blockBufferMemoryAllocator: nil,
            flags: UInt32(kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment),
            blockBufferOut: &blockBuffer
        )

        guard status == noErr else {
            return nil
        }

        let buffers = UnsafeMutableAudioBufferListPointer(audioBufferList)
        guard !buffers.isEmpty else {
            return nil
        }

        let isFloat = (asbd.mFormatFlags & kAudioFormatFlagIsFloat) != 0
        let isSignedInteger = (asbd.mFormatFlags & kAudioFormatFlagIsSignedInteger) != 0
        let isNonInterleaved = (asbd.mFormatFlags & kAudioFormatFlagIsNonInterleaved) != 0
        let bitsPerChannel = Int(asbd.mBitsPerChannel)
        let channelCount = max(
            1,
            isNonInterleaved ? min(buffers.count, declaredChannels) : declaredChannels
        )

        var mono = Array(repeating: Float(0), count: frameCount)

        if isFloat && bitsPerChannel == 32 {
            if isNonInterleaved {
                for frame in 0..<frameCount {
                    var mixed: Float = 0
                    for channel in 0..<channelCount {
                        guard let data = buffers[channel].mData else { continue }
                        mixed += data.assumingMemoryBound(to: Float.self)[frame]
                    }
                    mono[frame] = mixed / Float(channelCount)
                }
            } else {
                guard let data = buffers[0].mData else {
                    return nil
                }
                let interleaved = data.assumingMemoryBound(to: Float.self)
                for frame in 0..<frameCount {
                    let base = frame * channelCount
                    var mixed: Float = 0
                    for channel in 0..<channelCount {
                        mixed += interleaved[base + channel]
                    }
                    mono[frame] = mixed / Float(channelCount)
                }
            }

            return normalizeSampleRate(mono, inputSampleRate: asbd.mSampleRate)
        }

        if isSignedInteger && bitsPerChannel == 16 {
            let normalize = Float(Int16.max)

            if isNonInterleaved {
                for frame in 0..<frameCount {
                    var mixed: Float = 0
                    for channel in 0..<channelCount {
                        guard let data = buffers[channel].mData else { continue }
                        let sample = data.assumingMemoryBound(to: Int16.self)[frame]
                        mixed += Float(sample) / normalize
                    }
                    mono[frame] = mixed / Float(channelCount)
                }
            } else {
                guard let data = buffers[0].mData else {
                    return nil
                }
                let interleaved = data.assumingMemoryBound(to: Int16.self)
                for frame in 0..<frameCount {
                    let base = frame * channelCount
                    var mixed: Float = 0
                    for channel in 0..<channelCount {
                        mixed += Float(interleaved[base + channel]) / normalize
                    }
                    mono[frame] = mixed / Float(channelCount)
                }
            }

            return normalizeSampleRate(mono, inputSampleRate: asbd.mSampleRate)
        }

        if isSignedInteger && bitsPerChannel == 32 {
            let normalize = Float(Int32.max)

            if isNonInterleaved {
                for frame in 0..<frameCount {
                    var mixed: Float = 0
                    for channel in 0..<channelCount {
                        guard let data = buffers[channel].mData else { continue }
                        let sample = data.assumingMemoryBound(to: Int32.self)[frame]
                        mixed += Float(sample) / normalize
                    }
                    mono[frame] = mixed / Float(channelCount)
                }
            } else {
                guard let data = buffers[0].mData else {
                    return nil
                }
                let interleaved = data.assumingMemoryBound(to: Int32.self)
                for frame in 0..<frameCount {
                    let base = frame * channelCount
                    var mixed: Float = 0
                    for channel in 0..<channelCount {
                        mixed += Float(interleaved[base + channel]) / normalize
                    }
                    mono[frame] = mixed / Float(channelCount)
                }
            }

            return normalizeSampleRate(mono, inputSampleRate: asbd.mSampleRate)
        }

        return nil
    }

    func stream(
        _ stream: SCStream,
        didOutputSampleBuffer sampleBuffer: CMSampleBuffer,
        of outputType: SCStreamOutputType
    ) {
        guard outputType == .audio else {
            return
        }

        guard let samples = extractSamples(from: sampleBuffer) else {
            return
        }

        appendSamples(samples)
    }

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        stateLock.lock()
        lastErrorMessage = error.localizedDescription
        isCapturing = false
        self.stream = nil
        stateLock.unlock()
    }
}
#endif

private let systemAudioCaptureSession: AnyObject? = {
    #if canImport(ScreenCaptureKit)
        if #available(macOS 13.0, *) {
            return SystemAudioCaptureSession()
        }
    #endif
    return nil
}()

#if canImport(ScreenCaptureKit)
private func withSystemAudioSession<T>(_ body: (SystemAudioCaptureSession) -> T) -> T? {
    if #available(macOS 13.0, *),
        let session = systemAudioCaptureSession as? SystemAudioCaptureSession
    {
        return body(session)
    }
    return nil
}
#endif

private typealias SystemAudioResponsePointer = UnsafeMutablePointer<SystemAudioCaptureResponse>

private func makeSystemAudioSuccessResponse(_ samples: [Float]) -> SystemAudioResponsePointer {
    let responsePtr = SystemAudioResponsePointer.allocate(capacity: 1)

    let samplePtr: UnsafeMutablePointer<Float>?
    if samples.isEmpty {
        samplePtr = nil
    } else {
        let allocated = UnsafeMutablePointer<Float>.allocate(capacity: samples.count)
        allocated.initialize(from: samples, count: samples.count)
        samplePtr = allocated
    }

    responsePtr.initialize(
        to: SystemAudioCaptureResponse(
            samples: samplePtr,
            sample_count: UInt64(samples.count),
            success: 1,
            error_message: nil
        )
    )

    return responsePtr
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

@_cdecl("preflight_screen_capture_access")
public func preflightScreenCaptureAccess() -> Int32 {
    if #available(macOS 10.15, *) {
        return CGPreflightScreenCaptureAccess() ? 1 : 0
    }

    return 0
}

@_cdecl("request_screen_capture_access")
public func requestScreenCaptureAccess() -> Int32 {
    if #available(macOS 10.15, *) {
        if CGPreflightScreenCaptureAccess() {
            return 1
        }

        return CGRequestScreenCaptureAccess() ? 1 : 0
    }

    return 0
}

@_cdecl("start_system_audio_capture")
public func startSystemAudioCapture() -> Int32 {
    #if canImport(ScreenCaptureKit)
        guard let started = withSystemAudioSession({ $0.startCapture() }) else {
            return 0
        }

        return started ? 1 : 0
    #else
        return 0
    #endif
}

@_cdecl("stop_system_audio_capture")
public func stopSystemAudioCapture() -> UnsafeMutablePointer<SystemAudioCaptureResponse> {
    #if canImport(ScreenCaptureKit)
        guard let result = withSystemAudioSession({ $0.stopCapture() }) else {
            return makeSystemAudioErrorResponse(
                "System audio capture is not supported on this macOS version."
            )
        }

        switch result {
        case .success(let samples):
            return makeSystemAudioSuccessResponse(samples)
        case .failure(let error):
            return makeSystemAudioErrorResponse(error)
        }
    #else
        return makeSystemAudioErrorResponse(
            "System audio capture is not available in this build."
        )
    #endif
}

@_cdecl("get_system_audio_levels")
public func getSystemAudioLevels(
    _ outLevels: UnsafeMutablePointer<Float>?,
    _ capacity: UInt64
) -> Int32 {
    guard let outLevels, capacity > 0 else {
        return 0
    }

    #if canImport(ScreenCaptureKit)
        guard let levels = withSystemAudioSession({ $0.currentLevels() }) else {
            return 0
        }

        let copiedCount = min(Int(capacity), levels.count)
        guard copiedCount > 0 else {
            return 0
        }

        for index in 0..<copiedCount {
            outLevels[index] = levels[index]
        }

        return Int32(copiedCount)
    #else
        return 0
    #endif
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