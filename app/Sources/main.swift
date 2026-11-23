// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import ImageIO
import UniformTypeIdentifiers

/// The app: its menus, the skin it shows, and what the command line asks for.
///
/// ```
/// Andamp [skin.wsz|skin.wal] [--modern] [--zoom n]
///        [--dump <folder>]     writes every window as a PNG after two seconds, and quits
///        [--bench <seconds>]   prints what that many seconds cost, and quits
/// ```
final class AppDelegate: NSObject, NSApplicationDelegate {
    private let controller: SkinController
    private var options = Options()

    struct Options {
        var skin: URL?
        var modern = false
        var zoom = 1
        var dump: URL?
        var bench: Double?

        init() {
            var args = CommandLine.arguments.dropFirst()
            while let arg = args.popFirst() {
                switch arg {
                case "--modern": modern = true
                case "--zoom": zoom = args.popFirst().flatMap { Int($0) } ?? 1
                case "--dump": dump = args.popFirst().map { URL(fileURLWithPath: $0) }
                case "--bench": bench = args.popFirst().flatMap { Double($0) }
                // the arguments Xcode and macOS add, such as -NSDocumentRevisionsDebugMode YES
                case _ where arg.hasPrefix("-") && !arg.hasPrefix("--"): args = args.dropFirst()
                default: skin = URL(fileURLWithPath: arg)
                }
            }
        }
    }

    override init() {
        controller = SkinController(zoom: options.zoom)
        super.init()
    }

    private var fonts: URL? { Bundle.main.url(forResource: "fonts", withExtension: nil) }

    private func builtin(modern: Bool) -> URL? {
        Bundle.main.url(forResource: "skins", withExtension: nil)?
            .appendingPathComponent(modern ? "walkit-modern.wal" : "walkit-classic.wsz")
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.mainMenu = makeMenu()
        if let skin = options.skin ?? builtin(modern: options.modern) { open(skin) }
        NSApp.activate(ignoringOtherApps: true)
        if let folder = options.dump {
            DispatchQueue.main.asyncAfter(deadline: .now() + 2) { self.dump(to: folder) }
        }
        if let seconds = options.bench { bench(seconds) }
    }

    private func open(_ skin: URL) {
        do {
            try controller.open(file: skin, fonts: fonts)
        } catch {
            if options.dump != nil || options.bench != nil {
                FileHandle.standardError.write(Data("andamp: \(error.localizedDescription)\n".utf8))
                exit(1)
            }
            NSAlert(error: error).runModal()
        }
    }

    // MARK: menus

    private func makeMenu() -> NSMenu {
        let main = NSMenu()
        func add(_ title: String, _ items: [NSMenuItem]) {
            let menu = NSMenu(title: title)
            items.forEach(menu.addItem)
            let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
            item.submenu = menu
            main.addItem(item)
        }
        func item(_ title: String, _ action: Selector, _ key: String = "") -> NSMenuItem {
            let item = NSMenuItem(title: title, action: action, keyEquivalent: key)
            item.target = action == #selector(NSApplication.terminate(_:)) ? NSApp : self
            return item
        }
        add("Andamp", [item("Quit Andamp", #selector(NSApplication.terminate(_:)), "q")])
        add("File", [item("Open Skin…", #selector(openSkin), "o")])
        add("Skins", [
            item("Classic", #selector(showClassic)),
            item("Modern", #selector(showModern)),
        ])
        add("View", [item("Double Size", #selector(toggleDoubleSize), "d")])
        return main
    }

    @objc private func openSkin() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = ["wsz", "wal", "zip"].compactMap { UTType(filenameExtension: $0) }
        panel.allowsMultipleSelection = false
        if panel.runModal() == .OK, let file = panel.url { open(file) }
    }

    @objc private func showClassic() { builtin(modern: false).map(open) }
    @objc private func showModern() { builtin(modern: true).map(open) }
    @objc private func toggleDoubleSize() { controller.setZoom(controller.zoom == 1 ? 2 : 1) }

    // MARK: --dump and --bench

    /// Writes every window as `window-<n>-<id>.png`, bottom first, and where each one is.
    private func dump(to folder: URL) {
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        var lines = ["skin \(controller.name) zoom \(controller.zoom)"]
        for (n, window) in controller.pictures().enumerated() {
            let file = folder.appendingPathComponent("window-\(n)-\(window.id).png")
            if let out = CGImageDestinationCreateWithURL(file as CFURL, UTType.png.identifier as CFString, 1, nil) {
                CGImageDestinationAddImage(out, window.image, nil)
                CGImageDestinationFinalize(out)
            }
            let f = window.frame
            lines.append(
                "window \(n) id \(window.id) frame \(Int(f.minX)),\(Int(f.minY)) \(Int(f.width))x\(Int(f.height))"
                    + " pixels \(window.image.width)x\(window.image.height)")
        }
        try? lines.joined(separator: "\n").appending("\n")
            .write(to: folder.appendingPathComponent("windows.txt"), atomically: true, encoding: .utf8)
        exit(0)
    }

    /// Lets the skin run for `seconds` and prints what it cost.
    private func bench(_ seconds: Double) {
        func cpu() -> Double {
            var usage = rusage()
            getrusage(RUSAGE_SELF, &usage)
            let user = Double(usage.ru_utime.tv_sec) + Double(usage.ru_utime.tv_usec) / 1e6
            let system = Double(usage.ru_stime.tv_sec) + Double(usage.ru_stime.tv_usec) / 1e6
            return user + system
        }
        // the first second is the launch: not counted
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
            let before = (self.controller.stats(), cpu())
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
                        format: "%@ zoom %d, %.0f s: %.1f%% of a core; %.1f steps/s, %.1f windows drawn/s; "
                            + "engine %.2f ms/s, drawing %.2f ms/s (%.0f us a window); peak memory %.0f MB",
                        self.controller.name, self.controller.zoom, seconds,
                        (after.1 - before.1) / seconds * 100, Double(steps) / seconds, Double(drawn) / seconds,
                        engine / seconds * 1000, draw / seconds * 1000,
                        drawn > 0 ? draw / Double(drawn) * 1e6 : 0, Double(usage.ru_maxrss) / 1_048_576))
                exit(0)
            }
        }
    }
}

let delegate = AppDelegate()
NSApplication.shared.delegate = delegate
NSApplication.shared.setActivationPolicy(.regular)
NSApplication.shared.run()
