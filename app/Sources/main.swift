// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import ImageIO
import UniformTypeIdentifiers

/// The Mac's shell around the app in the library: its menu bar, its panels and what the Mac
/// alone can do. What the app is (the command line, the menu's items, what each does, what a
/// start puts back) is the library's, the same on every system.
///
/// The command line is the library's (`core/src/app.rs`). The options for checking and
/// measuring are answered here:
///
/// ```
///        [--click x,y]         clicks there on the screen, in layout pixels from its top left
///                              corner, a second after the start; may be given more than once
///        [--quit-after <seconds>]  prints what it would keep for the next run, and quits as
///                              the menu's Quit does
///        [--dump <folder>]     writes every window as a PNG after two seconds, and quits;
///                              with --show-vis and --show-effects those windows too
///        [--bench <seconds>]   prints what that many seconds cost, and quits
///        [--then-skin <file>]  wears that skin a second after the start, as a drop does
///        [--hidden]            hides the app once it is up, as the Hide command does
/// ```
final class AppDelegate: NSObject, NSApplicationDelegate {
    private let controller = SkinController()
    private var nowPlaying: NowPlaying?
    private lazy var effects = Effects(app: controller.app)
    private lazy var visualizer: Visualizer = {
        let made = Visualizer(app: controller.app)
        made.onChange = { [weak self] in self?.controller.pump(force: true) }
        return made
    }()

    /// A run that only looks or measures: a skin that does not open ends it.
    private var looking: Bool { controller.option("dump") != nil || controller.option("bench") != nil }

