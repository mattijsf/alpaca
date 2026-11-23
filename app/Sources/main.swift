// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import ImageIO
import UniformTypeIdentifiers

/// The app: its menus, the skin it shows, and what the command line asks for.
///
/// ```
/// Andamp [skin.wsz|skin.wal] [song ...] [--modern] [--zoom n] [--volume 0..255]
///        [--click x,y]         clicks there on the screen, in layout pixels from its top left
///                              corner, a second after the start; may be given more than once
///        [--dump <folder>]     writes every window as a PNG after two seconds, and quits
///        [--bench <seconds>]   prints what that many seconds cost, and quits
///
/// Songs named on the command line play at once.
/// ```
final class AppDelegate: NSObject, NSApplicationDelegate {
    private let controller: SkinController
    private var options = Options()

    struct Options {
        var skin: URL?
        var songs: [URL] = []
        var volume: Int?
        var modern = false
        var zoom = 1
        var dump: URL?
        var bench: Double?
        var clicks: [(Int32, Int32)] = []

        init() {
            var args = CommandLine.arguments.dropFirst()
            while let arg = args.popFirst() {
                switch arg {
                case "--modern": modern = true
                case "--zoom": zoom = args.popFirst().flatMap { Int($0) } ?? 1
                case "--volume": volume = args.popFirst().flatMap { Int($0) }
                case "--dump": dump = args.popFirst().map { URL(fileURLWithPath: $0) }
                case "--click":
                    let at = (args.popFirst() ?? "").split(separator: ",").compactMap { Int32($0) }
                    if at.count == 2 { clicks.append((at[0], at[1])) }
                case "--bench": bench = args.popFirst().flatMap { Double($0) }
                // the arguments Xcode and macOS add, such as -NSDocumentRevisionsDebugMode YES
                case _ where arg.hasPrefix("-") && !arg.hasPrefix("--"): args = args.dropFirst()
                default:
                    let file = URL(fileURLWithPath: arg)
                    if AppDelegate.skinTypes.contains(file.pathExtension.lowercased()) {
                        skin = file
                    } else {
                        songs.append(file)
                    }
                }
            }
        }
    }

    override init() {
        controller = SkinController(zoom: options.zoom)
        super.init()
    }

    /// The file name endings of a skin.
    static let skinTypes = ["wsz", "wal", "zip"]

    private var fonts: URL? { Bundle.main.url(forResource: "fonts", withExtension: nil) }

