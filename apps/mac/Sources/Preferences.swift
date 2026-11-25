// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit

/// The preferences as the library spells them out: pages of rows, each row of a kind this
/// window knows how to draw. What a row means is the library's.
struct PreferencePages: Decodable {
    struct Option: Decodable {
        let name: String
        let value: String
    }

    struct Row: Decodable {
        let id: String
        /// `toggle`, `choice`, `slider`, `action`, `text` or `heading`.
        let kind: String
        let label: String
        let value: String
        let options: [Option]?
        let min: Double?
        let max: Double?
        let step: Double?
        let unit: String?
    }

    struct Page: Decodable {
        let title: String
        let rows: [Row]
    }

    let pages: [Page]
}

/// The Preferences window: a tab for each page, a line for each row. A change goes to the
/// library, and the window is then drawn again from what the library says, since one change
/// may move another row.
final class Preferences: NSObject, NSWindowDelegate {
    private let app: OpaquePointer
    private var window: NSWindow?
    private let tabs = NSTabView()
    /// The pages' names in a row over them; a click brings its page to the front.
    private let picker = NSSegmentedControl()
    /// What the controls call; kept for as long as the controls are.
    private var handlers: [Handler] = []
    /// Called after the library was told something, for what it asks in answer.
    var onChange: (() -> Void)?

    init(app: OpaquePointer) {
        self.app = app
        super.init()
    }

    var isShown: Bool { window?.isVisible == true }

    private var pages: [PreferencePages.Page] {
        guard let json = andamp_app_preferences(app) else { return [] }
        defer { andamp_string_free(json) }
        let data = Data(String(cString: json).utf8)
        return (try? JSONDecoder().decode(PreferencePages.self, from: data))?.pages ?? []
    }

