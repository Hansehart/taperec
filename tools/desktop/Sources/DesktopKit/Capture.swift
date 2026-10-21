// Records the main display through ScreenCaptureKit into an H.264 movie stamped with wall-clock time.
#if os(macOS)
import AVFoundation
import CoreMedia
import Foundation
import ScreenCaptureKit
import VideoToolbox

/// The metadata key holding the wall-clock time of the first frame; taperec's `recorder::START_TAG`.
let startTag = "com.taperec.start"
/// Frames per second; taperec's `recorder::FPS`.
let framesPerSecond: Int32 = 30
/// How often a still screen is shown again.
let stillInterval = CMTime(value: 1, timescale: 10)

/// Writes complete frames as they come, keeping a frame the writer is too busy for until the next callback.
final class ScreenWriter: NSObject, SCStreamOutput, SCStreamDelegate {
    let writer: AVAssetWriter
    let input: AVAssetWriterInput
    let queue = DispatchQueue(label: "taperec.capture")
    /// Wall-clock time minus host time, fixed at the first frame.
    var offset: CMTime?
    var last: CMSampleBuffer?
    var pending: CMSampleBuffer?
    /// When a frame last went into the movie.
    var appended = CMTime.zero

    init(out: URL, width: Int, height: Int) throws {
        try? FileManager.default.removeItem(at: out)
        writer = try AVAssetWriter(outputURL: out, fileType: .mov)
        // Fragments reach the disk each second, so a killed recorder leaves a readable file.
        writer.movieFragmentInterval = CMTime(value: 1, timescale: 1)
        input = AVAssetWriterInput(mediaType: .video, outputSettings: [
            AVVideoCodecKey: AVVideoCodecType.h264,
            AVVideoWidthKey: width,
            AVVideoHeightKey: height,
            AVVideoCompressionPropertiesKey: [
                AVVideoAverageBitRateKey: 12_000_000,
                AVVideoExpectedSourceFrameRateKey: framesPerSecond,
                AVVideoMaxKeyFrameIntervalKey: framesPerSecond,
                AVVideoAllowFrameReorderingKey: false,
            ],
        ])
        input.expectsMediaDataInRealTime = true
        writer.add(input)
    }

    /// Now on the frames' clock.
    func now() -> CMTime? {
        offset.map { CMTimeAdd(CMClockGetTime(CMClockGetHostTimeClock()), $0) }
    }

    func stream(_ stream: SCStream, didOutputSampleBuffer buffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen, buffer.isValid else { return }
        flush()
        // Only changed screens arrive as complete frames; idle ones carry no image.
        guard let infos = CMSampleBufferGetSampleAttachmentsArray(buffer, createIfNecessary: false)
                as? [[SCStreamFrameInfo: Any]],
              let status = infos.first?[.status] as? Int, SCFrameStatus(rawValue: status) == .complete,
              let frame = retimed(buffer)
        else { return }
        last = frame
        if input.isReadyForMoreMediaData {
            append(frame)
        } else {
            pending = frame
        }
    }

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        write("screen capture stopped: \(error)\n", to: .standardError)
        exit(Exit.failed.rawValue)
    }

    /// `buffer` moved from host time to wall-clock time; the first frame starts the movie.
    func retimed(_ buffer: CMSampleBuffer) -> CMSampleBuffer? {
        if offset == nil {
            let wall = CMTime(seconds: Date().timeIntervalSince1970, preferredTimescale: 1_000_000)
            offset = CMTimeSubtract(wall, CMClockGetTime(CMClockGetHostTimeClock()))
        }
        guard let frame = buffer.retimed(to: CMTimeAdd(buffer.presentationTimeStamp, offset!)) else { return nil }
        if writer.status == .unknown {
            let tag = AVMutableMetadataItem()
            tag.keySpace = .quickTimeMetadata
            tag.key = startTag as NSString
            tag.value = String(format: "%.6f", frame.presentationTimeStamp.seconds) as NSString
            writer.metadata = [tag]
            writer.startWriting()
            writer.startSession(atSourceTime: frame.presentationTimeStamp)
            write("recording\n", to: .standardOutput)
        }
        return frame
    }

    /// Appends the frame the writer was too busy for, once it is ready.
    func flush() {
        guard let waiting = pending, input.isReadyForMoreMediaData else { return }
        pending = nil
        append(waiting)
    }

    /// Shows a still screen again every tenth of a second, so frames keep reaching the disk; Intel's software
    /// encoder holds back nine frames, a second at this rate.
    ///
    /// Each copy trails the clock by a quarter second, so frames captured meanwhile still come later.
    func heartbeat() {
        flush()
        guard let last, pending == nil, input.isReadyForMoreMediaData, let now = now(),
              CMTimeSubtract(now, appended).seconds >= 0.25,
              let again = last.retimed(to: CMTimeAdd(appended, stillInterval))
        else { return }
        append(again)
    }

    /// Appends `frame` if it comes after the last one; the writer fails for good on a time going backwards.
    func append(_ frame: CMSampleBuffer) {
        guard CMTimeCompare(frame.presentationTimeStamp, appended) > 0 else { return }
        appended = frame.presentationTimeStamp
        if !input.append(frame) {
            write("writing the \(describe(frame)) failed: \(String(describing: writer.error))\n",
                  to: .standardError)
            exit(Exit.failed.rawValue)
        }
    }

    /// Shows the last frame again at the stop, so a still screen lasts to the end, then closes the movie.
    func finish() {
        queue.async {
            guard let last = self.last, let end = self.now() else {
                write("no frame was captured\n", to: .standardError)
                exit(Exit.failed.rawValue)
            }
            while !self.input.isReadyForMoreMediaData { usleep(10_000) }
            if let waiting = self.pending { self.append(waiting) }
            while !self.input.isReadyForMoreMediaData { usleep(10_000) }
            if let again = last.retimed(to: end) { self.append(again) }
            self.input.markAsFinished()
            self.writer.endSession(atSourceTime: end)
            self.writer.finishWriting {
                guard self.writer.status == .completed else {
                    write("finishing the movie failed: \(String(describing: self.writer.error))\n", to: .standardError)
                    exit(Exit.failed.rawValue)
                }
                exit(Exit.ok.rawValue)
            }
        }
    }
}

