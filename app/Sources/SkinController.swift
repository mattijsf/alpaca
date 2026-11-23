// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit

/// A loaded skin and its windows on the desktop, and the app's player behind it.
///
/// The engine has the screen as its viewport, in layout pixels: a layout pixel is `zoom`
/// points. After every step the controller puts one `SkinWindow` where the engine has each of
/// its windows and hands over the pixels of those that changed. It sleeps until the engine
/// says the skin next changes by itself.
final class SkinController {
    private var skin: OpaquePointer?
    private var windows: [UInt32: SkinWindow] = [:]
    /// The engine's windows from the bottom up, as last shown.
    private var order: [UInt32] = []
    /// Points per layout pixel.
    private(set) var zoom: Int
    /// Whether the windows stay above other apps' windows.
    private(set) var onTop = false
    /// The part of the screen the windows move on, in points.
    private var area: NSRect
    private var timer: Timer?
    private var touch: Int32 = 0
    private var pressed = false
    /// Set while one of the skin's menus is open: the engine waits for the answer inside a
    /// mouse call, and must not be called again until it has it.
    private var inMenu = false
    /// The name of the skin shown.
    private(set) var name = ""
    /// The app's player, which every skin shows and drives.
    let player: OpaquePointer
    /// Called when the skin's eject button asks for files to play.
    var onOpenFiles: (() -> Void)?
    /// Called when a button of the skin asks for the app's menu, with the place for it on the screen.
    var onMenu: ((NSPoint) -> Void)?
    /// Called with files dropped on a window.
    var onDrop: (([URL]) -> Void)?

