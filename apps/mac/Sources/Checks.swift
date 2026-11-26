// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import ImageIO
import UniformTypeIdentifiers

/// Looking at the app and measuring it with nobody at the screen: `--dump` writes the windows
/// as pictures, `--bench` prints what a stretch of time cost. Both end the app.
extension AppDelegate {
    /// Writes every window as `window-<n>-<id>.png`, bottom first, and where each one is.
    func dump(to folder: URL) {
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        var lines = ["skin \(controller.name) zoom \(controller.zoom)"]
        // the visualizer, drawn into the skin window it shows in, as it is seen there
        let vis = controller.option("show-vis") != nil ? visualizer.picture(after: 60) : nil
        let inSkin = visualizer.insetArea
        for (n, window) in controller.pictures().enumerated() {
            let file = folder.appendingPathComponent("window-\(n)-\(window.id).png")
            var image = window.image
            if let inSkin, inSkin.window == window.id, let drawn = vis?.cgImage {
                image = Self.paste(drawn, into: image, at: inSkin.rect) ?? image
                lines.append("visualizer in window \(window.id) at \(inSkin.rect)")
            }
            if let out = CGImageDestinationCreateWithURL(file as CFURL, UTType.png.identifier as CFString, 1, nil) {
                CGImageDestinationAddImage(out, image, nil)
                CGImageDestinationFinalize(out)
            }
            let f = window.frame
            lines.append(
                "window \(n) id \(window.id) frame \(Int(f.minX)),\(Int(f.minY)) \(Int(f.width))x\(Int(f.height))"
                    + " pixels \(window.image.width)x\(window.image.height)")
        }
        if let png = vis?.representation(using: .png, properties: [:]) {
            try? png.write(to: folder.appendingPathComponent("visualizer.png"))
        }
        if controller.option("show-effects") != nil,
            let png = effects.picture()?.representation(using: .png, properties: [:])
        {
            try? png.write(to: folder.appendingPathComponent("effects.png"))
        }
        if controller.option("show-prefs") != nil {
            for (n, page) in preferences.pictures().enumerated() {
                try? page.representation(using: .png, properties: [:])?
                    .write(to: folder.appendingPathComponent("preferences-\(n).png"))
            }
        }
        try? lines.joined(separator: "\n").appending("\n")
            .write(to: folder.appendingPathComponent("windows.txt"), atomically: true, encoding: .utf8)
        exit(0)
    }

    /// `picture` over `image`, scaled into `rect`, in `image`'s pixels from its top left corner.
    private static func paste(_ picture: CGImage, into image: CGImage, at rect: CGRect) -> CGImage? {
        guard
            let context = CGContext(
                data: nil, width: image.width, height: image.height, bitsPerComponent: 8, bytesPerRow: 0,
                space: CGColorSpace(name: CGColorSpace.sRGB)!,
                bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)
        else { return nil }
        let height = CGFloat(image.height)
        context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        context.interpolationQuality = .high
        context.draw(picture, in: CGRect(x: rect.minX, y: height - rect.maxY, width: rect.width, height: rect.height))
        return context.makeImage()
    }

    /// Seconds from the process starting to the skin's first window showing its pixels.
    private var startup: Double {
        var info = kinfo_proc()
        var size = MemoryLayout<kinfo_proc>.stride
        var name = [CTL_KERN, KERN_PROC, KERN_PROC_PID, getpid()]
        guard sysctl(&name, 4, &info, &size, nil, 0) == 0, controller.firstShown > 0 else { return 0 }
        let started = Double(info.kp_proc.p_starttime.tv_sec) + Double(info.kp_proc.p_starttime.tv_usec) / 1e6
        // the first window's moment as a time of day: now, less how long ago it was
        let shown = Date().timeIntervalSince1970 - (ProcessInfo.processInfo.systemUptime - controller.firstShown)
        return shown - started
    }

    /// Lets the skin run for `seconds` and prints what it cost.
    func bench(_ seconds: Double) {
        /// Seconds of processor time so far, in the app's own code and in the system for it.
        func cpu() -> (user: Double, system: Double) {
            var usage = rusage()
            getrusage(RUSAGE_SELF, &usage)
            return (
                Double(usage.ru_utime.tv_sec) + Double(usage.ru_utime.tv_usec) / 1e6,
                Double(usage.ru_stime.tv_sec) + Double(usage.ru_stime.tv_usec) / 1e6
            )
        }
        // the first second is the launch: not counted
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
            let before = (self.controller.stats(), cpu())
            let milkdrop = self.visualizer.frameCost
            DispatchQueue.main.asyncAfter(deadline: .now() + seconds) {
                let after = (self.controller.stats(), cpu())
                let steps = after.0.steps - before.0.steps
                let drawn = after.0.windowsDrawn - before.0.windowsDrawn
                let engine = after.0.engine - before.0.engine
                let draw = after.0.draw - before.0.draw
                var usage = rusage()
                getrusage(RUSAGE_SELF, &usage)
                print(
                    String(
                        format: "%@ zoom %d, %.0f s: %.1f%% of a core (%.1f%% own code, %.1f%% system); "
                            + "%.1f steps/s, %.1f windows drawn/s; "
                            + "engine %.2f ms/s, drawing %.2f ms/s (%.0f us a window); peak memory %.0f MB; "
                            + "%d songs, %llu underruns; first window %.0f ms after the start, "
                            + "of which the skin, the playlist and the first drawing took %.0f ms",
                        self.controller.name, self.controller.zoom, seconds,
                        (after.1.user + after.1.system - before.1.user - before.1.system) / seconds * 100,
                        (after.1.user - before.1.user) / seconds * 100,
                        (after.1.system - before.1.system) / seconds * 100,
                        Double(steps) / seconds, Double(drawn) / seconds,
                        engine / seconds * 1000, draw / seconds * 1000,
                        drawn > 0 ? draw / Double(drawn) * 1e6 : 0, Double(usage.ru_maxrss) / 1_048_576,
                        Int(alpaca_player_count(self.controller.player)),
                        alpaca_player_underruns(self.controller.player), self.startup * 1000,
                        self.controller.startTook * 1000))
                let shown = self.visualizer.frameCost
                if shown.frames > milkdrop.frames {
                    let frames = Double(shown.frames - milkdrop.frames)
                    print(
                        String(
                            format: "Milkdrop: %.1f frames/s, %.2f ms of the main thread a frame",
                            frames / seconds, (shown.seconds - milkdrop.seconds) / frames * 1000))
                }
                exit(0)
            }
        }
    }
}
