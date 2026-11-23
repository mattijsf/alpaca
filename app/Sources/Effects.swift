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
        let options: [Option]

        /// The value as the listener reads it, by the rules of Andamp's plug-in reference: the
        /// `zero` text at the minimum; a share of one as a whole number of hundredths; else the
        /// value times its scale, with its unit.
        func readout(_ value: Double) -> String {
            if !zero.isEmpty, value <= min { return zero }
            if scale == 1, unit == "pc" || (unit.isEmpty && max <= 1) {
                return String(Int((value * 100).rounded()))
            }
            let places = decimals >= 0 ? decimals : ((max - min) * scale <= 50 ? 1 : 0)
            let symbols = [
                "db": " dB", "hz": " Hz", "khz": " kHz", "ms": " ms", "s": " s", "pc": " %",
                "semitone12TET": " st", "coef": "", "degree": "°", "bar": " bars",
            ]
            return String(format: "%.\(places)f", value * scale) + (symbols[unit] ?? "")
        }
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
/// A plug-in is a Lua file in the format of Andamp on Android. The ones that come with the app
/// are in its resources; the listener's own are kept in Application Support. They are loaded
/// the first time they are needed: when the window opens, or at the start when one was left on.
final class Effects: NSObject, NSWindowDelegate {
    private let player: OpaquePointer
    private let settings: Settings
    private var loaded = false
    private var window: NSWindow?
    private let list = NSStackView()
    /// What the controls call; kept for as long as the controls are.
    private var handlers: [Handler] = []
    /// The readouts beside the sliders, by effect and control.
    private var readouts: [String: NSTextField] = [:]
    /// The ids of the plug-ins the listener added, which can be removed again.
    private var own: Set<String> = []

    init(player: OpaquePointer, settings: Settings) {
        self.player = player
        self.settings = settings
        super.init()
    }

    /// Where the listener's own plug-ins are kept.
    private var folder: URL? {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first?
            .appendingPathComponent("Andamp/Plugins", isDirectory: true)
    }

    /// The rack as it stands.
    var rack: Rack {
        guard let json = andamp_player_effects(player) else { return Rack(effects: []) }
        defer { andamp_string_free(json) }
        let data = Data(String(cString: json).utf8)
        return (try? JSONDecoder().decode(Rack.self, from: data)) ?? Rack(effects: [])
    }

    /// Loads the plug-ins at the start when the listener left one on, so it is heard at once.
    func loadIfLeftOn() {
        if settings.effects.values.contains(where: { $0["on"] as? Bool == true }) { load() }
    }

