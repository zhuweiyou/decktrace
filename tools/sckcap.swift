// 常驻 ScreenCaptureKit 采集器。
//
// 用法:
//   sckcap --window-id 3552 --out frames/latest.jpg --fps 15 [--quality 0.88] [--duration 10]
//
// 只做一件事：按固定帧率把最新窗口画面原子写入 --out。
// Python 侧轮询文件 mtime，不再每帧启动 screencapture。

import Foundation
import AppKit
import ScreenCaptureKit
import CoreMedia
import CoreImage
import ImageIO
import UniformTypeIdentifiers
import Darwin

struct Options {
    var windowID: CGWindowID = 0
    var outPath: String = "latest.jpg"
    var fps: Double = 15
    var quality: Double = 0.88
    var duration: Double = 0
    var stream = false

    static func parse() -> Options {
        var opts = Options()
        var args = CommandLine.arguments.dropFirst().makeIterator()
        while let arg = args.next() {
            switch arg {
            case "--window-id":
                if let value = args.next(), let id = UInt32(value) {
                    opts.windowID = CGWindowID(id)
                }
            case "--out":
                if let value = args.next() { opts.outPath = value }
            case "--fps":
                if let value = args.next(), let fps = Double(value), fps > 0 {
                    opts.fps = min(fps, 30)
                }
            case "--quality":
                if let value = args.next(), let quality = Double(value) {
                    opts.quality = min(max(quality, 0.3), 1.0)
                }
            case "--duration":
                if let value = args.next(), let duration = Double(value) {
                    opts.duration = max(duration, 0)
                }
            case "--stream":
                opts.stream = true
            default:
                break
            }
        }
        return opts
    }
}

func appendLE<T: FixedWidthInteger>(_ value: T, to data: inout Data) {
    var little = value.littleEndian
    withUnsafeBytes(of: &little) { data.append(contentsOf: $0) }
}

final class FrameWriter: NSObject, SCStreamOutput, SCStreamDelegate {
    private let context = CIContext(options: [.useSoftwareRenderer: false])
    private let outURL: URL
    private let tmpURL: URL
    private let quality: Double
    private let streamMode: Bool
    private let lock = NSLock()
    private var stream: SCStream?
    private var frameCount = 0
    private var startedAt = Date()
    private var lastReport = Date()
    private var lastFrameAt = Date()

    init(outURL: URL, quality: Double, streamMode: Bool) {
        self.outURL = outURL
        self.tmpURL = outURL.deletingLastPathComponent()
            .appendingPathComponent("sck_\(getpid()).tmp.jpg")
        self.quality = quality
        self.streamMode = streamMode
        super.init()
    }

    func attach(_ stream: SCStream) { self.stream = stream }

    func stream(
        _ stream: SCStream,
        didOutputSampleBuffer sampleBuffer: CMSampleBuffer,
        of outputType: SCStreamOutputType
    ) {
        guard outputType == .screen else { return }
        guard let pixelBuffer = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }

        let image = CIImage(cvPixelBuffer: pixelBuffer)
        guard let cgImage = context.createCGImage(image, from: image.extent) else { return }

        let now = Date()
        var count = 0
        var report = false

        if streamMode {
            let jpeg = NSMutableData()
            guard let dest = CGImageDestinationCreateWithData(
                jpeg as CFMutableData,
                UTType.jpeg.identifier as CFString,
                1,
                nil
            ) else { return }
            CGImageDestinationAddImage(
                dest,
                cgImage,
                [kCGImageDestinationLossyCompressionQuality: quality] as CFDictionary
            )
            guard CGImageDestinationFinalize(dest) else { return }

            lock.lock()
            frameCount += 1
            count = frameCount
            lastFrameAt = now
            if now.timeIntervalSince(lastReport) >= 5.0 {
                lastReport = now
                report = true
            }
            let elapsed = now.timeIntervalSince(startedAt)
            lock.unlock()

            var header = Data()
            header.append(contentsOf: [0x53, 0x43, 0x4B, 0x31]) // SCK1
            appendLE(UInt64(count), to: &header)
            appendLE(now.timeIntervalSince1970.bitPattern, to: &header)
            appendLE(UInt32(cgImage.width), to: &header)
            appendLE(UInt32(cgImage.height), to: &header)
            appendLE(UInt32(jpeg.length), to: &header)
            do {
                try FileHandle.standardOutput.write(contentsOf: header)
                try FileHandle.standardOutput.write(contentsOf: jpeg as Data)
            } catch {
                exit(1)
            }

            if report {
                let fps = elapsed > 0 ? Double(count) / elapsed : 0
                fputs(
                    String(
                        format: "[sck] frames=%d fps=%.1f size=%dx%d stream=1\n",
                        count, fps, cgImage.width, cgImage.height
                    ),
                    stderr
                )
                fflush(stderr)
            }
            return
        }

