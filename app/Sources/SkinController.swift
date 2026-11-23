// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit

/// A loaded skin and its windows on the desktop.
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
    /// The part of the screen the windows move on, in points.
    private var area: NSRect
    private var timer: Timer?
    private var touch: Int32 = 0
    private var pressed = false
    /// The name of the skin shown.
    private(set) var name = ""

    init(zoom: Int = 1) {
        self.zoom = max(1, zoom)
        area = NSScreen.main?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1440, height: 900)
    }

    deinit { close() }

    /// The viewport in layout pixels.
    private var viewport: (Int32, Int32) {
        (Int32(area.width) / Int32(zoom), Int32(area.height) / Int32(zoom))
    }

    /// Opens the skin archive at `file` in place of the one shown. Throws what the engine says
    /// when it cannot.
    func open(file: URL, fonts: URL?) throws {
        let bytes = try Data(contentsOf: file)
        var error: UnsafeMutablePointer<CChar>?
        let (w, h) = viewport
        let opened = bytes.withUnsafeBytes { raw in
            andamp_skin_open(
                raw.bindMemory(to: UInt8.self).baseAddress, raw.count, file.lastPathComponent,
                nil, 0, nil, 0, fonts?.path, w, h, &error)
        }
        guard let opened else {
            let message = error.map { String(cString: $0) } ?? "the skin did not open"
            andamp_string_free(error)
            throw NSError(domain: "Andamp", code: 1, userInfo: [NSLocalizedDescriptionKey: message])
        }
        close()
        skin = opened
        name = file.lastPathComponent
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

    /// Sets how many points a layout pixel is: 2 is Winamp's double size.
    func setZoom(_ zoom: Int) {
        guard zoom != self.zoom, zoom >= 1, let skin else { return }
        self.zoom = zoom
        let (w, h) = viewport
        andamp_skin_set_viewport(skin, w, h)
        for window in windows.values { window.shownGeneration = 0 }
        order.removeAll()
        pump(force: true)
    }

    /// Runs the engine and shows what changed, then sleeps until the skin next changes.
    func pump(force: Bool = false) {
        guard let skin else { return }
        let changed = andamp_skin_step(skin)
        if changed != 0 || force { apply() }
        schedule()
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

    // MARK: the mouse

    /// The mouse on the viewport, in layout pixels.
    private func point() -> (Int32, Int32) {
        let p = NSEvent.mouseLocation
        let z = CGFloat(zoom)
        return (Int32(((p.x - area.minX) / z).rounded(.down)), Int32(((area.maxY - p.y) / z).rounded(.down)))
    }

    func mouseDown(_ event: NSEvent) {
        guard let skin else { return }
        let (x, y) = point()
        touch += 1
        pressed = true
        andamp_skin_pointer_down(skin, x, y, touch)
        if event.clickCount == 2 { andamp_skin_double_click(skin, x, y, touch) }
        pump()
    }

    func mouseDragged(_ event: NSEvent) {
        guard let skin, pressed else { return }
        let (x, y) = point()
        andamp_skin_pointer_move(skin, x, y)
        pump()
    }

    func mouseUp(_ event: NSEvent) {
        guard let skin, pressed else { return }
        pressed = false
        let (x, y) = point()
        andamp_skin_pointer_up(skin, x, y)
        pump()
    }

    func rightMouseDown(_ event: NSEvent) {
        guard let skin else { return }
        let (x, y) = point()
        touch += 1
        andamp_skin_pointer_down(skin, x, y, touch)
        andamp_skin_right_click(skin, touch)
        andamp_skin_pointer_up(skin, x, y)
        pump()
    }

    // MARK: for --dump and --bench

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