    init(zoom: Int = 1) {
        player = andamp_player_new()
        self.zoom = max(1, zoom)
        area = NSScreen.main?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1440, height: 900)
    }

    deinit {
        close()
        andamp_player_free(player)
    }

    /// The viewport in layout pixels.
    private var viewport: (Int32, Int32) {
        (Int32(area.width) / Int32(zoom), Int32(area.height) / Int32(zoom))
    }

    /// A place on the viewport, in layout pixels, as a place on the screen.
    private func screenPoint(_ x: Int, _ y: Int) -> NSPoint {
        NSPoint(x: area.minX + CGFloat(x * zoom), y: area.maxY - CGFloat(y * zoom))
    }

    /// What lies under a skin: a classic skin under every classic skin, which gives the
    /// pictures a skin leaves out; under a modern skin a zip of a wasabi folder or, without
    /// one, a folder of such files; and the folder of the fonts that stand in for Arial and Tahoma.
    struct Under {
        var base: URL?
        var wasabi: URL?
        var wasabiFolder: URL?
        var fonts: URL?
    }

    /// Opens the skin archive at `file` in place of the one shown. Throws what the engine says
    /// when it cannot.
    func open(file: URL, under: Under) throws {
        let bytes = try Data(contentsOf: file)
        let base = under.base.flatMap { try? Data(contentsOf: $0) } ?? Data()
        let wasabi = under.wasabi.flatMap { try? Data(contentsOf: $0) } ?? Data()
        var error: UnsafeMutablePointer<CChar>?
        let (w, h) = viewport
        func pointer(_ raw: UnsafeRawBufferPointer) -> UnsafePointer<UInt8>? {
            raw.isEmpty ? nil : raw.bindMemory(to: UInt8.self).baseAddress
        }
        let opened = bytes.withUnsafeBytes { skin in
            base.withUnsafeBytes { base in
                wasabi.withUnsafeBytes { wasabi in
                    andamp_skin_open(
                        pointer(skin), skin.count, file.lastPathComponent, pointer(base), base.count,
                        pointer(wasabi), wasabi.count, under.wasabiFolder?.path, under.fonts?.path, w, h,
                        player, &error)
                }
            }
        }
        guard let opened else {
            let message = error.map { String(cString: $0) } ?? "the skin did not open"
            andamp_string_free(error)
            throw NSError(domain: "Andamp", code: 1, userInfo: [NSLocalizedDescriptionKey: message])
        }
        close()
        skin = opened
        name = file.lastPathComponent
        andamp_skin_set_menu_handler(
            opened,
            { context, menu in
                guard let context, let menu else { return 0 }
                let controller = Unmanaged<SkinController>.fromOpaque(context).takeUnretainedValue()
                return controller.showSkinMenu(String(cString: menu))
            }, Unmanaged.passUnretained(self).toOpaque())
        tellSwitches()
        // the windows open in the screen's top left corner: a little in from it
        andamp_skin_move_windows(opened, 120 / Int32(zoom), 80 / Int32(zoom))
        pump()
    }

    /// Closes the skin and its windows.
    func close() {
        timer?.invalidate()
        timer = nil
        for window in windows.values { window.close() }
        windows.removeAll()
        order.removeAll()
        if let skin { andamp_skin_free(skin) }
        skin = nil
    }

    /// Tells the skin which of the app's switches are on, so its buttons show them.
    private func tellSwitches() {
        guard let skin else { return }
        andamp_skin_set_switch(skin, "doublesize", zoom > 1)
        andamp_skin_set_switch(skin, "ontop", onTop)
    }

    /// Sets how many points a layout pixel is: 2 is Winamp's double size.
    func setZoom(_ zoom: Int) {
        guard zoom != self.zoom, zoom >= 1, let skin else { return }
        // the main window's corner stays where it is
        let corner = windows[order.last ?? 0].map { NSPoint(x: $0.frame.minX, y: $0.frame.maxY) }
        self.zoom = zoom
        let (w, h) = viewport
        andamp_skin_set_viewport(skin, w, h)
        tellSwitches()
        for window in windows.values { window.shownGeneration = 0 }
        order.removeAll()
        pump(force: true)
        if let corner, let now = windows[order.last ?? 0] {
            let dx = (corner.x - now.frame.minX) / CGFloat(zoom)
            let dy = (now.frame.maxY - corner.y) / CGFloat(zoom)
            andamp_skin_move_windows(skin, Int32(dx.rounded()), Int32(dy.rounded()))
            pump(force: true)
        }
    }

    /// Keeps the windows above other apps' windows, or not.
    func setOnTop(_ onTop: Bool) {
        self.onTop = onTop
        for window in windows.values { window.level = onTop ? .floating : .normal }
        tellSwitches()
        pump(force: true)
    }

    /// Runs the engine and shows what changed, then sleeps until the skin next changes.
    func pump(force: Bool = false) {
        guard let skin, !inMenu else { return }
        let changed = andamp_skin_step(skin)
        if changed != 0 || force { apply() }
        schedule()
        if andamp_player_take_requests(player) & UInt32(ANDAMP_REQUEST_OPEN_FILES) != 0 {
            // after this turn of the run loop: the panel must not open inside a mouse event
            DispatchQueue.main.async { [weak self] in self?.onOpenFiles?() }
        }
        if let actions = andamp_skin_take_actions(skin) {
            let lines = String(cString: actions).split(separator: "\n").map(String.init)
            andamp_string_free(actions)
            DispatchQueue.main.async { [weak self] in lines.forEach { self?.act($0) } }
        }
    }

    /// Answers something a button of the skin asked of the app: `line` is the action, its
    /// param and the place it came from, with tabs between.
    private func act(_ line: String) {
        let parts = line.split(separator: "\t", omittingEmptySubsequences: false).map(String.init)
        let at: NSPoint? =
            parts.count >= 4
            ? Int(parts[2]).flatMap { x in Int(parts[3]).map { y in screenPoint(x, y) } } : nil
        switch parts.first ?? "" {
        case "CLOSE": NSApp.terminate(nil)
        case "MINIMIZE": NSApp.hide(nil)
        case "DOUBLESIZE": setZoom(zoom == 1 ? 2 : 1)
        case "ALWAYS_ON_TOP": setOnTop(!onTop)
        case "MENU", "SYSMENU", "CONTROLMENU": onMenu?(at ?? NSEvent.mouseLocation)
        case "ABOUT": NSApp.orderFrontStandardAboutPanel(nil)
        case "PE_ADD": onOpenFiles?()
        default: break
        }
    }

    /// Puts the app's windows where the engine's are and shows the pixels that changed.
    private func apply() {
        guard let skin else { return }
        let count = andamp_skin_window_count(skin)
        var seen: [UInt32] = []
        var info = AndampWindow()
        let z = CGFloat(zoom)
        for index in 0..<count where andamp_skin_window(skin, index, &info) {
            seen.append(info.id)
            let window = windows[info.id] ?? {
                let made = SkinWindow(skinId: info.id, controller: self)
                made.level = onTop ? .floating : .normal
                windows[info.id] = made
                return made
            }()
            // the engine counts from the top left of the area, the screen from its bottom left
            let frame = NSRect(
                x: area.minX + CGFloat(info.x) * z,
                y: area.maxY - CGFloat(info.y + info.h) * z,
                width: CGFloat(info.w) * z, height: CGFloat(info.h) * z)
            if window.frame != frame { window.setFrame(frame, display: false) }
            if window.shownGeneration != info.generation, let pixels = info.pixels {
                window.show(pixels: pixels, width: Int(info.w), height: Int(info.h))
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

    /// Wakes the controller when the skin next changes by itself; not at all when it will not.
    private func schedule() {
        timer?.invalidate()
        timer = nil
        guard let skin else { return }
        let due = andamp_skin_next_due_ms(skin)
        guard due >= 0 else { return }
        // "now" means the next frame of the screen
        let wait = max(Double(due) / 1000, 1.0 / 120)
        let next = Timer(timeInterval: wait, repeats: false) { [weak self] _ in self?.pump() }
        next.tolerance = 0.002
        // a timer in the common modes also fires while the mouse is held on a slider
        RunLoop.main.add(next, forMode: .common)
        timer = next
    }

    // MARK: the skin's menus

    /// Shows a popup menu of the skin at the mouse and gives the id picked, 0 for none.
    /// `text` is the menu as the library spells it out, a line for each item.
    private func showSkinMenu(_ text: String) -> Int32 {
        let pick = MenuPick()
        var stack = [NSMenu()]
        for line in text.split(separator: "\n") {
            let parts = line.split(separator: "\t", omittingEmptySubsequences: false).map(String.init)
            guard let menu = stack.last else { break }
            switch parts.first {
            case "C" where parts.count >= 5:
                let item = NSMenuItem(title: parts[4], action: #selector(MenuPick.pick(_:)), keyEquivalent: "")
                item.target = pick
                item.tag = Int(parts[1]) ?? 0
                item.state = parts[2] == "1" ? .on : .off
                if parts[3] == "1" {
                    item.action = nil
                    item.target = nil
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
        guard let menu = stack.first, menu.numberOfItems > 0 else { return 0 }
        // the engine waits inside a mouse call: nothing may call it until the menu is gone
        inMenu = true
        menu.popUp(positioning: nil, at: NSEvent.mouseLocation, in: nil)
        inMenu = false
        return Int32(pick.picked)
    }

    // MARK: the player

    /// Adds audio files to the playlist; how many of them were audio.
    @discardableResult
    func add(files: [URL]) -> Int {
        let added = files.filter { andamp_player_add(player, $0.path) >= 0 }.count
        pump(force: true)
        return added
    }

    /// Empties the playlist, adds `files` and plays the first.
    func play(files: [URL]) {
        andamp_player_clear(player)
        if add(files: files) > 0 { andamp_player_play_index(player, 0) }
        pump(force: true)
    }

    /// The transport: `ANDAMP_PLAY` and the like.
    func command(_ command: Int32) {
        andamp_player_command(player, command)
        pump(force: true)
    }

    func setVolume(_ volume: Int) {
        andamp_player_set_volume(player, Int32(max(0, min(255, volume))))
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

    /// Winamp's keys, with no modifier: Z X C V B for the transport, the arrows for the
    /// volume and for five seconds back and on. False for a key that is not one of them.
    func keyDown(_ event: NSEvent) -> Bool {
        guard event.modifierFlags.intersection([.command, .control, .option]).isEmpty else { return false }
        switch event.charactersIgnoringModifiers?.lowercased() {
        case "z": command(ANDAMP_PREVIOUS)
        case "x": command(ANDAMP_PLAY)
        case "c", " ": command(ANDAMP_PAUSE)
        case "v": command(ANDAMP_STOP)
        case "b": command(ANDAMP_NEXT)
        default:
            switch event.keyCode {
            case 126: setVolume(Int(andamp_player_volume(player)) + 5)  // up
            case 125: setVolume(Int(andamp_player_volume(player)) - 5)  // down
            case 123:  // left
                andamp_player_seek_by(player, -5000)
                pump(force: true)
            case 124:  // right
                andamp_player_seek_by(player, 5000)
                pump(force: true)
            default: return false
            }
        }
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