        guard let dest = CGImageDestinationCreateWithURL(
            tmpURL as CFURL,
            UTType.jpeg.identifier as CFString,
            1,
            nil
        ) else { return }
        CGImageDestinationAddImage(
            dest,
            cgImage,
            [kCGImageDestinationLossyCompressionQuality: quality] as CFDictionary
        )
        guard CGImageDestinationFinalize(dest) else { return }
        guard rename(tmpURL.path, outURL.path) == 0 else { return }

        lock.lock()
        frameCount += 1
        count = frameCount
        lastFrameAt = now
        if now.timeIntervalSince(lastReport) >= 5.0 {
            lastReport = now
            report = true
        }
        let elapsed = now.timeIntervalSince(startedAt)
        lock.unlock()

        if report {
            let fps = elapsed > 0 ? Double(count) / elapsed : 0
            fputs(
                String(
                    format: "[sck] frames=%d fps=%.1f size=%dx%d\n",
                    count, fps, cgImage.width, cgImage.height
                ),
                stderr
            )
            fflush(stderr)
        }
    }

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        fputs("[sck] stream stopped: \(error)\n", stderr)
        fflush(stderr)
        exit(1)
    }
}

@main
struct Main {
    static func main() async {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.accessory)
        _ = CGMainDisplayID()

        let opts = Options.parse()
        guard opts.windowID != 0 else {
            fputs("missing --window-id\n", stderr)
            exit(2)
        }

        do {
            let content = try await SCShareableContent.excludingDesktopWindows(
                false,
                onScreenWindowsOnly: false
            )
            guard let window = content.windows.first(where: { $0.windowID == opts.windowID }) else {
                fputs("window \(opts.windowID) not found\n", stderr)
                exit(3)
            }

            let outURL = URL(fileURLWithPath: opts.outPath)
            try? FileManager.default.createDirectory(
                at: outURL.deletingLastPathComponent(),
                withIntermediateDirectories: true
            )

            let scale = NSScreen.screens
                .first(where: { $0.frame.intersects(window.frame) })?
                .backingScaleFactor ?? 2.0
            let config = SCStreamConfiguration()
            config.width = max(2, Int(window.frame.width * scale))
            config.height = max(2, Int(window.frame.height * scale))
            config.pixelFormat = kCVPixelFormatType_32BGRA
            config.queueDepth = 3
            config.showsCursor = false
            config.capturesAudio = false
            config.minimumFrameInterval = CMTime(
                value: 1,
                timescale: CMTimeScale(max(1, Int(opts.fps.rounded())))
            )

            fputs(
                "[sck] ready id=\(opts.windowID) frame=\(window.frame) "
                + "pixels=\(config.width)x\(config.height) fps=\(opts.fps)\n",
                stderr
            )
            fflush(stderr)

            let filter = SCContentFilter(desktopIndependentWindow: window)
            signal(SIGPIPE, SIG_IGN)
            let writer = FrameWriter(
                outURL: outURL,
                quality: opts.quality,
                streamMode: opts.stream
            )
            let stream = SCStream(filter: filter, configuration: config, delegate: writer)
            writer.attach(stream)
            try stream.addStreamOutput(
                writer,
                type: .screen,
                sampleHandlerQueue: DispatchQueue(label: "sck.output")
            )
            try await stream.startCapture()

            if opts.duration > 0 {
                try await Task.sleep(
                    nanoseconds: UInt64(opts.duration * 1_000_000_000)
                )
                try await stream.stopCapture()
                exit(0)
            } else {
                while true {
                    try await Task.sleep(nanoseconds: 3600 * 1_000_000_000)
                }
            }
        } catch {
            fputs("[sck] ERROR: \(error)\n", stderr)
            exit(1)
        }
    }
}
