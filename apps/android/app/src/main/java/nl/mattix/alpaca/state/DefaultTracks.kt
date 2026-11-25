// SPDX-License-Identifier: GPL-3.0-or-later

package nl.mattix.alpaca.state

import nl.mattix.alpaca.core.model.Track

/**
 * The track a fresh install starts with: the backing track of Winamp's llama intro with a
 * re-recorded vocal, which names the app by its old name, Andamp, as does the file. The
 * entry has no artist, so its display name is the title alone. NOTICE.md records whose
 * music it is.
 */
object DefaultTracks {
    val tracks: List<Track> =
        listOf(
            Track(
                id = "llama",
                artist = "",
                title = "Alpaca Whippin' Intro",
                durationMs = 5329,
                bitrateKbps = 56,
                sampleRateKhz = 22,
                uri = "asset:///audio/andamp-intro.mp3",
            ),
        )
}
