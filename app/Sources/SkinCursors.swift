// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit

/// The cursor over the skin's windows: one of the system's, or a picture a classic skin
/// brings, which may move. The library says which belongs where; this shows it.
final class SkinCursors {
    /// A skin's own cursor: a cursor for each of its pictures, and the steps that show them.
    private struct Pictures {
        let frames: [NSCursor]
        let steps: [(frame: Int, seconds: TimeInterval)]
    }

    private let app: OpaquePointer
    /// The cursor last set, as the library numbers them.
    private var shown = UInt32(ANDAMP_CURSOR_ARROW)
    /// The skin's own cursors made so far, by the library's number; nil for one that has no
    /// pictures after all.
    private var made: [UInt32: Pictures?] = [:]
    /// Steps a cursor that moves; nil while the one shown stands still.
    private var timer: Timer?
    private var step = 0

    init(app: OpaquePointer) { self.app = app }

    /// Shows cursor `kind`, a number of the library's. It is set only when it is another than
    /// the last one, so the arrow is the system's to change over what is not the skin.
    func show(_ kind: UInt32) {
        guard kind != shown else { return }
        shown = kind
        timer?.invalidate()
        timer = nil
        if kind >= UInt32(ANDAMP_CURSOR_SKIN), let pictures = pictures(kind) {
            step = 0
            pictures.frames[pictures.steps[0].frame].set()
            if pictures.steps.count > 1 { wait(pictures, for: kind) }
            return
        }
        switch Int32(kind) {
        case ANDAMP_CURSOR_LEFT_RIGHT: NSCursor.resizeLeftRight.set()
        case ANDAMP_CURSOR_UP_DOWN: NSCursor.resizeUpDown.set()
        case ANDAMP_CURSOR_DIAGONAL: Self.corner(right: true).set()
        case ANDAMP_CURSOR_OTHER_DIAGONAL: Self.corner(right: false).set()
        default: NSCursor.arrow.set()
        }
    }

    /// Another skin is worn: its cursors are other pictures.
    func forget() {
        made.removeAll()
        show(UInt32(ANDAMP_CURSOR_ARROW))
    }

    /// Shows the next picture of a cursor that moves when this one's time is over, for as
    /// long as that cursor is the one shown.
    private func wait(_ pictures: Pictures, for kind: UInt32) {
        let seconds = max(pictures.steps[step].seconds, 1.0 / 60)
        timer = Timer.scheduledTimer(withTimeInterval: seconds, repeats: false) { [weak self] _ in
            guard let self, self.shown == kind else { return }
            self.step = (self.step + 1) % pictures.steps.count
            pictures.frames[pictures.steps[self.step].frame].set()
            self.wait(pictures, for: kind)
        }
    }

    /// The skin's own cursor `kind`, made once from what the library gives.
    private func pictures(_ kind: UInt32) -> Pictures? {
        if let known = made[kind] { return known }
        let pictures = read(kind)
        made[kind] = .some(pictures)
        return pictures
    }

    private func read(_ kind: UInt32) -> Pictures? {
        var count = 0
        guard let words = andamp_app_cursor_picture(app, kind, &count) else { return nil }
        defer { andamp_words_free(words, count) }
        let all = UnsafeBufferPointer(start: words, count: count)
        var at = 0
        // the next whole number, or nil past the end
        func next() -> Int? {
            guard at < all.count else { return nil }
            defer { at += 1 }
            return Int(all[at])
        }
        var steps: [(frame: Int, seconds: TimeInterval)] = []
        for _ in 0..<(next() ?? 0) {
            guard let frame = next(), let ms = next() else { return nil }
            steps.append((frame, TimeInterval(ms) / 1000))
        }
        var frames: [NSCursor] = []
        for _ in 0..<(next() ?? 0) {
            guard let width = next(), let height = next(), let tipX = next(), let tipY = next(),
                width > 0, height > 0, at + width * height <= all.count
            else { return nil }
            let pixels = UnsafeBufferPointer(rebasing: all[at..<(at + width * height)])
            at += width * height
            guard let image = Self.image(pixels, width: width, height: height) else { return nil }
            frames.append(NSCursor(image: image, hotSpot: NSPoint(x: tipX, y: tipY)))
        }
        guard !frames.isEmpty, !steps.isEmpty, steps.allSatisfy({ $0.frame < frames.count }) else { return nil }
        return Pictures(frames: frames, steps: steps)
    }

    /// A cursor's picture from its pixels, each 0xAARRGGBB with the color not multiplied by
    /// the alpha: a point of the screen to a pixel, as Winamp shows a cursor at any size of
    /// the skin, with the pixels kept sharp on a screen that has more of them.
    private static func image(_ pixels: UnsafeBufferPointer<UInt32>, width: Int, height: Int) -> NSImage? {
        let sharp = Int((NSScreen.screens.map(\.backingScaleFactor).max() ?? 1).rounded(.up))
        let info = CGBitmapInfo.byteOrder32Little.rawValue | CGImageAlphaInfo.first.rawValue
        guard let space = CGColorSpace(name: CGColorSpace.sRGB),
            let provider = CGDataProvider(data: Data(buffer: pixels) as CFData),
            let small = CGImage(
                width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4,
                space: space, bitmapInfo: CGBitmapInfo(rawValue: info), provider: provider, decode: nil,
                shouldInterpolate: false, intent: .defaultIntent),
            let context = CGContext(
                data: nil, width: width * sharp, height: height * sharp, bitsPerComponent: 8, bytesPerRow: 0,
                space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        else { return nil }
        context.interpolationQuality = .none
        context.draw(small, in: CGRect(x: 0, y: 0, width: width * sharp, height: height * sharp))
        guard let large = context.makeImage() else { return nil }
        return NSImage(cgImage: large, size: NSSize(width: width, height: height))
    }

    /// The cursor for sizing a window by a corner: the bottom right one, or the bottom left.
    /// Before macOS 15 the system has none to give, and the crosshair stands in.
    private static func corner(right: Bool) -> NSCursor {
        if #available(macOS 15.0, *) {
            return NSCursor.frameResize(position: right ? .bottomRight : .bottomLeft, directions: .all)
        }
        return NSCursor.crosshair
    }
}
