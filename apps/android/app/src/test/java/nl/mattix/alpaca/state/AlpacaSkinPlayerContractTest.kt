// SPDX-License-Identifier: GPL-3.0-or-later

package nl.mattix.alpaca.state

import nl.mattix.alpaca.core.model.Track
import nl.mattix.alpaca.skinplayer.Entry
import nl.mattix.alpaca.skinplayer.SkinPlayer
import nl.mattix.alpaca.skinplayer.SkinPlayerContract
import nl.mattix.alpaca.wal.DeferredControls

/**
 * Alpaca's player meets the contract every skin relies on, over the fake backend the player tests
 * use: a command reaches the state once main has run what was posted and the backend has answered.
 */
class AlpacaSkinPlayerContractTest : SkinPlayerContract() {
    private val posted = ArrayDeque<Runnable>()
    private lateinit var controls: DeferredControls

    override fun player(playlist: List<Entry>): SkinPlayer {
        val state =
            WinampState().apply {
                this.playlist =
                    playlist.mapIndexed { n, entry ->
                        Track(
                            id = "song-$n",
                            artist = "Artist $n",
                            title = entry.title,
                            durationMs = entry.lengthMs,
                            uri = "file:///music/$n.mp3",
                        )
                    }
                currentIndex = 0
            }
        controls = DeferredControls(state)
        return AlpacaSkinPlayer(
            controls = controls,
            winamp = state,
            tapOf = { null },
            positionMs = { controls.reportedMs },
            openFile = {},
            post = { posted += it },
            clock = { controls.now },
        )
    }

    override fun settle(player: SkinPlayer) {
        while (posted.isNotEmpty()) posted.removeFirst().run()
        controls.apply()
    }
}
