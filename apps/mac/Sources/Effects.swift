// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import UniformTypeIdentifiers

/// The effect rack as the library describes it.
struct Rack: Decodable {
    struct Option: Decodable {
        let name: String
        let value: Double
    }

    struct Param: Decodable {
        let id: String
        let name: String
        let kind: String
        let min: Double
        let max: Double
        let `default`: Double
        let value: Double
        let unit: String
        let help: String
        let scale: Double
        let decimals: Int
        let zero: String
        /// The value as the listener reads it: "50 %", "-3.0 dB".
        let readout: String
        let options: [Option]
    }

    struct Preset: Decodable {
        let name: String
        let values: [[Double]]
    }

    struct Effect: Decodable {
        let id: String
        let name: String
        let version: String
        let author: String
        let about: String
        let on: Bool
        let error: String
        let params: [Param]
        let presets: [Preset]
    }

    let effects: [Effect]
}

/// The effects: the plug-ins that come with the app and the listener's own, and the window
/// that shows the rack with a switch for each effect and its controls.
///
/// A plug-in is a Lua file in the format of Alpaca on Android. The library loads them, the
/// ones that come with the app and the listener's own, the first time they are needed: when
/// the window opens, or at the start when one was left on. This is the window.
final class Effects: NSObject, NSWindowDelegate {
    private let app: OpaquePointer
    private var player: OpaquePointer { alpaca_app_player(app) }
    private var window: NSWindow?
    private let list = NSStackView()
    /// What the controls call; kept for as long as the controls are.
    private var handlers: [Handler] = []
    /// The readouts beside the sliders, by effect and control.
    private var readouts: [String: NSTextField] = [:]

    init(app: OpaquePointer) {
        self.app = app
        super.init()
    }

    /// The rack as it stands.
    var rack: Rack {
        guard let json = alpaca_player_effects(player) else { return Rack(effects: []) }
        defer { alpaca_string_free(json) }
        let data = Data(String(cString: json).utf8)
        return (try? JSONDecoder().decode(Rack.self, from: data)) ?? Rack(effects: [])
    }

    /// Loads every plug-in once: the library knows which come with the app, which are the
    /// listener's own, and how each was left.
    private func load() { alpaca_app_load_effects(app) }

    /// Keeps which effects are on and where their controls stand.
    private func save() { alpaca_app_save(app) }

    // MARK: the window

