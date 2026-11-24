// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit

/// The app's windows on the desktop: one for each window of the skin worn.
///
/// The library has the app (`andamp_app_`): its player, the skin, the menu, what each command
/// does. The engine has the screen as its viewport, in layout pixels: a layout pixel is `zoom`
/// points. After every step the controller puts one `SkinWindow` where the engine has each of
/// its windows and hands over the pixels of those that changed. It sleeps until the engine
/// says the skin next changes by itself. What the library asks that only the Mac can do, it
/// hands to `onRequests`.
final class SkinController {
    /// The app in the library.
    let app: OpaquePointer
    /// The app's player, which every skin shows and drives.
    var player: OpaquePointer { andamp_app_player(app) }
    /// The skin worn; the library's, and another one after a request `skin`.
    private var skin: OpaquePointer? { andamp_app_skin(app) }
    private var windows: [UInt32: SkinWindow] = [:]
    /// The engine's windows from the bottom up, as last shown.
    private var order: [UInt32] = []
    /// The part of the screen the windows move on, in points.
    private let area: NSRect
    private var timer: Timer?
    /// What watches the windows going out of sight and coming back.
    private var watchers: [NSObjectProtocol] = []
    private var touch: Int32 = 0
    private var pressed = false
    /// Set while one of the skin's menus is open: the engine waits for the answer inside a
    /// mouse call, and must not be called again until it has it.
    private var inMenu = false
    /// Set when every window is to be shown again whatever the engine says changed.
    private var stale = false
    /// Called with what the library asks of the app, a line for each, after the turn of the
    /// run loop they came in: a panel must not open inside a mouse event.
    var onRequests: (([String]) -> Void)?
    /// Called after every step of the skin.
    var onStep: (() -> Void)?

    /// Points per layout pixel.
    var zoom: Int { Int(andamp_app_zoom(app)) }
    /// Whether the windows stay above other apps' windows.
    private var onTop: Bool { andamp_app_on_top(app) }
    /// How much of the windows shows over what is behind them: 1 for all of it.
    private var opacity: CGFloat { CGFloat(andamp_app_opacity(app)) / 100 }

    /// The file name of the skin worn.
    var name: String {
        guard let name = andamp_app_skin_name(app) else { return "" }
        defer { andamp_string_free(name) }
        return String(cString: name)
    }

