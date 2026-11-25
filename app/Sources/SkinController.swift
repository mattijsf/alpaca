// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit

/// The app's windows on the desktop: one for each window of the skin worn.
///
/// The library has the app (`andamp_app_`): its player, the skin, the menu, what each command
/// does. The controller tells it where the screens are, as rectangles on the desktop in points
/// from the main screen's top left corner with y down, and again whenever they change. The
/// engine counts in layout pixels from the library's origin, the top left of the screen the
/// main window is on: a layout pixel is `zoom` points. After every step the controller puts one `SkinWindow` where the engine has each of
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
    private var timer: Timer?
    /// What watches the screens coming, going and changing.
    private var screenWatcher: NSObjectProtocol?
    /// Told when the app gets the keyboard and when another app takes it.
    private var activeWatchers: [NSObjectProtocol] = []
    /// What watches the windows going out of sight and coming back.
    private var watchers: [NSObjectProtocol] = []
    private var touch: Int32 = 0
    private var pressed = false
    /// Shows the cursor that belongs where the mouse is.
    private lazy var cursors = SkinCursors(app: app)
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
        let area = NSScreen.screens.first?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1440, height: 900)
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
        tellScreens()
        screenWatcher = NotificationCenter.default.addObserver(
            forName: NSApplication.didChangeScreenParametersNotification, object: nil, queue: .main
        ) { [weak self] _ in
            guard let self else { return }
            self.tellScreens()
            self.stale = true
            self.pump(force: true)
        }
        activeWatchers = [
            (NSApplication.didBecomeActiveNotification, true), (NSApplication.didResignActiveNotification, false),
        ].map { name, active in
            NotificationCenter.default.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                self?.appActive(active)
            }
        }
    }

    deinit {
        if let screenWatcher { NotificationCenter.default.removeObserver(screenWatcher) }
        activeWatchers.forEach(NotificationCenter.default.removeObserver)
        dropWindows()
        andamp_app_free(app)
    }

    /// The top of the main screen, from the bottom of it: the desktop the library knows counts
    /// down from here, where the Mac counts up from the bottom.
    private var desktopTop: CGFloat { NSScreen.screens.first?.frame.maxY ?? 900 }

    /// Tells the library where the screens are: the part of each the windows move on, less the
    /// menu bar and the Dock, in whole points inside it. The first is the main screen, the one
    /// with the menu bar in the system's settings.
    private func tellScreens() {
        let top = desktopTop
        var rects: [Int32] = []
        for screen in NSScreen.screens {
            let frame = screen.visibleFrame
            let (left, upper) = (frame.minX.rounded(.up), (top - frame.maxY).rounded(.up))
            let (right, lower) = (frame.maxX.rounded(.down), (top - frame.minY).rounded(.down))
            rects += [Int32(left), Int32(upper), Int32(right - left), Int32(lower - upper)]
        }
        rects.withUnsafeBufferPointer { andamp_app_set_screens(app, $0.baseAddress, Int32($0.count / 4)) }
    }

    /// Where the library counts the windows and the mouse from, on the desktop.
    private var origin: (x: CGFloat, y: CGFloat) {
        var x: Int32 = 0
        var y: Int32 = 0
        andamp_app_origin(app, &x, &y)
        return (CGFloat(x), CGFloat(y))
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

    /// A place on the library's desktop, in points, as a place on the Mac's screens.
    func screenPoint(_ x: Int, _ y: Int) -> NSPoint {
        NSPoint(x: CGFloat(x), y: desktopTop - CGFloat(y))
    }

    /// Closes the windows; the skin they showed is gone.
    private func dropWindows() {
        timer?.invalidate()
        timer = nil
        watchers.forEach(NotificationCenter.default.removeObserver)
        watchers.removeAll()
        for window in windows.values {
            window.parent?.removeChildWindow(window)
            window.close()
        }
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
                cursors.forget()
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
        var infos: [AndampWindow] = []
        var info = AndampWindow()
        for index in 0..<andamp_skin_window_count(skin) where andamp_skin_window(skin, index, &info) {
            infos.append(info)
        }
        let seen = infos.map(\.id)
        let z = CGFloat(zoom)
        let (ox, oy) = origin
        let top = desktopTop
        // the engine counts from the origin down, the Mac from the main screen's bottom up
        var wanted: [UInt32: NSRect] = [:]
        for info in infos {
            wanted[info.id] = NSRect(
                x: ox + CGFloat(info.x) * z,
                y: top - oy - CGFloat(info.y + info.h) * z,
                width: CGFloat(info.w) * z, height: CGFloat(info.h) * z)
            if windows[info.id] == nil { windows[info.id] = makeWindow(info.id) }
        }
        for (id, window) in windows where !seen.contains(id) {
            // a window that goes takes none of the others with it
            for child in window.childWindows ?? [] { window.removeChildWindow(child) }
            window.parent?.removeChildWindow(window)
            window.close()
            windows[id] = nil
        }
        place(wanted)
        for info in infos {
            guard let window = windows[info.id], window.shownGeneration != info.generation,
                let pixels = info.pixels
            else { continue }
            // the drawing right after the one shown differs from it only where it says
            let next = window.shownGeneration != 0 && window.shownGeneration + 1 == info.generation
            let changed = CGRect(
                x: Int(info.changed_x), y: Int(info.changed_y), width: Int(info.changed_w),
                height: Int(info.changed_h))
            window.show(pixels: pixels, width: Int(info.w), height: Int(info.h), changed: next ? changed : nil)
            window.shownGeneration = info.generation
        }
        // bottom to top, as the engine has them
        if seen != order {
            order = seen
            for id in seen { windows[id]?.orderFront(nil) }
            if let top = seen.last, NSApp.isActive { windows[top]?.makeKey() }
        }
    }

    private func makeWindow(_ id: UInt32) -> SkinWindow {
        let made = SkinWindow(skinId: id, controller: self)
        made.level = onTop ? .floating : .normal
        made.alphaValue = opacity
        watchers.append(
            NotificationCenter.default.addObserver(
                forName: NSWindow.didChangeOcclusionStateNotification, object: made, queue: .main
            ) { [weak self] _ in self?.sightChanged() })
        return made
    }

    /// Puts every window where the engine has it, and keeps the skin's windows one group
    /// around the main one: the Mac then shows the player as one in Mission Control and
    /// brings its windows forward together.
    ///
    /// The Mac moves a group's other windows along when its first one moves. A window the
    /// engine does not move along, one that is not docked to the main window, is let go of
    /// before the move and taken back after it, so it stays where it is.
    private func place(_ wanted: [UInt32: NSRect]) {
        let main = andamp_app_main_window(app)
        let lead = windows[main]
        if let lead, let to = wanted[main], to.origin != lead.frame.origin {
            let (dx, dy) = (to.minX - lead.frame.minX, to.minY - lead.frame.minY)
            for case let child as SkinWindow in lead.childWindows ?? [] {
                guard let goes = wanted[child.skinId] else { continue }
                let along = NSPoint(x: child.frame.minX + dx, y: child.frame.minY + dy)
                if goes.origin != along { lead.removeChildWindow(child) }
            }
        }
        // the main window first: what moved along with it is then where it belongs already
        if let lead, let to = wanted[main], lead.frame != to { lead.setFrame(to, display: false) }
        for (id, to) in wanted where id != main {
            guard let window = windows[id] else { continue }
            if window.frame != to { window.setFrame(to, display: false) }
            if let lead, window.parent !== lead { lead.addChildWindow(window, ordered: .above) }
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
        let (ox, oy) = origin
        return (Int32(((p.x - ox) / z).rounded(.down)), Int32(((desktopTop - p.y - oy) / z).rounded(.down)))
    }

    func mouseDown(_ event: NSEvent) {
        guard skin != nil, !inMenu else { return }
        let (x, y) = point()
        touch += 1
        pressed = true
        let held = event.modifierFlags
        andamp_app_modifiers(app, held.contains(.command), held.contains(.shift))
        andamp_app_pointer_down(app, x, y, touch)
        if event.clickCount == 2 { andamp_app_double_click(app, x, y, touch) }
        pump()
    }

    func mouseDragged(_ event: NSEvent) {
        guard skin != nil, pressed, !inMenu else { return }
        let (x, y) = point()
        andamp_app_pointer_move(app, x, y)
        pump()
        showCursor()
    }

    /// The mouse moved over the skin with no button down, or left it: what is under it hears
    /// it, the library says which cursor belongs where it is, and the arrow belongs off the skin.
    func mouseMoved(inside: Bool) {
        guard skin != nil, !pressed, !inMenu else { return }
        if inside {
            let (x, y) = point()
            andamp_app_hover(app, x, y)
        } else {
            andamp_app_pointer_leave(app)
        }
        pump()
        showCursor(inside: inside)
    }

    /// The app got the keyboard, or another app took it: every window of the skin shows itself
    /// behind while another app has it.
    func appActive(_ active: Bool) {
        guard skin != nil else { return }
        andamp_app_set_active(app, active)
        pump()
    }

    /// The cursor the library names for the mouse's place, or the arrow off the skin.
    private func showCursor(inside: Bool = true) {
        let (x, y) = point()
        cursors.show(inside ? andamp_app_cursor(app, x, y) : UInt32(ANDAMP_CURSOR_ARROW))
    }

    /// The library holds a release back when the press was too short to be seen, and the
    /// timer set by `schedule` then lets it go.
    func mouseUp(_ event: NSEvent) {
        guard skin != nil, pressed, !inMenu else { return }
        pressed = false
        let (x, y) = point()
        andamp_app_pointer_up(app, x, y)
        pump()
        showCursor()
    }

    func rightMouseDown(_ event: NSEvent) {
        guard skin != nil, !inMenu else { return }
        let (x, y) = point()
        touch += 1
        andamp_app_right_click(app, x, y, touch)
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
        guard let name = named[event.keyCode] ?? event.charactersIgnoringModifiers?.lowercased() else {
            return false
        }
        // Shift goes along with the keys that move a selection
        let moves = ["up", "down", "pageup", "pagedown", "home", "end"].contains(name)
        let key = moves && event.modifierFlags.contains(.shift) ? "shift+" + name : name
        guard andamp_app_key(app, window, key) else { return false }
        pump(force: true)
        return true
    }

    /// The skin's window `window` became the one the keys go to.
    func becameKey(_ window: UInt32) {
        guard skin != nil else { return }
        andamp_app_focus(app, window)
        // drawn after this call: a step of the app can itself be what made the window key
        DispatchQueue.main.async { [weak self] in self?.pump() }
    }

    // MARK: for --click, --key-window, --dump and --bench

    /// Gives the keys to the skin's window `window`, as Command-` does.
    func makeKey(window: UInt32) { windows[window]?.makeKey() }

    /// A click at `x`, `y` on the viewport, in layout pixels, as if the mouse made it.
    func click(x: Int32, y: Int32) {
        guard skin != nil else { return }
        touch += 1
        andamp_app_pointer_down(app, x, y, touch)
        andamp_app_pointer_up(app, x, y)
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
