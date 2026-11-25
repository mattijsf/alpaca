// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import MediaPlayer

/// The Mac's side of the player: the song in Control Center and on the lock screen, and the
/// keyboard's and the headphones' play, pause, next and previous.
final class NowPlaying {
    private let controller: SkinController
    /// What was last told to the system: the entry, the transport, and the place in the song
    /// with the moment it was told.
    private var told: (index: Int32, status: Int32, count: Int32) = (-1, -1, -1)
    private var toldAt: (position: Double, when: Date) = (0, .distantPast)

    init(controller: SkinController) {
        self.controller = controller
        let center = MPRemoteCommandCenter.shared()
        func on(_ command: MPRemoteCommand, _ run: @escaping () -> Void) {
            command.isEnabled = true
            command.addTarget { _ in
                run()
                return .success
            }
        }
        on(center.playCommand) { controller.command(ALPACA_COMMAND_PLAY) }
        on(center.pauseCommand) {
            if alpaca_player_status(controller.player) == 1 { controller.command(ALPACA_COMMAND_PAUSE) }
        }
        on(center.togglePlayPauseCommand) {
            controller.command(
                alpaca_player_status(controller.player) == 0 ? ALPACA_COMMAND_PLAY : ALPACA_COMMAND_PAUSE)
        }
        on(center.stopCommand) { controller.command(ALPACA_COMMAND_STOP) }
        on(center.nextTrackCommand) { controller.command(ALPACA_COMMAND_NEXT) }
        on(center.previousTrackCommand) { controller.command(ALPACA_COMMAND_PREVIOUS) }
        center.changePlaybackPositionCommand.isEnabled = true
        center.changePlaybackPositionCommand.addTarget { event in
            guard let event = event as? MPChangePlaybackPositionCommandEvent else { return .commandFailed }
            controller.seek(toMs: Int32(event.positionTime * 1000))
            return .success
        }
    }

    /// Tells the system what plays when that changed: another song, the transport, or the
    /// place in the song after a seek. Called after every step of the skin, so it reads three
    /// numbers and most often does nothing more.
    func update() {
        let player = controller.player
        let now = (alpaca_player_index(player), alpaca_player_status(player), alpaca_player_count(player))
        let position = Double(alpaca_player_position_ms(player)) / 1000
        // the system counts the seconds on by itself while a song plays
        let expected = toldAt.position + (told.status == 1 ? Date().timeIntervalSince(toldAt.when) : 0)
        guard now != told || abs(position - expected) > 1.5 else { return }
        told = now
        toldAt = (position, Date())
        let center = MPNowPlayingInfoCenter.default()
        var title: UnsafeMutablePointer<CChar>?
        var artist: UnsafeMutablePointer<CChar>?
        var album: UnsafeMutablePointer<CChar>?
        let length = alpaca_player_entry(player, now.0, &title, &artist, &album, nil)
        defer { for text in [title, artist, album] { alpaca_string_free(text) } }
        guard length >= 0, now.1 != 0 else {
            center.nowPlayingInfo = nil
            center.playbackState = .stopped
            return
        }
        var info: [String: Any] = [
            MPMediaItemPropertyTitle: title.map { String(cString: $0) } ?? "",
            MPMediaItemPropertyPlaybackDuration: Double(length) / 1000,
            MPNowPlayingInfoPropertyElapsedPlaybackTime: position,
            MPNowPlayingInfoPropertyPlaybackRate: now.1 == 1 ? 1.0 : 0.0,
        ]
        if let artist, artist.pointee != 0 { info[MPMediaItemPropertyArtist] = String(cString: artist) }
        if let album, album.pointee != 0 { info[MPMediaItemPropertyAlbumTitle] = String(cString: album) }
        center.nowPlayingInfo = info
        center.playbackState = now.1 == 1 ? .playing : .paused
    }
}