    /// Makes the app from the command line, with what is in the bundle's resources.
    init() {
        area = NSScreen.main?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1440, height: 900)
        let arguments = CommandLine.arguments.dropFirst().map { UnsafePointer(strdup($0)) }
        defer { for argument in arguments { free(UnsafeMutablePointer(mutating: argument)) } }
        guard
            let made = andamp_app_new(
                arguments, Int32(arguments.count), Bundle.main.resourcePath ?? "", Int32(area.width),
                Int32(area.height))
        else { fatalError("the library did not start") }
        app = made
        andamp_app_set_menu_handler(
            app,
            { context, menu in
                guard let context, let menu else { return 0 }
                let controller = Unmanaged<SkinController>.fromOpaque(context).takeUnretainedValue()
                return controller.showSkinMenu(String(cString: menu))
            }, Unmanaged.passUnretained(self).toOpaque())
    }

    deinit {
        dropWindows()
        andamp_app_free(app)
    }

    /// Wears the first skin, shows it, and then puts the playlist back.
    func start() {
        let began = ProcessInfo.processInfo.systemUptime
        andamp_app_start(app)
        pump(force: true)
        // the library reads the songs at its second step: after the skin is on the screen,
        // in sight or not
        DispatchQueue.main.async { [weak self] in self?.pump(force: true) }
        startTook = ProcessInfo.processInfo.systemUptime - began
    }

    /// What the command line gave for an option the shell answers: nil when it was not given.
    func option(_ name: String) -> String? {
        guard let value = andamp_app_option(app, name) else { return nil }
        defer { andamp_string_free(value) }
        return String(cString: value)
    }

    /// A place in points from the top left corner of the area, as a place on the screen.
    func screenPoint(_ x: Int, _ y: Int) -> NSPoint {
        NSPoint(x: area.minX + CGFloat(x), y: area.maxY - CGFloat(y))
    }

    /// Closes the windows; the skin they showed is gone.
    private func dropWindows() {
        timer?.invalidate()
        timer = nil
        watchers.forEach(NotificationCenter.default.removeObserver)
        watchers.removeAll()
        for window in windows.values { window.close() }
        windows.removeAll()
        order.removeAll()
    }

    /// Runs the engine and shows what changed, then sleeps until the skin next changes.
    func pump(force: Bool = false) {
        guard !inMenu else { return }
        let changed = andamp_app_step(app)
        answer()
        if changed != 0 || force || stale { apply() }
        stale = false
        schedule()
        onStep?()
    }

    /// Takes what the library asked for: what concerns the windows is done here, the rest is
    /// handed on.
    private func answer() {
        guard let text = andamp_app_take_requests(app) else { return }
        let lines = String(cString: text).split(separator: "\n").map(String.init)
        andamp_string_free(text)
        var rest: [String] = []
        for line in lines {
            switch line {
            case "skin":
                dropWindows()
                stale = true
            case "look":
                for window in windows.values {
                    window.level = onTop ? .floating : .normal
                    window.alphaValue = opacity
                    window.shownGeneration = 0
                }
                order.removeAll()
                stale = true
            default: rest.append(line)
            }
        }
        if !rest.isEmpty { DispatchQueue.main.async { [weak self] in self?.onRequests?(rest) } }
    }

    /// How long the start took, in seconds: the first skin, the playlist and the first drawing.
    private(set) var startTook: TimeInterval = 0

    /// When the first window showed its pixels, as seconds since the Mac started; 0 before.
    private(set) var firstShown: TimeInterval = 0

    /// Puts the app's windows where the engine's are and shows the pixels that changed.
    private func apply() {
        guard let skin else { return }
        defer { if firstShown == 0, !windows.isEmpty { firstShown = ProcessInfo.processInfo.systemUptime } }
        let count = andamp_skin_window_count(skin)
        var seen: [UInt32] = []
        var info = AndampWindow()
        let z = CGFloat(zoom)
        for index in 0..<count where andamp_skin_window(skin, index, &info) {
            seen.append(info.id)
            let window =
                windows[info.id]
                ?? {
                    let made = SkinWindow(skinId: info.id, controller: self)
                    made.level = onTop ? .floating : .normal
                    made.alphaValue = opacity
                    windows[info.id] = made
                    watchers.append(
                        NotificationCenter.default.addObserver(
                            forName: NSWindow.didChangeOcclusionStateNotification, object: made, queue: .main
                        ) { [weak self] _ in self?.sightChanged() })
                    return made
                }()
            // the engine counts from the top left of the area, the screen from its bottom left
            let frame = NSRect(
                x: area.minX + CGFloat(info.x) * z,
                y: area.maxY - CGFloat(info.y + info.h) * z,
                width: CGFloat(info.w) * z, height: CGFloat(info.h) * z)
            if window.frame != frame { window.setFrame(frame, display: false) }
            if window.shownGeneration != info.generation, let pixels = info.pixels {
                // the drawing right after the one shown differs from it only where it says
                let next = window.shownGeneration != 0 && window.shownGeneration + 1 == info.generation
                let changed = CGRect(
                    x: Int(info.changed_x), y: Int(info.changed_y), width: Int(info.changed_w),
                    height: Int(info.changed_h))
                window.show(
                    pixels: pixels, width: Int(info.w), height: Int(info.h), changed: next ? changed : nil)
                window.shownGeneration = info.generation
            }
        }
        for (id, window) in windows where !seen.contains(id) {
            window.close()
            windows[id] = nil
        }
        // bottom to top, as the engine has them
        if seen != order {
            order = seen
            for id in seen { windows[id]?.orderFront(nil) }
            if let top = seen.last, NSApp.isActive { windows[top]?.makeKey() }
        }
    }

    /// Whether any window of the skin can be seen: not when the app is hidden or every window
    /// lies under other windows.
    private var inSight: Bool {
        !NSApp.isHidden && windows.values.contains { $0.occlusionState.contains(.visible) }
    }

    /// A window came into sight or went out of it.
    func sightChanged() {
        if inSight { pump(force: true) } else { schedule() }
    }

    /// Wakes the controller when the skin next changes by itself; not at all when it will not.
    private func schedule() {
        timer?.invalidate()
        timer = nil
        guard skin != nil else { return }
        guard inSight || windows.isEmpty else {
            // nobody sees the skin: it is not run or drawn. While a song plays the player is
            // asked twice a second whether the song is over, so the playlist goes on.
            guard andamp_player_status(player) == 1 else { return }
            let next = Timer(timeInterval: 0.5, repeats: false) { [weak self] _ in
                guard let self else { return }
                andamp_player_poll(self.player)
                self.onStep?()
                self.schedule()
            }
            next.tolerance = 0.2
            RunLoop.main.add(next, forMode: .common)
            timer = next
            return
        }
        let due = andamp_app_next_due_ms(app)
        guard due >= 0 else { return }
        // "now" means the next frame of the screen
        let wait = max(Double(due) / 1000, 1.0 / 120)
        let next = Timer(timeInterval: wait, repeats: false) { [weak self] _ in self?.pump() }
        next.tolerance = 0.002
        // a timer in the common modes also fires while the mouse is held on a slider
        RunLoop.main.add(next, forMode: .common)
        timer = next
    }

    /// The mouse wheel or the trackpad: the library scrolls the playlist under it or turns
    /// the volume.
    func scrollWheel(_ event: NSEvent) {
        guard event.scrollingDeltaY != 0 else { return }
        let (x, y) = point()
        andamp_app_wheel(app, x, y, Float(event.scrollingDeltaY), event.hasPreciseScrollingDeltas)
        pump(force: true)
    }

    // MARK: the skin's menus

    /// Shows a popup menu of the skin at the mouse and gives the id picked, 0 for none.
    /// `text` is the menu as the library spells it out, a line for each item.
    private func showSkinMenu(_ text: String) -> Int32 {
        let pick = MenuPick()
        let menu = NSMenu.from(lines: text, target: pick, action: #selector(MenuPick.pick(_:)))
        guard menu.numberOfItems > 0 else { return 0 }
        // the engine waits inside a mouse call: nothing may call it until the menu is gone
        inMenu = true
        menu.popUp(positioning: nil, at: NSEvent.mouseLocation, in: nil)
        inMenu = false
        return Int32(pick.picked)
    }

    // MARK: the app

    /// A song as the library takes it: a file by its path, a radio stream by its address.
    static func location(_ song: URL) -> String { song.isFileURL ? song.path : song.absoluteString }

    /// Files dropped on a window or opened from Finder: the library wears a skin and plays
    /// the rest.
    func open(_ files: [URL]) {
        andamp_app_open(app, files.map(Self.location).joined(separator: "\n"))
        pump(force: true)
    }

    /// Does one of the app's commands, by its number.
    func command(_ id: Int32) {
        andamp_app_command(app, id)
        pump(force: true)
    }

    /// What the listener picked when the library asked for `what`.
    func picked(_ what: String, _ answer: [String]) {
        andamp_app_picked(app, what, answer.joined(separator: "\n"))
        pump(force: true)
    }

    /// Moves the song to `ms` milliseconds.
    func seek(toMs ms: Int32) {
        andamp_player_seek_to(player, ms)
        pump(force: true)
    }

    // MARK: the mouse and the keys

    /// The mouse on the viewport, in layout pixels.
    private func point() -> (Int32, Int32) {
        let p = NSEvent.mouseLocation
        let z = CGFloat(zoom)
        return (Int32(((p.x - area.minX) / z).rounded(.down)), Int32(((area.maxY - p.y) / z).rounded(.down)))
    }

    func mouseDown(_ event: NSEvent) {
        guard let skin, !inMenu else { return }
        let (x, y) = point()
        touch += 1
        pressed = true
        andamp_skin_pointer_down(skin, x, y, touch)
        if event.clickCount == 2 { andamp_skin_double_click(skin, x, y, touch) }
        pump()
    }

    func mouseDragged(_ event: NSEvent) {
        guard let skin, pressed, !inMenu else { return }
        let (x, y) = point()
        andamp_skin_pointer_move(skin, x, y)
        pump()
    }

    func mouseUp(_ event: NSEvent) {
        guard let skin, pressed, !inMenu else { return }
        pressed = false
        let (x, y) = point()
        andamp_skin_pointer_up(skin, x, y)
        pump()
    }

    func rightMouseDown(_ event: NSEvent) {
        guard let skin, !inMenu else { return }
        let (x, y) = point()
        touch += 1
        andamp_skin_pointer_down(skin, x, y, touch)
        andamp_skin_right_click(skin, touch)
        andamp_skin_pointer_up(skin, x, y)
        pump()
    }

    /// A key with no modifier, pressed in the skin's window `window`: Winamp's keys, and in
    /// the playlist's window the arrows, delete and return. The library knows what each does
    /// where. False for a key that is none of them.
    func keyDown(_ event: NSEvent, in window: UInt32) -> Bool {
        guard event.modifierFlags.intersection([.command, .control, .option]).isEmpty else { return false }
        // the keys with no letter, by the key itself
        let named: [UInt16: String] = [
            126: "up", 125: "down", 123: "left", 124: "right", 49: "space", 51: "delete", 117: "delete",
            36: "return", 76: "return", 116: "pageup", 121: "pagedown", 115: "home", 119: "end",
        ]
        guard let key = named[event.keyCode] ?? event.charactersIgnoringModifiers?.lowercased(),
            andamp_app_key(app, window, key)
        else { return false }
        pump(force: true)
        return true
    }

    // MARK: for --click, --dump and --bench

    /// A click at `x`, `y` on the viewport, in layout pixels, as if the mouse made it.
    func click(x: Int32, y: Int32) {
        guard let skin else { return }
        touch += 1
        andamp_skin_pointer_down(skin, x, y, touch)
        andamp_skin_pointer_up(skin, x, y)
        pump()
    }

    /// Every window bottom to top: its id, its frame on the screen and its pixels as a picture.
    func pictures() -> [(id: UInt32, frame: NSRect, image: CGImage)] {
        order.compactMap { id in
            guard let window = windows[id], let image = window.picture() else { return nil }
            return (id, window.frame, image)
        }
    }

    /// Steps in which something changed, windows drawn again, and the seconds the engine and
    /// the drawing took, so far.
    func stats() -> (steps: UInt64, windowsDrawn: UInt64, engine: Double, draw: Double) {
        var steps: UInt64 = 0
        var drawn: UInt64 = 0
        var engine: UInt64 = 0
        var draw: UInt64 = 0
        if let skin { andamp_skin_stats(skin, &steps, &drawn, &engine, &draw) }
        return (steps, drawn, Double(engine) / 1e9, Double(draw) / 1e9)
    }
}

