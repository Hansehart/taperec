// Records the main display through ScreenCaptureKit into a fragmented H.264 .mov on the epoch clock: sck-probe OUT SECONDS
import AVFoundation
import CoreMedia
import Foundation
import ScreenCaptureKit
import VideoToolbox

func log(_ text: String) {
    FileHandle.standardError.write(Data((text + "\n").utf8))
}

/// Whether VideoToolbox encodes 1080p H.264 in hardware here.
func hardwareEncoder() -> String {
    var session: VTCompressionSession?
    let status = VTCompressionSessionCreate(
        allocator: nil, width: 1920, height: 1080, codecType: kCMVideoCodecType_H264,
        encoderSpecification: nil, imageBufferAttributes: nil, compressedDataAllocator: nil,
        outputCallback: nil, refcon: nil, compressionSessionOut: &session)
    guard status == noErr, let session else { return "no session (\(status))" }
    var value: CFTypeRef?
    VTSessionCopyProperty(session, key: kVTCompressionPropertyKey_UsingHardwareAcceleratedVideoEncoder,
                          allocator: nil, valueOut: &value)
    VTCompressionSessionInvalidate(session)
    return "hardware=\(String(describing: value))"
}

final class Recorder: NSObject, SCStreamOutput, SCStreamDelegate {
    let writer: AVAssetWriter
    let input: AVAssetWriterInput
    let queue = DispatchQueue(label: "frames")
    var offset: CMTime?
    var started = false
    var complete = 0, skipped = 0, busy = 0, firstLatency = 0.0
    var last: CMSampleBuffer?
    var firstPTS = CMTime.zero

    init(out: URL) throws {
        try? FileManager.default.removeItem(at: out)
        writer = try AVAssetWriter(outputURL: out, fileType: .mov)
        writer.movieFragmentInterval = CMTime(seconds: 1, preferredTimescale: 600)
        input = AVAssetWriterInput(mediaType: .video, outputSettings: [
            AVVideoCodecKey: AVVideoCodecType.h264,
            AVVideoWidthKey: 1920,
            AVVideoHeightKey: 1080,
            AVVideoCompressionPropertiesKey: [
                AVVideoAverageBitRateKey: 12_000_000,
                AVVideoExpectedSourceFrameRateKey: 30,
                AVVideoMaxKeyFrameIntervalKey: 30,
            ],
        ])
        input.expectsMediaDataInRealTime = true
        writer.add(input)
        super.init()
    }