    private func builtin(modern: Bool) -> URL? {
        Bundle.main.url(forResource: "skins", withExtension: nil)?
            .appendingPathComponent(modern ? "walkit-modern.wal" : "walkit-classic.wsz")
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.mainMenu = makeMenu()
        controller.onOpenFiles = { [weak self] in self?.openFiles() }
        controller.onMenu = { [weak self] at in self?.popUpMenu(at: at) }
        controller.onDrop = { [weak self] files in self?.dropped(files) }
        if let volume = options.volume { controller.setVolume(volume) }
        if let skin = options.skin ?? builtin(modern: options.modern) { open(skin) }
        if !options.songs.isEmpty { controller.play(files: options.songs) }
        NSApp.activate(ignoringOtherApps: true)
        for (n, at) in options.clicks.enumerated() {
            DispatchQueue.main.asyncAfter(deadline: .now() + 1 + Double(n) * 0.2) {
                self.controller.click(x: at.0, y: at.1)
            }
        }
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

    /// Files dropped on a window: a skin is worn, songs are played.
    private func dropped(_ files: [URL]) {
        if let skin = files.first(where: { Self.skinTypes.contains($0.pathExtension.lowercased()) }) {
            open(skin)
        }
        let songs = files.filter { !Self.skinTypes.contains($0.pathExtension.lowercased()) }
        if !songs.isEmpty { controller.play(files: songs) }
    }

    /// The app's menu as a popup, for the skin's menu button: what the menu bar has, in one list.
    private func popUpMenu(at point: NSPoint) {
        let menu = NSMenu()
        func item(_ title: String, _ action: Selector, on: Bool = false) {
            let item = NSMenuItem(title: title, action: action, keyEquivalent: "")
            item.target = action == #selector(NSApplication.terminate(_:)) ? NSApp : self
            item.state = on ? .on : .off
            menu.addItem(item)
        }
        item("Open Files…", #selector(openFiles))
        item("Add Files…", #selector(addFiles))
        menu.addItem(.separator())
        item("Open Skin…", #selector(openSkin))
        item("Classic Skin", #selector(showClassic))
        item("Modern Skin", #selector(showModern))
        menu.addItem(.separator())
        item("Double Size", #selector(toggleDoubleSize), on: controller.zoom > 1)
        item("Always on Top", #selector(toggleOnTop), on: controller.onTop)
        menu.addItem(.separator())
        item("Quit Andamp", #selector(NSApplication.terminate(_:)))
        menu.popUp(positioning: nil, at: point, in: nil)
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
        let openSkinItem = item("Open Skin…", #selector(openSkin), "o")
        openSkinItem.keyEquivalentModifierMask = [.command, .shift]
        add("File", [
            item("Open Files…", #selector(openFiles), "o"),
            item("Add Files…", #selector(addFiles), "l"),
            .separator(),
            openSkinItem,
        ])
        add("Play", [
            item("Previous", #selector(previous), "z"),
            item("Play", #selector(play), "x"),
            item("Pause", #selector(pause), "c"),
            item("Stop", #selector(stop), "v"),
            item("Next", #selector(next), "b"),
        ])
        add("Skins", [
            item("Classic", #selector(showClassic)),
            item("Modern", #selector(showModern)),
        ])
        add("View", [
            item("Double Size", #selector(toggleDoubleSize), "d"),
            item("Always on Top", #selector(toggleOnTop), "a"),
        ])
        return main
    }

    /// Lets the listener pick songs; nil when they picked none.
    private func pickSongs() -> [URL]? {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.audio]
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        return panel.runModal() == .OK && !panel.urls.isEmpty ? panel.urls : nil
    }

    @objc private func openFiles() { pickSongs().map(controller.play(files:)) }
    @objc private func addFiles() { pickSongs().map { _ = controller.add(files: $0) } }
    // Winamp's keys, with the command key: Z X C V B
    @objc private func previous() { controller.command(ANDAMP_PREVIOUS) }
    @objc private func play() { controller.command(ANDAMP_PLAY) }
    @objc private func pause() { controller.command(ANDAMP_PAUSE) }
    @objc private func stop() { controller.command(ANDAMP_STOP) }
    @objc private func next() { controller.command(ANDAMP_NEXT) }

    @objc private func openSkin() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = Self.skinTypes.compactMap { UTType(filenameExtension: $0) }
        panel.allowsMultipleSelection = false
        if panel.runModal() == .OK, let file = panel.url { open(file) }
    }

    @objc private func showClassic() { builtin(modern: false).map(open) }
    @objc private func showModern() { builtin(modern: true).map(open) }
    @objc private func toggleDoubleSize() { controller.setZoom(controller.zoom == 1 ? 2 : 1) }
    @objc private func toggleOnTop() { controller.setOnTop(!controller.onTop) }

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
                            + "%d songs, %llu underruns",
                        self.controller.name, self.controller.zoom, seconds,
                        (after.1.user + after.1.system - before.1.user - before.1.system) / seconds * 100,
                        (after.1.user - before.1.user) / seconds * 100,
                        (after.1.system - before.1.system) / seconds * 100,
                        Double(steps) / seconds, Double(drawn) / seconds,
                        engine / seconds * 1000, draw / seconds * 1000,
                        drawn > 0 ? draw / Double(drawn) * 1e6 : 0, Double(usage.ru_maxrss) / 1_048_576,
                        Int(andamp_player_count(self.controller.player)),
                        andamp_player_underruns(self.controller.player)))
                exit(0)
            }
        }
    }
}

let delegate = AppDelegate()
NSApplication.shared.delegate = delegate
NSApplication.shared.setActivationPolicy(.regular)
NSApplication.shared.run()
