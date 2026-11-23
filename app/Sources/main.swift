// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import ImageIO
import UniformTypeIdentifiers

/// The app: its menus, the skin it shows, and what the command line asks for.
///
/// ```
/// Andamp [skin.wsz|skin.wal] [song ...] [--modern] [--zoom n] [--volume 0..255]
///        [--base skin.wsz]     a classic skin laid under classic skins, in place of AndAmp Dark
///        [--wasabi base.zip]   a zip of a wasabi folder laid under modern skins, in place of
///                              Andamp's own element set
///        [--click x,y]         clicks there on the screen, in layout pixels from its top left
///                              corner, a second after the start; may be given more than once
///        [--quit-after <seconds>]  prints what it would keep for the next run, and quits as
///                              the menu's Quit does
///        [--dump <folder>]     writes every window as a PNG after two seconds, and quits
///        [--bench <seconds>]   prints what that many seconds cost, and quits
///
/// Songs named on the command line play at once.
/// ```
final class AppDelegate: NSObject, NSApplicationDelegate {
    private let controller: SkinController
    private var options = Options()
    private let settings: Settings
    private var nowPlaying: NowPlaying?
    /// The skin file worn.
    private var worn: URL?

    struct Options {
        var skin: URL?
        var songs: [URL] = []
        var volume: Int?
        var modern = false
        var zoom: Int?
        var base: URL?
        var wasabi: URL?
        var dump: URL?
        var bench: Double?
        var quitAfter: Double?
        var clicks: [(Int32, Int32)] = []