    /// Shows the rack.
    func show() {
        load()
        if window == nil {
            let made = NSWindow(
                contentRect: NSRect(x: 0, y: 0, width: 480, height: 620),
                styleMask: [.titled, .closable, .resizable, .miniaturizable], backing: .buffered, defer: false)
            made.title = "Effects"
            made.isReleasedWhenClosed = false
            made.delegate = self
            list.orientation = .vertical
            list.alignment = .leading
            list.spacing = 14
            list.edgeInsets = NSEdgeInsets(top: 16, left: 16, bottom: 16, right: 16)
            list.translatesAutoresizingMaskIntoConstraints = false
            let scroll = NSScrollView()
            scroll.hasVerticalScroller = true
            scroll.drawsBackground = false
            let clip = FlippedView()
            clip.translatesAutoresizingMaskIntoConstraints = false
            clip.addSubview(list)
            scroll.documentView = clip
            made.contentView = scroll
            NSLayoutConstraint.activate([
                list.topAnchor.constraint(equalTo: clip.topAnchor),
                list.leadingAnchor.constraint(equalTo: clip.leadingAnchor),
                list.trailingAnchor.constraint(equalTo: clip.trailingAnchor),
                list.bottomAnchor.constraint(equalTo: clip.bottomAnchor),
                clip.widthAnchor.constraint(equalTo: scroll.contentView.widthAnchor),
            ])
            made.contentMinSize = NSSize(width: 440, height: 240)
            made.center()
            window = made
        }
        rebuild()
        window?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    func windowWillClose(_ notification: Notification) { save() }

    /// The window as a picture, for `--dump`.
    func picture() -> NSBitmapImageRep? { window?.contentView?.snapshot() }

    /// Builds the window's rows again from the rack.
    private func rebuild() {
        for view in list.arrangedSubviews { view.removeFromSuperview() }
        handlers.removeAll()
        readouts.removeAll()
        for (index, effect) in rack.effects.enumerated() {
            let card = card(index, effect)
            list.addArrangedSubview(card)
            // as wide as the window, less the margins
            card.widthAnchor.constraint(equalTo: list.widthAnchor, constant: -32).isActive = true
        }
        let add = button("Add Plug-in…") { [weak self] _ in self?.addPlugin() }
        list.addArrangedSubview(add)
    }

    private func on(_ control: NSControl, _ run: @escaping (NSControl) -> Void) {
        let handler = Handler(run)
        handlers.append(handler)
        control.target = handler
        control.action = #selector(Handler.fire(_:))
    }

    private func button(_ title: String, _ run: @escaping (NSControl) -> Void) -> NSButton {
        let made = NSButton(title: title, target: nil, action: nil)
        on(made, run)
        return made
    }

    private func text(_ string: String, small: Bool = false, color: NSColor = .labelColor) -> NSTextField {
        let made = NSTextField(wrappingLabelWithString: string)
        made.font = .systemFont(ofSize: small ? NSFont.smallSystemFontSize : NSFont.systemFontSize)
        made.textColor = color
        made.isSelectable = false
        return made
    }

    /// One effect: its switch, its presets, what it does and its controls.
    private func card(_ index: Int, _ effect: Rack.Effect) -> NSView {
        let card = NSStackView()
        card.orientation = .vertical
        card.alignment = .leading
        card.spacing = 6

        let power = NSButton(checkboxWithTitle: effect.name, target: nil, action: nil)
        power.font = .boldSystemFont(ofSize: NSFont.systemFontSize)
        power.state = effect.on ? .on : .off
        on(power) { [weak self] sender in
            guard let self else { return }
            alpaca_player_set_effect_on(self.player, Int32(index), (sender as? NSButton)?.state == .on)
            self.save()
        }
        // a pull-down with the presets, back to the defaults and, for the listener's own, remove
        let menu = NSPopUpButton(frame: .zero, pullsDown: true)
        menu.addItem(withTitle: "Presets")
        for preset in effect.presets { menu.addItem(withTitle: preset.name) }
        menu.menu?.addItem(.separator())
        menu.addItem(withTitle: "Reset to Defaults")
        if alpaca_app_effect_own(self.app, Int32(index)) { menu.addItem(withTitle: "Remove Plug-in") }
        on(menu) { [weak self] sender in
            guard let self, let picked = (sender as? NSPopUpButton)?.indexOfSelectedItem else { return }
            let title = (sender as? NSPopUpButton)?.titleOfSelectedItem
            if title == "Remove Plug-in" {
                alpaca_app_remove_plugin(self.app, Int32(index))
            } else if title == "Reset to Defaults" {
                alpaca_player_apply_preset(self.player, Int32(index), -1)
            } else if picked >= 1, picked <= effect.presets.count {
                alpaca_player_apply_preset(self.player, Int32(index), Int32(picked - 1))
            }
            self.save()
            self.rebuild()
        }
        let header = NSStackView(views: [power, NSView(), menu])
        header.distribution = .fill
        card.addArrangedSubview(header)
        header.widthAnchor.constraint(equalTo: card.widthAnchor).isActive = true

        if !effect.about.isEmpty {
            card.addArrangedSubview(text(effect.about, small: true, color: .secondaryLabelColor))
        }
        if !effect.error.isEmpty {
            card.addArrangedSubview(
                text("Left out of this song: \(effect.error)", small: true, color: .systemRed))
        }
        for (place, param) in effect.params.enumerated() {
            card.addArrangedSubview(row(index, place, param))
        }
        return card
    }

    /// One control: its name, the control and what it reads.
    private func row(_ index: Int, _ place: Int, _ param: Rack.Param) -> NSView {
        let name = text(param.name)
        name.toolTip = param.help.isEmpty ? nil : param.help
        name.widthAnchor.constraint(equalToConstant: 110).isActive = true
        let set = { [weak self] (value: Double) in
            guard let self else { return }
            alpaca_player_set_effect_value(self.player, Int32(index), Int32(place), Float(value))
        }
        let control: NSView
        var readout: NSTextField?
        switch param.kind {
        case "toggle":
            let made = NSSwitch()
            made.state = param.value >= 0.5 ? .on : .off
            on(made) { [weak self] sender in
                set((sender as? NSSwitch)?.state == .on ? 1 : 0)
                self?.save()
            }
            control = made
        case "choice":
            let made = NSPopUpButton(frame: .zero, pullsDown: false)
            for option in param.options { made.addItem(withTitle: option.name) }
            made.selectItem(at: param.options.firstIndex { $0.value == param.value } ?? 0)
            on(made) { [weak self] sender in
                let picked = (sender as? NSPopUpButton)?.indexOfSelectedItem ?? 0
                if param.options.indices.contains(picked) { set(param.options[picked].value) }
                self?.save()
            }
            control = made
        default:
            let made = NSSlider(value: param.value, minValue: param.min, maxValue: param.max, target: nil, action: nil)
            made.isContinuous = true
            let shown = text(param.readout)
            shown.alignment = .right
            shown.widthAnchor.constraint(equalToConstant: 70).isActive = true
            readout = shown
            on(made) { [weak self] sender in
                let value = (sender as? NSSlider)?.doubleValue ?? param.value
                set(value)
                if let read = alpaca_player_effect_readout(self?.player, Int32(index), Int32(place), Float(value)) {
                    shown.stringValue = String(cString: read)
                    alpaca_string_free(read)
                }
                // kept when the drag ends, not at every step of it
                if NSApp.currentEvent?.type == .leftMouseUp { self?.save() }
            }
            made.widthAnchor.constraint(greaterThanOrEqualToConstant: 200).isActive = true
            control = made
        }
        let row = NSStackView(views: [name, control] + (readout.map { [$0] } ?? []))
        row.spacing = 10
        return row
    }

    // MARK: the listener's own plug-ins

    /// Lets the listener pick a plug-in file; it is taken when it loads, and refused with the
    /// reason when it does not. The library keeps a copy of it in the app's folder.
    func addPlugin() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "lua")].compactMap { $0 }
        guard panel.runModal() == .OK, let file = panel.url else { return }
        var error: UnsafeMutablePointer<CChar>?
        guard alpaca_app_add_plugin(app, file.path, &error) >= 0 else {
            let alert = NSAlert()
            alert.messageText = "\(file.lastPathComponent) is not a plug-in Alpaca can run"
            alert.informativeText = error.map { String(cString: $0) } ?? ""
            alpaca_string_free(error)
            alert.runModal()
            return
        }
        if window != nil { rebuild() }
    }
}

/// Calls a closure when a control fires.
final class Handler: NSObject {
    private let run: (NSControl) -> Void
    init(_ run: @escaping (NSControl) -> Void) { self.run = run }
    @objc func fire(_ sender: NSControl) { run(sender) }
}

/// A view that counts down from its top, so a list starts at the top of its scroll view.
final class FlippedView: NSView {
    override var isFlipped: Bool { true }
}

extension NSView {
    /// The view as a picture over the window's own background, which the view does not draw.
    func snapshot() -> NSBitmapImageRep? {
        layoutSubtreeIfNeeded()
        guard let rep = bitmapImageRepForCachingDisplay(in: bounds),
            let context = NSGraphicsContext(bitmapImageRep: rep)
        else { return nil }
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = context
        effectiveAppearance.performAsCurrentDrawingAppearance {
            NSColor.windowBackgroundColor.setFill()
            NSRect(origin: .zero, size: bounds.size).fill()
        }
        NSGraphicsContext.restoreGraphicsState()
        cacheDisplay(in: bounds, to: rep)
        return rep
    }
}