/// A frame's time, size and pixel format, for error messages.
func describe(_ frame: CMSampleBuffer) -> String {
    guard let pixels = frame.imageBuffer else { return "frame at \(frame.presentationTimeStamp.seconds) without pixels" }
    let format = CVPixelBufferGetPixelFormatType(pixels)
    let code = String((0..<4).map { Character(UnicodeScalar(UInt8((format >> (24 - 8 * $0)) & 0xff))) })
    return "frame at \(frame.presentationTimeStamp.seconds), \(CVPixelBufferGetWidth(pixels))x"
        + "\(CVPixelBufferGetHeight(pixels)) \(code)"
}

/// Whether VideoToolbox encodes 1080p H.264 in hardware here.
func hardwareEncoder() -> Bool {
    var session: VTCompressionSession?
    let status = VTCompressionSessionCreate(
        allocator: nil, width: 1920, height: 1080, codecType: kCMVideoCodecType_H264,
        encoderSpecification: nil, imageBufferAttributes: nil, compressedDataAllocator: nil,
        outputCallback: nil, refcon: nil, compressionSessionOut: &session)
    guard status == noErr, let session else { return false }
    defer { VTCompressionSessionInvalidate(session) }
    var used: CFBoolean?
    let found = VTSessionCopyProperty(session, key: kVTCompressionPropertyKey_UsingHardwareAcceleratedVideoEncoder,
                                      allocator: nil, valueOut: &used)
    return found == noErr && used == kCFBooleanTrue
}

extension CMSampleBuffer {
    /// A copy presented at `time`.
    func retimed(to time: CMTime) -> CMSampleBuffer? {
        var timing = CMSampleTimingInfo(duration: .invalid, presentationTimeStamp: time, decodeTimeStamp: .invalid)
        var copy: CMSampleBuffer?
        CMSampleBufferCreateCopyWithNewTiming(allocator: nil, sampleBuffer: self, sampleTimingEntryCount: 1,
                                              sampleTimingArray: &timing, sampleBufferOut: &copy)
        return copy
    }
}

/// Records until `q` or the end of stdin, or SIGINT/SIGTERM, then exits.
func record(to path: String) -> Never {
    let stopped = DispatchSemaphore(value: 0)
    Thread {
        while let line = readLine(), line != "q" {}
        stopped.signal()
    }.start()
    for number in [SIGINT, SIGTERM] {
        signal(number, SIG_IGN)
        let source = DispatchSource.makeSignalSource(signal: number)
        source.setEventHandler { stopped.signal() }
        source.resume()
        sources.append(source)
    }
    Task {
        do {
            let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
            guard let display = content.displays.first(where: { $0.displayID == CGMainDisplayID() })
                    ?? content.displays.first
            else {
                write("no display to record\n", to: .standardError)
                exit(Exit.failed.rawValue)
            }
            let screen = try ScreenWriter(out: URL(fileURLWithPath: path), width: display.width, height: display.height)
            let config = SCStreamConfiguration()
            config.width = display.width
            config.height = display.height
            config.minimumFrameInterval = CMTime(value: 1, timescale: framesPerSecond)
            // The encoder's own 4:2:0 input, so no pixel transfer stands between capture and encoding.
            config.pixelFormat = kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
            config.colorMatrix = CGDisplayStream.yCbCrMatrix_ITU_R_709_2
            config.colorSpaceName = CGColorSpace.sRGB
            config.queueDepth = 8
            config.showsCursor = true
            if #available(macOS 15.0, *) { config.showMouseClicks = true }
            let stream = SCStream(filter: SCContentFilter(display: display, excludingWindows: []),
                                  configuration: config, delegate: screen)
            try stream.addStreamOutput(screen, type: .screen, sampleHandlerQueue: screen.queue)
            try await stream.startCapture()
            let beat = DispatchSource.makeTimerSource(queue: screen.queue)
            beat.schedule(deadline: .now() + stillInterval.seconds, repeating: stillInterval.seconds)
            beat.setEventHandler { screen.heartbeat() }
            beat.resume()
            heartbeats = beat
            DispatchQueue.global().async {
                stopped.wait()
                Task {
                    try? await stream.stopCapture()
                    screen.finish()
                }
            }
        } catch {
            write("screen capture failed: \(error)\n", to: .standardError)
            exit(Exit.failed.rawValue)
        }
    }
    dispatchMain()
}

/// Signal sources and the heartbeat timer live as long as the recording.
var sources: [DispatchSourceSignal] = []
var heartbeats: DispatchSourceTimer?
#endif