        init() {
            var args = CommandLine.arguments.dropFirst()
            while let arg = args.popFirst() {
                switch arg {
                case "--modern": modern = true
                case "--zoom": zoom = args.popFirst().flatMap { Int($0) }
                case "--volume": volume = args.popFirst().flatMap { Int($0) }
                case "--dump": dump = args.popFirst().map { URL(fileURLWithPath: $0) }
                case "--base": base = args.popFirst().map { URL(fileURLWithPath: $0) }
                case "--wasabi": wasabi = args.popFirst().map { URL(fileURLWithPath: $0) }
                case "--click":
                    let at = (args.popFirst() ?? "").split(separator: ",").compactMap { Int32($0) }
                    if at.count == 2 { clicks.append((at[0], at[1])) }
                case "--bench": bench = args.popFirst().flatMap { Double($0) }
                case "--quit-after": quitAfter = args.popFirst().flatMap { Double($0) }
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
        // a run that only looks or measures starts from nothing and leaves nothing
        settings = Settings(kept: options.dump == nil && options.bench == nil && options.clicks.isEmpty)
        controller = SkinController(zoom: options.zoom ?? settings.zoom)
        super.init()
    }

    /// The file name endings of a skin.
    static let skinTypes = ["wsz", "wal", "zip"]

    /// A skin that comes with the app, by its file name.
    private func builtin(_ name: String) -> URL? {
        Bundle.main.url(forResource: "skins", withExtension: nil)?.appendingPathComponent(name)
    }

    /// The skin shown when none is asked for, which also lies under every classic skin.
    private static let defaultSkin = "AndAmp Dark.wsz"
    private static let modernSkin = "walkit-modern.wal"

    /// What lies under every skin: AndAmp Dark, Andamp's element set and the stand-in fonts,
    /// unless the command line names others.
    private var under: SkinController.Under {
        SkinController.Under(
            base: options.base ?? builtin(Self.defaultSkin),
            wasabi: options.wasabi,
            wasabiFolder: Bundle.main.url(forResource: "wasabi", withExtension: nil),
            fonts: Bundle.main.url(forResource: "fonts", withExtension: nil))
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.mainMenu = makeMenu()
        controller.onOpenFiles = { [weak self] in self?.openFiles() }
        controller.onMenu = { [weak self] at in self?.popUpMenu(at: at) }
        controller.onDrop = { [weak self] files in self?.dropped(files) }
        controller.onPlaylist = { [weak self] choice in self?.playlist(choice) }
        if let volume = options.volume ?? settings.volume { controller.setVolume(volume) }
        if settings.onTop { controller.setOnTop(true) }
        let builtIn = builtin(options.modern ? Self.modernSkin : Self.defaultSkin)
        if let skin = options.skin ?? (options.modern ? nil : settings.skin) ?? builtIn { open(skin) }
        if !options.songs.isEmpty {
            // songs, folders of songs and playlist files, as if dropped on a window
            dropped(options.songs)
        } else if !opened.isEmpty {
            // files Finder opened the app with
            dropped(opened)
        } else {
            // the playlist as it was, at the song it was at, not playing
            controller.add(files: settings.songs)
            andamp_player_select(controller.player, Int32(settings.song))
            controller.pump(force: true)
        }
        launched = true
        nowPlaying = NowPlaying(controller: controller)
        controller.onStep = { [weak self] in self?.nowPlaying?.update() }
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
        if let seconds = options.quitAfter {
            DispatchQueue.main.asyncAfter(deadline: .now() + seconds) {
                let corner = self.controller.places.values.map { "\(Int($0.x)),\(Int($0.y))" }.sorted()
                print(
                    "skin \(self.controller.name) zoom \(self.controller.zoom) on top \(self.controller.onTop)"
                        + " volume \(andamp_player_volume(self.controller.player))"
                        + " songs \(self.controller.songs.count) at \(andamp_player_index(self.controller.player))"
                        + " status \(andamp_player_status(self.controller.player)) windows \(corner)")
                NSApp.terminate(nil)
            }
        }
    }

    /// Files Finder asked for before the app was up.
    private var opened: [URL] = []
    private var launched = false

    func application(_ application: NSApplication, open urls: [URL]) {
        if launched { dropped(urls) } else { opened += urls }
    }

    func applicationWillTerminate(_ notification: Notification) { save() }

    func applicationDidResignActive(_ notification: Notification) { save() }

    /// Keeps what the next run starts from.
    private func save() {
        settings.skin = worn
        settings.zoom = controller.zoom
        settings.onTop = controller.onTop
        settings.volume = Int(andamp_player_volume(controller.player))
        settings.songs = controller.songs
        settings.song = Int(andamp_player_index(controller.player))
        settings.setPlaces(controller.places, skin: controller.name)
    }

    private func open(_ skin: URL) {
        do {
            // the places of the skin worn are kept before it goes
            if worn != nil { settings.setPlaces(controller.places, skin: controller.name) }
            try controller.open(
                file: skin, under: under, places: settings.places(skin: skin.lastPathComponent))
            worn = skin
        } catch {
            if options.dump != nil || options.bench != nil {
                FileHandle.standardError.write(Data("andamp: \(error.localizedDescription)\n".utf8))
                exit(1)
            }
            NSAlert(error: error).runModal()
        }
    }

    /// The file name endings of a playlist file.
    static let listTypes = ["m3u", "m3u8", "pls"]

    /// Files dropped on a window or opened from Finder: a skin is worn, songs and the songs
    /// of folders and playlist files are played.
    private func dropped(_ files: [URL]) {
        let kind = { (file: URL) in file.pathExtension.lowercased() }
        if let skin = files.first(where: { Self.skinTypes.contains(kind($0)) }) { open(skin) }
        let rest = files.filter { !Self.skinTypes.contains(kind($0)) }
        guard !rest.isEmpty else { return }
        andamp_player_clear(controller.player)
        for file in rest {
            if Self.listTypes.contains(kind(file)) {
                controller.add(list: file)
            } else {
                controller.add(files: songs(in: file))
            }
        }
        if andamp_player_count(controller.player) > 0 { andamp_player_play_index(controller.player, 0) }
        controller.pump(force: true)
    }

    /// `file` itself, or when it is a folder the files in it and in the folders under it, by name.
    private func songs(in file: URL) -> [URL] {
        var isFolder: ObjCBool = false
        guard FileManager.default.fileExists(atPath: file.path, isDirectory: &isFolder), isFolder.boolValue
        else { return [file] }
        let found = FileManager.default.enumerator(
            at: file, includingPropertiesForKeys: [.isRegularFileKey], options: [.skipsHiddenFiles])
        let files = (found?.allObjects as? [URL] ?? []).filter {
            (try? $0.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true
                && UTType(filenameExtension: $0.pathExtension)?.conforms(to: .audio) == true
        }
        return files.sorted { $0.path.localizedStandardCompare($1.path) == .orderedAscending }
    }

    /// What the playlist window's buttons ask of the app.
    private func playlist(_ choice: String) {
        switch choice {
        case "add_file": addFiles()
        case "add_dir":
            let panel = NSOpenPanel()
            panel.canChooseDirectories = true
            panel.canChooseFiles = false
            panel.allowsMultipleSelection = true
            if panel.runModal() == .OK { controller.add(files: panel.urls.flatMap(songs(in:))) }
        case "list_load":
            let panel = NSOpenPanel()
            panel.allowedContentTypes = Self.listTypes.compactMap { UTType(filenameExtension: $0) }
            if panel.runModal() == .OK, let file = panel.url {
                andamp_player_clear(controller.player)
                controller.add(list: file)
            }
        case "list_save":
            let panel = NSSavePanel()
            panel.allowedContentTypes = [UTType(filenameExtension: "m3u8")].compactMap { $0 }
            panel.nameFieldStringValue = "Playlist.m3u8"
            if panel.runModal() == .OK, let file = panel.url { controller.save(list: file) }
        default: break
        }
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
        let skins = NSMenuItem(title: "Skins", action: nil, keyEquivalent: "")
        skins.submenu = NSMenu(title: "Skins")
        skinItems().forEach { skins.submenu?.addItem($0) }
        menu.addItem(skins)
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
        add("Skins", skinItems())
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
        panel.canChooseDirectories = true
        return panel.runModal() == .OK && !panel.urls.isEmpty ? panel.urls.flatMap(songs(in:)) : nil
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

    /// The skins that come with the app, as menu items.
    private func skinItems() -> [NSMenuItem] {
        let folder = Bundle.main.url(forResource: "skins", withExtension: nil)
        let files = (folder.flatMap { try? FileManager.default.contentsOfDirectory(atPath: $0.path) } ?? [])
            .filter { Self.skinTypes.contains(($0 as NSString).pathExtension.lowercased()) }
            .sorted()
        return files.map { file in
            let item = NSMenuItem(
                title: (file as NSString).deletingPathExtension, action: #selector(showBuiltin(_:)),
                keyEquivalent: "")
            item.target = self
            item.representedObject = file
            item.state = file == controller.name ? .on : .off
            return item
        }
    }

    @objc private func showBuiltin(_ item: NSMenuItem) {
        (item.representedObject as? String).flatMap(builtin).map(open)
    }
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