    func stream(_ stream: SCStream, didOutputSampleBuffer buffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen, buffer.isValid else { return }
        guard let infos = CMSampleBufferGetSampleAttachmentsArray(buffer, createIfNecessary: false)
                as? [[SCStreamFrameInfo: Any]],
              let raw = infos.first?[.status] as? Int, SCFrameStatus(rawValue: raw) == .complete
        else { skipped += 1; return }
        complete += 1
        let host = buffer.presentationTimeStamp
        let hostNow = CMClockGetTime(CMClockGetHostTimeClock())
        if offset == nil {
            let epochNow = CMTime(seconds: Date().timeIntervalSince1970, preferredTimescale: 1_000_000)
            offset = CMTimeSubtract(epochNow, hostNow)
            firstLatency = CMTimeSubtract(hostNow, host).seconds
        }
        let pts = CMTimeAdd(host, offset!)
        if !started {
            let tag = AVMutableMetadataItem()
            tag.keySpace = .quickTimeMetadata
            tag.key = "com.taperec.start" as NSString
            tag.value = String(format: "%.6f", pts.seconds) as NSString
            writer.metadata = [tag]
            firstPTS = pts
            writer.startWriting()
            writer.startSession(atSourceTime: pts)
            started = true
            log(String(format: "first frame epoch %.6f, %.1f ms after capture", pts.seconds, firstLatency * 1000))
        }
        guard input.isReadyForMoreMediaData else {
            busy += 1
            log(String(format: "busy at +%.2f s", CMTimeSubtract(pts, firstPTS).seconds))
            return
        }
        var timing = CMSampleTimingInfo(duration: .invalid, presentationTimeStamp: pts, decodeTimeStamp: .invalid)
        var retimed: CMSampleBuffer?
        CMSampleBufferCreateCopyWithNewTiming(allocator: nil, sampleBuffer: buffer, sampleTimingEntryCount: 1,
                                              sampleTimingArray: &timing, sampleBufferOut: &retimed)
        guard let retimed else { return }
        last = retimed
        if !input.append(retimed) {
            log("append failed: \(String(describing: writer.error))")
        }
    }

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        log("stream stopped: \(error)")
        exit(1)
    }

    func finish() {
        queue.async {
            self.log("frames: \(self.complete) complete, \(self.skipped) skipped, \(self.busy) dropped busy")
            guard self.started else { self.log("no frames"); exit(1) }
            // The screen may have stood still since the last frame; showing it again makes the file last until now.
            let now = CMTimeAdd(CMClockGetTime(CMClockGetHostTimeClock()), self.offset!)
            var timing = CMSampleTimingInfo(duration: .invalid, presentationTimeStamp: now, decodeTimeStamp: .invalid)
            var again: CMSampleBuffer?
            if let last = self.last {
                CMSampleBufferCreateCopyWithNewTiming(allocator: nil, sampleBuffer: last, sampleTimingEntryCount: 1,
                                                      sampleTimingArray: &timing, sampleBufferOut: &again)
            }
            while !self.input.isReadyForMoreMediaData { usleep(10_000) }
            if let again, !self.input.append(again) { self.log("final append failed") }
            self.log(String(format: "stopped at +%.2f s", CMTimeSubtract(now, self.firstPTS).seconds))
            self.input.markAsFinished()
            self.writer.finishWriting {
                self.log("finished: \(self.writer.status.rawValue) \(String(describing: self.writer.error))")
                exit(self.writer.status == .completed ? 0 : 1)
            }
        }
    }

    func log(_ text: String) { FileHandle.standardError.write(Data((text + "\n").utf8)) }
}

let out = URL(fileURLWithPath: CommandLine.arguments[1])
let seconds = Double(CommandLine.arguments[2]) ?? 20
log("encoder: \(hardwareEncoder())")
let recorder = try Recorder(out: out)
var stream: SCStream?

func stop() {
    Task {
        try? await stream?.stopCapture()
        recorder.finish()
    }
}

Task {
    do {
        let started = Date()
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
        guard let display = content.displays.first else { log("no display"); exit(1) }
        let config = SCStreamConfiguration()
        config.width = display.width * 2 > 1920 ? 1920 : display.width
        config.height = display.width * 2 > 1920 ? 1080 : display.height
        config.minimumFrameInterval = CMTime(value: 1, timescale: 30)
        config.pixelFormat = kCVPixelFormatType_32BGRA
        config.queueDepth = 8
        config.showsCursor = true
        if #available(macOS 15.0, *) { config.showMouseClicks = true }
        let filter = SCContentFilter(display: display, excludingWindows: [])
        let capture = SCStream(filter: filter, configuration: config, delegate: recorder)
        try capture.addStreamOutput(recorder, type: .screen, sampleHandlerQueue: recorder.queue)
        try await capture.startCapture()
        stream = capture
        log(String(format: "capturing display %d %dx%d after %.2f s", display.displayID, display.width,
                   display.height, Date().timeIntervalSince(started)))
    } catch {
        log("capture failed: \(error)")
        exit(1)
    }
}

// `q` or end of input stops, as for ffmpeg; so does the time limit.
Thread {
    while let line = readLine(), line != "q" {}
    stop()
}.start()
DispatchQueue.main.asyncAfter(deadline: .now() + seconds) { stop() }
dispatchMain()