/// Remembers which item of a skin's menu was picked.
private final class MenuPick: NSObject {
    var picked = 0
    @objc func pick(_ item: NSMenuItem) { picked = item.tag }
}

extension NSMenu {
    /// A menu from the lines the library spells one out in: `C`, the number, 1 when checked, 1
    /// when disabled, the text and the key, with tabs between, for a command; `S` for a
    /// separator; `>` and the text where a submenu starts, `<` where it ends. An item carries
    /// its number as its tag and sends `action` to `target`. A key is a letter with the
    /// command key, and Shift when it says `shift+`.
    static func from(lines text: String, target: AnyObject, action: Selector) -> NSMenu {
        var stack = [NSMenu()]
        for line in text.split(separator: "\n") {
            let parts = line.split(separator: "\t", omittingEmptySubsequences: false).map(String.init)
            guard let menu = stack.last else { break }
            switch parts.first {
            case "C" where parts.count >= 5:
                let item = NSMenuItem(title: parts[4], action: action, keyEquivalent: "")
                item.target = target
                item.tag = Int(parts[1]) ?? 0
                item.state = parts[2] == "1" ? .on : .off
                if parts[3] == "1" {
                    item.action = nil
                    item.target = nil
                }
                if parts.count >= 6, let key = parts[5].split(separator: "+").last {
                    item.keyEquivalent = String(key)
                    item.keyEquivalentModifierMask = parts[5].hasPrefix("shift+") ? [.command, .shift] : [.command]
                }
                menu.addItem(item)
            case "S": menu.addItem(.separator())
            case ">" where parts.count >= 2:
                let item = NSMenuItem(title: parts[1], action: nil, keyEquivalent: "")
                let sub = NSMenu(title: parts[1])
                item.submenu = sub
                menu.addItem(item)
                stack.append(sub)
            case "<": if stack.count > 1 { stack.removeLast() }
            default: break
            }
        }
        return stack[0]
    }
}
