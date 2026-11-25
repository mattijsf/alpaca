// SPDX-License-Identifier: GPL-3.0-or-later

package nl.mattix.alpaca.state

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.StateFlow
import nl.mattix.alpaca.backend.mock.MockBackend
import nl.mattix.alpaca.core.model.BackendState
import nl.mattix.alpaca.core.model.Capabilities
import nl.mattix.alpaca.core.model.Track
import nl.mattix.alpaca.core.playback.PlaybackBackend

/** A mock that can pan, so tests can watch the balance slider reach the audio. */
class BalanceRecordingBackend(
    tracks: List<Track>,
    scope: CoroutineScope,
    val pans: MutableList<Float> = mutableListOf(),
) : PlaybackBackend by MockBackend(tracks, scope) {
    private val delegate = MockBackend(tracks, scope)

    override val state: StateFlow<BackendState> get() = delegate.state

    override val capabilities: Capabilities get() = delegate.capabilities.copy(hasBalance = true)

    override fun setBalance(balance: Float) {
        pans += balance
    }
}