    func show() {
        if window == nil {
            let made = NSWindow(
                contentRect: NSRect(x: 0, y: 0, width: 560, height: 500),
                styleMask: [.titled, .closable, .miniaturizable], backing: .buffered, defer: false)
            made.title = "Preferences"
            made.isReleasedWhenClosed = false
            made.delegate = self
            tabs.translatesAutoresizingMaskIntoConstraints = false
            tabs.tabViewType = .noTabsNoBorder
            picker.translatesAutoresizingMaskIntoConstraints = false
            picker.trackingMode = .selectOne
            picker.target = self
            picker.action = #selector(pickPage(_:))
            let content = NSView()
            content.addSubview(picker)
            content.addSubview(tabs)
            NSLayoutConstraint.activate([
                picker.topAnchor.constraint(equalTo: content.topAnchor, constant: 14),
                picker.centerXAnchor.constraint(equalTo: content.centerXAnchor),
                tabs.topAnchor.constraint(equalTo: picker.bottomAnchor, constant: 8),
                tabs.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 12),
                tabs.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -12),
                tabs.bottomAnchor.constraint(equalTo: content.bottomAnchor, constant: -12),
            ])
            made.contentView = content
            made.center()
            window = made
        }
        rebuild()
        window?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    /// Draws the window again when something the library says changed: a menu pick may have
    /// turned a switch this window shows.
    func refresh() { if isShown { rebuild() } }

    /// Presses the switch or the button of row `id` as a click does, for `--press-pref`.
    func press(row id: String) {
        func find(_ view: NSView) -> NSButton? {
            if let button = view as? NSButton, button.identifier?.rawValue == id { return button }
            return view.subviews.lazy.compactMap(find).first
        }
        for (index, item) in tabs.tabViewItems.enumerated() {
            guard let button = item.view.flatMap(find) else { continue }
            bring(page: index)
            button.performClick(nil)
            return
        }
    }

    /// Every page as a picture, for `--dump`.
    func pictures() -> [NSBitmapImageRep] {
        tabs.tabViewItems.indices.compactMap { index in
            bring(page: index)
            return window?.contentView?.snapshot()
        }
    }

    /// Builds the tabs again from the library's pages, keeping the one in front.
    private func rebuild() {
        let front = tabs.selectedTabViewItem.map { tabs.indexOfTabViewItem($0) } ?? 0
        for item in tabs.tabViewItems { tabs.removeTabViewItem(item) }
        handlers.removeAll()
        for page in pages {
            let item = NSTabViewItem()
            item.label = page.title
            item.view = view(of: page)
            tabs.addTabViewItem(item)
        }
        picker.segmentCount = tabs.numberOfTabViewItems
        for (index, item) in tabs.tabViewItems.enumerated() {
            picker.setLabel(item.label, forSegment: index)
            picker.setWidth(0, forSegment: index)
        }
        bring(page: tabs.tabViewItems.indices.contains(front) ? front : 0)
    }

    /// Brings page `index` to the front.
    private func bring(page index: Int) {
        guard tabs.tabViewItems.indices.contains(index) else { return }
        tabs.selectTabViewItem(at: index)
        picker.selectedSegment = index
    }

    @objc private func pickPage(_ sender: NSSegmentedControl) { bring(page: sender.selectedSegment) }

    /// One page: its rows from the top, the labels in a column at the left.
    private func view(of page: PreferencePages.Page) -> NSView {
        // two columns from the start: a page may begin with a title across both
        let grid = NSGridView(numberOfColumns: 2, rows: 0)
        grid.translatesAutoresizingMaskIntoConstraints = false
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        for row in page.rows {
            switch row.kind {
            case "heading":
                let title = NSTextField(labelWithString: row.label)
                title.font = .boldSystemFont(ofSize: NSFont.systemFontSize)
                let added = grid.addRow(with: [title, NSGridCell.emptyContentView])
                added.mergeCells(in: NSRange(location: 0, length: 2))
                // a title starts at the left edge, over the labels' column
                added.cell(at: 0).xPlacement = .leading
                added.topPadding = grid.numberOfRows > 1 ? 8 : 0
            case "toggle":
                let box = NSButton(checkboxWithTitle: row.label, target: nil, action: nil)
                box.state = row.value == "1" ? .on : .off
                box.identifier = NSUserInterfaceItemIdentifier(row.id)
                on(box) { [weak self] sender in
                    self?.set(row.id, (sender as? NSButton)?.state == .on ? "1" : "0")
                }
                grid.addRow(with: [NSGridCell.emptyContentView, box])
            case "choice":
                let options = row.options ?? []
                let popup = NSPopUpButton(frame: .zero, pullsDown: false)
                for option in options { popup.addItem(withTitle: option.name) }
                popup.selectItem(at: options.firstIndex { $0.value == row.value } ?? -1)
                on(popup) { [weak self] sender in
                    guard let index = (sender as? NSPopUpButton)?.indexOfSelectedItem,
                        options.indices.contains(index)
                    else { return }
                    self?.set(row.id, options[index].value)
                }
                grid.addRow(with: [label(row.label), popup])
            case "slider":
                grid.addRow(with: [label(row.label), slider(row)])
            case "action":
                let button = NSButton(title: row.label, target: nil, action: nil)
                button.identifier = NSUserInterfaceItemIdentifier(row.id)
                on(button) { [weak self] _ in self?.set(row.id, "") }
                grid.addRow(with: [NSGridCell.emptyContentView, button])
            case "text":
                let value = NSTextField(labelWithString: row.value)
                value.isSelectable = true
                value.lineBreakMode = .byTruncatingMiddle
                value.textColor = .secondaryLabelColor
                value.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
                grid.addRow(with: [label(row.label), value])
            default: break
            }
        }
        grid.column(at: 0).xPlacement = .trailing
        let page = NSView()
        page.addSubview(grid)
        NSLayoutConstraint.activate([
            grid.topAnchor.constraint(equalTo: page.topAnchor, constant: 16),
            grid.leadingAnchor.constraint(equalTo: page.leadingAnchor, constant: 20),
            grid.trailingAnchor.constraint(lessThanOrEqualTo: page.trailingAnchor, constant: -20),
        ])
        return page
    }

    private func label(_ text: String) -> NSTextField {
        let made = NSTextField(labelWithString: text.isEmpty ? "" : text + ":")
        made.alignment = .right
        return made
    }

    /// A slider that steps by the row's step, with what it stands at beside it. The library
    /// hears of it when the mouse lets go.
    private func slider(_ row: PreferencePages.Row) -> NSView {
        let (low, high, step) = (row.min ?? 0, row.max ?? 100, max(row.step ?? 1, 1))
        let unit = row.unit ?? ""
        let made = NSSlider(value: Double(row.value) ?? low, minValue: low, maxValue: high, target: nil, action: nil)
        made.numberOfTickMarks = Int(((high - low) / step).rounded()) + 1
        made.allowsTickMarkValuesOnly = true
        made.isContinuous = false
        made.widthAnchor.constraint(equalToConstant: 220).isActive = true
        let readout = NSTextField(labelWithString: "\(Int(made.doubleValue)) \(unit)")
        readout.widthAnchor.constraint(equalToConstant: 56).isActive = true
        on(made) { [weak self] sender in
            guard let value = (sender as? NSSlider)?.doubleValue else { return }
            self?.set(row.id, String(Int(value.rounded())))
        }
        let line = NSStackView(views: [made, readout])
        line.spacing = 8
        return line
    }

    private func on(_ control: NSControl, _ run: @escaping (NSControl) -> Void) {
        let handler = Handler(run)
        handlers.append(handler)
        control.target = handler
        control.action = #selector(Handler.fire(_:))
    }

    /// Tells the library, lets the app answer what it asks, and draws the window again: after
    /// this turn of the run loop, since the control that fired is one of those drawn again.
    private func set(_ id: String, _ value: String) {
        andamp_app_set_preference(app, id, value)
        onChange?()
        DispatchQueue.main.async { [weak self] in self?.refresh() }
    }

    func windowWillClose(_ notification: Notification) { andamp_app_save(app) }
}
