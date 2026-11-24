// SPDX-License-Identifier: GPL-3.0-or-later
//
// Draws the Mac's app icon from Andamp's icon (app/icon/andamp.svg) as a 1024 pixel PNG: the
// rounded square of a Mac icon, 824 pixels wide with its soft shadow, filled with the middle
// 72 of the drawing's 108 units, which is the part an Android launcher shows.
//
//   swift tools/icon.swift <andamp.svg> <out.png>
import AppKit

let arguments = CommandLine.arguments
guard arguments.count == 3, let drawing = NSImage(contentsOfFile: arguments[1]) else {
    FileHandle.standardError.write(Data("usage: icon.swift <andamp.svg> <out.png>\n".utf8))
    exit(2)
}
let side = 1024.0
let square = CGRect(x: 100, y: 100, width: 824, height: 824)
guard
    let bitmap = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: Int(side), pixelsHigh: Int(side), bitsPerSample: 8,
        samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0,
        bitsPerPixel: 0),
    let context = NSGraphicsContext(bitmapImageRep: bitmap)
else { exit(1) }
NSGraphicsContext.current = context
let shape = NSBezierPath(roundedRect: square, xRadius: 185, yRadius: 185)
// the shadow a Mac icon stands on
context.cgContext.saveGState()
context.cgContext.setShadow(
    offset: CGSize(width: 0, height: -10), blur: 20, color: CGColor(gray: 0, alpha: 0.3))
NSColor.black.setFill()
shape.fill()
context.cgContext.restoreGState()
shape.addClip()
// the middle 72 of 108 units fill the square
let whole = square.width * 108 / 72
drawing.draw(
    in: CGRect(x: square.midX - whole / 2, y: square.midY - whole / 2, width: whole, height: whole),
    from: .zero, operation: .sourceOver, fraction: 1)
context.flushGraphics()
guard let png = bitmap.representation(using: .png, properties: [:]) else { exit(1) }
try png.write(to: URL(fileURLWithPath: arguments[2]))