    func applicationDidFinishLaunching(_ notification: Notification) {
        controller.onRequests = { [weak self] lines in self?.answer(lines) }
        controller.start()
        NSApp.mainMenu = makeMenu()
        nowPlaying = NowPlaying(controller: controller)
        controller.onStep = { [weak self] in self?.nowPlaying?.update() }
        NSApp.activate(ignoringOtherApps: true)

        let clicks = (controller.option("click") ?? "").split(separator: "\n")
        for (n, click) in clicks.enumerated() {
            let at = click.split(separator: ",").compactMap { Int32($0) }
            guard at.count == 2 else { continue }
            DispatchQueue.main.asyncAfter(deadline: .now() + 1 + Double(n) * 0.2) {
                self.controller.click(x: at[0], y: at[1])
            }
        }
        if let folder = controller.option("dump") {
            DispatchQueue.main.asyncAfter(deadline: .now() + 2) { self.dump(to: URL(fileURLWithPath: folder)) }
        }
        if let seconds = controller.option("bench").flatMap(Double.init) { bench(seconds) }
        if let skin = controller.option("then-skin") {
            DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
                self.controller.open([URL(fileURLWithPath: skin)])
            }
        }
        if controller.option("hidden") != nil {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { NSApp.hide(nil) }
        }
        if let seconds = controller.option("quit-after").flatMap(Double.init) {
            DispatchQueue.main.asyncAfter(deadline: .now() + seconds) {
                if let summary = andamp_app_summary(self.controller.app) {
                    print(String(cString: summary))
                    andamp_string_free(summary)
                }
                NSApp.terminate(nil)
            }
        }
    }

    /// Files Finder asks for: before the start the library keeps them for it.
    func application(_ application: NSApplication, open urls: [URL]) { controller.open(urls) }

    func applicationWillTerminate(_ notification: Notification) { andamp_app_save(controller.app) }

    func applicationDidHide(_ notification: Notification) { controller.sightChanged() }

    func applicationDidUnhide(_ notification: Notification) { controller.sightChanged() }

    func applicationDidResignActive(_ notification: Notification) { andamp_app_save(controller.app) }

    // MARK: what the library asks for

    /// Answers the library's requests, a line for each with tabs between its parts
    /// (`andamp_app_take_requests`).
    private func answer(_ lines: [String]) {
        for line in lines {
            let parts = line.split(separator: "\t", maxSplits: 3, omittingEmptySubsequences: false).map(String.init)
            let second = parts.count > 1 ? parts[1] : ""
            switch parts[0] {
            case "quit": NSApp.terminate(nil)
            case "hide": NSApp.hide(nil)
            case "about": NSApp.orderFrontStandardAboutPanel(nil)
            case "ask": ask(second)
            case "popup":
                let place = parts.count >= 4 ? Int(parts[2]).flatMap { x in Int(parts[3]).map { (x, $0) } } : nil
                popUp(menu: Int32(second) ?? ANDAMP_MENU_POPUP, at: place.map { controller.screenPoint($0.0, $0.1) })
            case "menu": NSApp.mainMenu = makeMenu()
            case "effects": effects.show()
            case "vis": if second == "1" { visualizer.show() } else { visualizer.close() }
            case "preset" where parts.count >= 4:
                visualizer.load(preset: parts[3], title: parts[2], blend: second == "1")
            case "error":
                if looking {
                    FileHandle.standardError.write(Data("andamp: \(second)\n".utf8))
                    exit(1)
                }
                let alert = NSAlert()
                alert.messageText = second
                alert.runModal()
            default: break
            }
        }
        controller.pump(force: true)
    }

    /// Lets the listener pick what the library asked for, and hands back the answer.
    private func ask(_ what: String) {
        func types(_ endings: [String]) -> [UTType] { endings.compactMap { UTType(filenameExtension: $0) } }
        let open = NSOpenPanel()
        switch what {
        case "open_songs", "add_songs":
            open.allowedContentTypes = [.audio]
            open.allowsMultipleSelection = true
            open.canChooseDirectories = true
        case "add_folders":
            open.canChooseDirectories = true
            open.canChooseFiles = false
            open.allowsMultipleSelection = true
        case "load_list": open.allowedContentTypes = types(["m3u", "m3u8", "pls"])
        case "skin": open.allowedContentTypes = types(["wsz", "wal", "zip"])
        case "presets":
            open.canChooseDirectories = true
            open.canChooseFiles = false
            open.message = "A folder of Milkdrop presets (.milk files)"
        case "save_list":
            let save = NSSavePanel()
            save.allowedContentTypes = types(["m3u8"])
            save.nameFieldStringValue = "Playlist.m3u8"
            if save.runModal() == .OK, let file = save.url { controller.picked(what, [file.path]) }
            return
        case "url":
            let alert = NSAlert()
            alert.messageText = "Add a radio stream"
            alert.informativeText = "The address of the stream, starting with http:// or https://"
            let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 360, height: 24))
            field.placeholderString = "https://"
            alert.accessoryView = field
            alert.addButton(withTitle: "Add")
            alert.addButton(withTitle: "Cancel")
            alert.window.initialFirstResponder = field
            if alert.runModal() == .alertFirstButtonReturn { controller.picked(what, [field.stringValue]) }
            return
        default: return
        }
        if open.runModal() == .OK, !open.urls.isEmpty { controller.picked(what, open.urls.map(\.path)) }
    }

    // MARK: menus

    /// The menu bar, from the library's lines: its top level is the bar's menus.
    private func makeMenu() -> NSMenu { menu(ANDAMP_MENU_BAR) }

    private func menu(_ number: Int32) -> NSMenu {
        guard let text = andamp_app_menu(controller.app, number) else { return NSMenu() }
        defer { andamp_string_free(text) }
        return NSMenu.from(lines: String(cString: text), target: self, action: #selector(pick(_:)))
    }

    /// One of the app's menus as a popup, at `point` on the screen or at the mouse.
    private func popUp(menu number: Int32, at point: NSPoint?) {
        menu(number).popUp(positioning: nil, at: point ?? NSEvent.mouseLocation, in: nil)
    }

    /// A pick from one of the app's menus: the item carries the command's number.
    @objc private func pick(_ item: NSMenuItem) { controller.command(Int32(item.tag)) }

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
        if controller.option("show-vis") != nil,
            let png = visualizer.picture(after: 60)?.representation(using: .png, properties: [:])
        {
            try? png.write(to: folder.appendingPathComponent("visualizer.png"))
        }
        if controller.option("show-effects") != nil,
            let png = effects.picture()?.representation(using: .png, properties: [:])
        {
            try? png.write(to: folder.appendingPathComponent("effects.png"))
        }
        try? lines.joined(separator: "\n").appending("\n")
            .write(to: folder.appendingPathComponent("windows.txt"), atomically: true, encoding: .utf8)
        exit(0)
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
                            + "%d songs, %llu underruns; first window %.0f ms after the start, "
                            + "of which the skin, the playlist and the first drawing took %.0f ms",
                        self.controller.name, self.controller.zoom, seconds,
                        (after.1.user + after.1.system - before.1.user - before.1.system) / seconds * 100,
                        (after.1.user - before.1.user) / seconds * 100,
                        (after.1.system - before.1.system) / seconds * 100,
                        Double(steps) / seconds, Double(drawn) / seconds,
                        engine / seconds * 1000, draw / seconds * 1000,
                        drawn > 0 ? draw / Double(drawn) * 1e6 : 0, Double(usage.ru_maxrss) / 1_048_576,
                        Int(andamp_player_count(self.controller.player)),
                        andamp_player_underruns(self.controller.player), self.startup * 1000,
                        self.controller.startTook * 1000))
                exit(0)
            }
        }
    }
}

let delegate = AppDelegate()
NSApplication.shared.delegate = delegate
NSApplication.shared.setActivationPolicy(.regular)
NSApplication.shared.run()