    /// Loads every plug-in once, and puts each back as the listener left it.
    func load() {
        guard !loaded else { return }
        loaded = true
        let bundled = Bundle.main.url(forResource: "plugins", withExtension: nil)
        for (folder, mine) in [(bundled, false), (folder, true)] {
            guard let folder else { continue }
            let files = (try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil)) ?? []
            for file in files.filter({ $0.pathExtension == "lua" }).sorted(by: { $0.path < $1.path }) {
                guard let source = try? String(contentsOf: file, encoding: .utf8) else { continue }
                var error: UnsafeMutablePointer<CChar>?
                let index = andamp_player_add_effect(player, source, &error)
                if let error {
                    FileHandle.standardError.write(
                        Data("andamp: \(file.lastPathComponent): \(String(cString: error))\n".utf8))
                    andamp_string_free(error)
                }
                if index >= 0, mine { own.insert(rack.effects[Int(index)].id) }
            }
        }
        let saved = settings.effects
        for (index, effect) in rack.effects.enumerated() {
            guard let was = saved[effect.id] else { continue }
            let values = was["values"] as? [String: Double] ?? [:]
            for (place, param) in effect.params.enumerated() {
                if let value = values[param.id] {
                    andamp_player_set_effect_value(player, Int32(index), Int32(place), Float(value))
                }
            }
            andamp_player_set_effect_on(player, Int32(index), was["on"] as? Bool ?? false)
        }
    }

    /// Switches on the effect with `id`, for the command line.
    func switchOn(id: String) {
        load()
        if let index = rack.effects.firstIndex(where: { $0.id == id || $0.name == id }) {
            andamp_player_set_effect_on(player, Int32(index), true)
        } else {
            FileHandle.standardError.write(Data("andamp: no effect \(id)\n".utf8))
        }
    }

    /// Keeps which effects are on and where their controls stand.
    func save() {
        guard loaded else { return }
        var all: [String: [String: Any]] = [:]
        for effect in rack.effects {
            var values: [String: Double] = [:]
            for param in effect.params { values[param.id] = param.value }
            all[effect.id] = ["on": effect.on, "values": values]
        }
        settings.effects = all
    }

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
    func picture() -> NSBitmapImageRep? {
        guard let view = window?.contentView else { return nil }
        view.layoutSubtreeIfNeeded()
        guard let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds),
            let context = NSGraphicsContext(bitmapImageRep: rep)
        else { return nil }
        // the window's own background first: the view draws none
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = context
        view.effectiveAppearance.performAsCurrentDrawingAppearance {
            NSColor.windowBackgroundColor.setFill()
            NSRect(origin: .zero, size: view.bounds.size).fill()
        }
        NSGraphicsContext.restoreGraphicsState()
        view.cacheDisplay(in: view.bounds, to: rep)
        return rep
    }

    /// Builds the window's rows again from the rack.
    private func rebuild() {
        list.arrangedSubviews.forEach { $0.removeFromSuperview() }
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
            andamp_player_set_effect_on(self.player, Int32(index), (sender as? NSButton)?.state == .on)
            self.save()
        }
        // a pull-down with the presets, back to the defaults and, for the listener's own, remove
        let menu = NSPopUpButton(frame: .zero, pullsDown: true)
        menu.addItem(withTitle: "Presets")
        for preset in effect.presets { menu.addItem(withTitle: preset.name) }
        menu.menu?.addItem(.separator())
        menu.addItem(withTitle: "Reset to Defaults")
        if own.contains(effect.id) { menu.addItem(withTitle: "Remove Plug-in") }
        on(menu) { [weak self] sender in
            guard let self, let picked = (sender as? NSPopUpButton)?.indexOfSelectedItem else { return }
            let title = (sender as? NSPopUpButton)?.titleOfSelectedItem
            if title == "Remove Plug-in" {
                self.remove(index, effect)
            } else if title == "Reset to Defaults" {
                andamp_player_apply_preset(self.player, Int32(index), -1)
            } else if picked >= 1, picked <= effect.presets.count {
                andamp_player_apply_preset(self.player, Int32(index), Int32(picked - 1))
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
            andamp_player_set_effect_value(self.player, Int32(index), Int32(place), Float(value))
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
            let shown = text(param.readout(param.value))
            shown.alignment = .right
            shown.widthAnchor.constraint(equalToConstant: 70).isActive = true
            readout = shown
            on(made) { [weak self] sender in
                let value = (sender as? NSSlider)?.doubleValue ?? param.value
                set(value)
                shown.stringValue = param.readout(value)
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
    /// reason when it does not.
    func addPlugin() {
        load()
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "lua")].compactMap { $0 }
        guard panel.runModal() == .OK, let file = panel.url,
            let source = try? String(contentsOf: file, encoding: .utf8)
        else { return }
        var error: UnsafeMutablePointer<CChar>?
        let index = andamp_player_add_effect(player, source, &error)
        guard index >= 0 else {
            let alert = NSAlert()
            alert.messageText = "\(file.lastPathComponent) is not a plug-in Andamp can run"
            alert.informativeText = error.map { String(cString: $0) } ?? ""
            andamp_string_free(error)
            alert.runModal()
            return
        }
        // kept under its id, so adding a newer version takes the older one's place
        let id = rack.effects[Int(index)].id
        own.insert(id)
        if let folder {
            try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            let kept = folder.appendingPathComponent(id + ".lua")
            try? FileManager.default.removeItem(at: kept)
            try? FileManager.default.copyItem(at: file, to: kept)
        }
        save()
        if window != nil { rebuild() }
    }

    private func remove(_ index: Int, _ effect: Rack.Effect) {
        andamp_player_remove_effect(player, Int32(index))
        own.remove(effect.id)
        if let folder { try? FileManager.default.removeItem(at: folder.appendingPathComponent(effect.id + ".lua")) }
    }
}

/// Calls a closure when a control fires.
private final class Handler: NSObject {
    private let run: (NSControl) -> Void
    init(_ run: @escaping (NSControl) -> Void) { self.run = run }
    @objc func fire(_ sender: NSControl) { run(sender) }
}

/// A view that counts down from its top, so a list starts at the top of its scroll view.
private final class FlippedView: NSView {
    override var isFlipped: Bool { true }
}
