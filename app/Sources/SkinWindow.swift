// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import IOSurface

/// One window of the skin on the desktop: no frame, clear where the skin draws nothing, and
/// never moved by the system. The engine says where it is; a drag on the skin's title bar is
/// the engine's too.
final class SkinWindow: NSWindow {
    /// The engine's id for the window.
    let skinId: UInt32
    /// The count the engine gave with the pixels shown.
    var shownGeneration: UInt64 = 0
    private let surfaceView: SurfaceView

    init(skinId: UInt32, controller: SkinController) {
        self.skinId = skinId
        surfaceView = SurfaceView(controller: controller)
        super.init(
            contentRect: NSRect(x: 0, y: 0, width: 1, height: 1), styleMask: .borderless,
            backing: .buffered, defer: false)
        isOpaque = false
        backgroundColor = .clear
        hasShadow = false
        isMovable = false
        isReleasedWhenClosed = false
        animationBehavior = .none
        contentView = surfaceView
        // the keys go to the view
        initialFirstResponder = surfaceView
    }

    // a window without a title bar takes the keyboard only when it says it can
    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { true }

    /// Shows `width` x `height` pixels, rows top first, BGRA with the color multiplied by the
    /// alpha. `changed` is the part that differs from the pixels shown last, when that is
    /// known; without it the whole picture is taken over.
    func show(pixels: UnsafePointer<UInt32>, width: Int, height: Int, changed: CGRect?) {
        surfaceView.show(pixels: pixels, width: width, height: height, changed: changed)
    }

    /// The pixels shown, as a picture, for `--dump`.
    func picture() -> CGImage? { surfaceView.picture() }
}

/// The view that shows a window's pixels and passes the mouse on.
///
/// The pixels go into an `IOSurface` that the layer shows as it is: the window server scales
/// it by whole numbers with nearest pixels and converts its colors, so the app draws every
/// layout pixel once, whatever the zoom and the screen.
final class SurfaceView: NSView {
    private unowned let controller: SkinController
    /// Two surfaces taken in turn: the window server may still read the one shown.
    private var surfaces: [IOSurface] = []
    /// For each surface, the part of the picture it does not have yet: what changed while the
    /// other one was shown.
    private var behind: [CGRect] = [.null, .null]
    private var shown = 0
    private var size = (width: 0, height: 0)

    init(controller: SkinController) {
        self.controller = controller
        super.init(frame: .zero)
        wantsLayer = true
        layer?.isOpaque = false
        layer?.contentsGravity = .resize
        layer?.magnificationFilter = .nearest
        layer?.minificationFilter = .nearest
        // a change of pixels shows at once, with no fade
        layer?.actions = ["contents": NSNull(), "bounds": NSNull(), "position": NSNull()]
        registerForDraggedTypes([.fileURL])
    }

    required init?(coder: NSCoder) { fatalError("not from a nib") }

    override var isFlipped: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override var mouseDownCanMoveWindow: Bool { false }

    private static let sRGB = CGColorSpace(name: CGColorSpace.sRGB)!

    private func makeSurface(width: Int, height: Int) -> IOSurface? {
        let surface = IOSurface(properties: [
            .width: width,
            .height: height,
            .bytesPerElement: 4,
            .pixelFormat: kCVPixelFormatType_32BGRA,
        ])
        // the skin's colors are sRGB: said once, the window server converts them for the screen
        if let surface, let space = Self.sRGB.copyPropertyList() {
            IOSurfaceSetValue(unsafeBitCast(surface, to: IOSurfaceRef.self), kIOSurfaceColorSpace, space)
        }
        return surface
    }

    func show(pixels: UnsafePointer<UInt32>, width: Int, height: Int, changed: CGRect?) {
        guard width > 0, height > 0 else { return }
        let whole = CGRect(x: 0, y: 0, width: width, height: height)
        var changed = changed ?? whole
        if size != (width, height) || surfaces.count < 2 {
            size = (width, height)
            surfaces = (0..<2).compactMap { _ in makeSurface(width: width, height: height) }
            shown = 0
            changed = whole
        }
        guard surfaces.count == 2 else { return }
        // both surfaces fall behind by what changed; the one written now catches up
        behind = behind.map { $0.union(changed) }
        let next = 1 - shown
        let part = behind[next].intersection(whole).integral
        behind[next] = .null
        let surface = surfaces[next]
        surface.lock(options: [], seed: nil)
        let rowBytes = surface.bytesPerRow
        let base = surface.baseAddress
        let (left, top, columns, rows) = (Int(part.minX), Int(part.minY), Int(part.width), Int(part.height))
        if columns > 0 {
            for row in top..<(top + rows) {
                memcpy(base + row * rowBytes + left * 4, pixels + row * width + left, columns * 4)
            }
        }
        surface.unlock(options: [], seed: nil)
        layer?.contents = surface
        shown = next
    }

    func picture() -> CGImage? {
        guard surfaces.count == 2 else { return nil }
        let surface = surfaces[shown]
        surface.lock(options: .readOnly, seed: nil)
        defer { surface.unlock(options: .readOnly, seed: nil) }
        let info = CGBitmapInfo.byteOrder32Little.rawValue | CGImageAlphaInfo.premultipliedFirst.rawValue
        let context = CGContext(
            data: surface.baseAddress, width: size.width, height: size.height, bitsPerComponent: 8,
            bytesPerRow: surface.bytesPerRow, space: Self.sRGB, bitmapInfo: info)
        return context?.makeImage()
    }

    // the mouse goes to the engine in screen places: a window dragged moves under the mouse,
    // so a place inside the window would not do

    override func mouseDown(with event: NSEvent) { controller.mouseDown(event) }
    override func mouseDragged(with event: NSEvent) { controller.mouseDragged(event) }
    override func mouseUp(with event: NSEvent) { controller.mouseUp(event) }
    override func rightMouseDown(with event: NSEvent) { controller.rightMouseDown(event) }
    override func scrollWheel(with event: NSEvent) { controller.scrollWheel(event) }

    override var acceptsFirstResponder: Bool { true }

    override func keyDown(with event: NSEvent) {
        if !controller.keyDown(event) { super.keyDown(with: event) }
    }

    // files dropped on a window: songs to play, or a skin to wear

    private func files(_ sender: NSDraggingInfo) -> [URL] {
        let options: [NSPasteboard.ReadingOptionKey: Any] = [.urlReadingFileURLsOnly: true]
        return sender.draggingPasteboard.readObjects(forClasses: [NSURL.self], options: options) as? [URL] ?? []
    }

    override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation {
        files(sender).isEmpty ? [] : .copy
    }

    override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        let dropped = files(sender)
        guard !dropped.isEmpty else { return false }
        controller.open(dropped)
        return true
    }
}
