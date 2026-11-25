import AppKit

/// A tooltip beside the mouse, drawn like the system's. The library says when it shows and what
/// it says; this only puts it on screen.
final class Tooltip {
    private let panel: NSPanel
    private let label = NSTextField(labelWithString: "")
    private var shown: String?

    init() {
        panel = NSPanel(
            contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: true)
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.level = .statusBar
        panel.ignoresMouseEvents = true
        panel.hidesOnDeactivate = false
        let box = NSVisualEffectView()
        box.material = .toolTip
        box.state = .active
        box.wantsLayer = true
        box.layer?.cornerRadius = 4
        label.font = .toolTipsFont(ofSize: 0)
        label.textColor = .labelColor
        box.addSubview(label)
        panel.contentView = box
    }

    /// Shows `text` just above `point` (screen coordinates), its left edge at the point, as
    /// Winamp's tooltip script places it; or hides the tooltip for nil.
    func show(_ text: String?, at point: NSPoint) {
        guard let text else {
            if shown != nil { panel.orderOut(nil) }
            shown = nil
            return
        }
        guard text != shown else { return }
        shown = text
        label.stringValue = text
        label.sizeToFit()
        let size = NSSize(width: label.frame.width + 12, height: label.frame.height + 6)
        label.setFrameOrigin(NSPoint(x: 6, y: 3))
        var origin = NSPoint(x: point.x, y: point.y)
        // kept on the screen the mouse is on
        if let screen = NSScreen.screens.first(where: { NSMouseInRect(point, $0.frame, false) }) {
            let area = screen.visibleFrame
            origin.x = min(max(origin.x, area.minX), area.maxX - size.width)
            origin.y = min(max(origin.y, area.minY), area.maxY - size.height)
        }
        panel.setFrame(NSRect(origin: origin, size: size), display: true)
        panel.orderFront(nil)
    }
}
