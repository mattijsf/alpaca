// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit

/// What the app keeps between runs: the skin worn, its size, where its windows are, the
/// volume and the playlist.
struct Settings {
    /// False for a run that must not read or leave anything: `--dump` and `--bench`.
    let kept: Bool
    private let store = UserDefaults.standard

    private enum Key {
        static let skin = "skin"
        static let zoom = "zoom"
        static let onTop = "onTop"
        static let volume = "volume"
        static let songs = "songs"
        static let song = "song"
        static let places = "places"
        static let effects = "effects"
        static let player = "player"
        static let opacity = "opacity"
    }

    /// How much of the windows shows over what is behind them, 0.2..1.
    var opacity: Double {
        get {
            let kept = kept ? store.double(forKey: Key.opacity) : 0
            return kept >= 0.2 ? min(kept, 1) : 1
        }
        nonmutating set { if kept { store.set(newValue, forKey: Key.opacity) } }
    }

    /// The player's switches and sliders as the library wrote them down: the equalizer, the
    /// balance, shuffle and repeat.
    var player: String? {
        get { kept ? store.string(forKey: Key.player) : nil }
        nonmutating set { if kept { store.set(newValue, forKey: Key.player) } }
    }

    /// The effects as the listener left them, by plug-in id: whether each is on ("on") and
    /// where its controls stand, by their ids ("values").
    var effects: [String: [String: Any]] {
        get { kept ? store.dictionary(forKey: Key.effects) as? [String: [String: Any]] ?? [:] : [:] }
        nonmutating set { if kept { store.set(newValue, forKey: Key.effects) } }
    }

    /// The skin file last worn, when it is still there.
    var skin: URL? {
        get {
            guard kept, let path = store.string(forKey: Key.skin), FileManager.default.fileExists(atPath: path)
            else { return nil }
            return URL(fileURLWithPath: path)
        }
        nonmutating set { if kept { store.set(newValue?.path, forKey: Key.skin) } }
    }

    var zoom: Int {
        get { kept ? max(1, store.integer(forKey: Key.zoom)) : 1 }
        nonmutating set { if kept { store.set(newValue, forKey: Key.zoom) } }
    }

    var onTop: Bool {
        get { kept && store.bool(forKey: Key.onTop) }
        nonmutating set { if kept { store.set(newValue, forKey: Key.onTop) } }
    }

    /// The volume, 0..255; nil before the first run.
    var volume: Int? {
        get { kept ? store.object(forKey: Key.volume) as? Int : nil }
        nonmutating set { if kept { store.set(newValue, forKey: Key.volume) } }
    }

    /// The playlist's files, those that are still there, and which one was current.
    var songs: [URL] {
        get {
            guard kept else { return [] }
            return (store.stringArray(forKey: Key.songs) ?? [])
                .filter { FileManager.default.fileExists(atPath: $0) }
                .map { URL(fileURLWithPath: $0) }
        }
        nonmutating set { if kept { store.set(newValue.map(\.path), forKey: Key.songs) } }
    }

    var song: Int {
        get { kept ? store.integer(forKey: Key.song) : 0 }
        nonmutating set { if kept { store.set(newValue, forKey: Key.song) } }
    }

    /// Where a skin's windows were, by the skin's name: for each window the engine's id and
    /// its top left corner on the screen, in points.
    func places(skin: String) -> [UInt32: NSPoint] {
        guard kept, let all = store.dictionary(forKey: Key.places),
            let saved = all[skin] as? [String: [Double]]
        else { return [:] }
        var places: [UInt32: NSPoint] = [:]
        for (id, at) in saved where at.count == 2 {
            if let id = UInt32(id) { places[id] = NSPoint(x: at[0], y: at[1]) }
        }
        return places
    }

    func setPlaces(_ places: [UInt32: NSPoint], skin: String) {
        guard kept, !places.isEmpty else { return }
        var all = store.dictionary(forKey: Key.places) ?? [:]
        var saved: [String: [Double]] = [:]
        for (id, at) in places { saved[String(id)] = [Double(at.x), Double(at.y)] }
        all[skin] = saved
        store.set(all, forKey: Key.places)
    }
}
