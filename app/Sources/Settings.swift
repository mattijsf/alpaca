// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit

/// What the app keeps between runs: the skin worn, its size, where its windows are, the
/// volume, the playlist and the effects.
///
/// The library keeps them, in one file in the app's folder (`andamp_config_dir`), the same on
/// every system. This is that file's keys with Swift's types.
final class Settings {
    private let store: OpaquePointer?
    /// The app's folder, where the settings and the listener's own plug-ins are.
    let folder: URL?

    /// `kept` false is for a run that must not read or leave anything: `--dump` and `--bench`.
    init(kept: Bool) {
        let dir = andamp_config_dir()
        folder = dir.map { URL(fileURLWithPath: String(cString: $0), isDirectory: true) }
        store = andamp_settings_open(kept ? UnsafePointer(dir) : nil)
        andamp_string_free(dir)
    }

    deinit { andamp_settings_free(store) }

    /// Writes the file when something changed.
    func save() { andamp_settings_save(store) }

    private func text(_ key: String) -> String? {
        guard let value = andamp_settings_get(store, key) else { return nil }
        defer { andamp_string_free(value) }
        return String(cString: value)
    }

    private func set(_ key: String, _ value: String?) { andamp_settings_set(store, key, value) }

    /// A file's path when the file is still there.
    private func file(_ key: String) -> URL? {
        guard let path = text(key), FileManager.default.fileExists(atPath: path) else { return nil }
        return URL(fileURLWithPath: path)
    }

    /// The skin file last worn, when it is still there.
    var skin: URL? {
        get { file("skin") }
        set { set("skin", newValue?.path) }
    }

    var zoom: Int {
        get { max(1, text("zoom").flatMap { Int($0) } ?? 1) }
        set { set("zoom", String(newValue)) }
    }

    var onTop: Bool {
        get { text("ontop") == "1" }
        set { set("ontop", newValue ? "1" : "0") }
    }

    /// How much of the windows shows over what is behind them, 0.2..1.
    var opacity: Double {
        get {
            let kept = text("opacity").flatMap { Double($0) } ?? 1
            return kept >= 0.2 ? min(kept, 1) : 1
        }
        set { set("opacity", String(newValue)) }
    }

    /// The volume, 0..255; nil before the first run.
    var volume: Int? {
        get { text("volume").flatMap { Int($0) } }
        set { set("volume", newValue.map { String($0) }) }
    }

    /// The playlist's files, those that are still there, with its radio streams, and which
    /// one was current.
    var songs: [URL] {
        get {
            (text("songs") ?? "").split(separator: "\n").compactMap { location in
                if location.contains("://") { return URL(string: String(location)) }
                let path = String(location)
                return FileManager.default.fileExists(atPath: path) ? URL(fileURLWithPath: path) : nil
            }
        }
        set { set("songs", newValue.map(SkinController.location).joined(separator: "\n")) }
    }

    var song: Int {
        get { text("song").flatMap { Int($0) } ?? 0 }
        set { set("song", String(newValue)) }
    }

    /// The player's switches and sliders as the library wrote them down: the equalizer, the
    /// balance, shuffle and repeat.
    var player: String? {
        get { text("player") }
        set { set("player", newValue) }
    }

    /// The effects as the library wrote them down: which are on, and where their controls stand.
    var effects: String? {
        get { text("effects") }
        set { set("effects", newValue) }
    }

    /// The folder of Milkdrop presets the listener picked, when it is still there.
    var presets: URL? {
        get { file("presets") }
        set { set("presets", newValue?.path) }
    }

    /// Where a skin's windows were, by the skin's name: for each window the engine's id and
    /// its top left corner on the screen, in points.
    func places(skin: String) -> [UInt32: NSPoint] {
        let prefix = "places.\(skin)."
        guard let keys = andamp_settings_keys(store, prefix) else { return [:] }
        defer { andamp_string_free(keys) }
        var places: [UInt32: NSPoint] = [:]
        for key in String(cString: keys).split(separator: "\n") {
            let at = (text(String(key)) ?? "").split(separator: ",").compactMap { Double($0) }
            if let id = UInt32(key.dropFirst(prefix.count)), at.count == 2 {
                places[id] = NSPoint(x: at[0], y: at[1])
            }
        }
        return places
    }

    func setPlaces(_ places: [UInt32: NSPoint], skin: String) {
        guard !places.isEmpty else { return }
        andamp_settings_clear(store, "places.\(skin).")
        for (id, at) in places { set("places.\(skin).\(id)", "\(Int(at.x)),\(Int(at.y))") }
    }
}
