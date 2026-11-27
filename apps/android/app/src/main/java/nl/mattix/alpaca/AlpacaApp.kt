// SPDX-License-Identifier: GPL-3.0-or-later

package nl.mattix.alpaca

import android.app.Application
import nl.mattix.alpaca.backend.media3.Media3PlaybackHost
import nl.mattix.alpaca.state.PackSources
import nl.mattix.alpaca.state.PlaybackRoot
import nl.mattix.alpaca.wal.ClassicBase
import nl.mattix.alpaca.wal.WalApart
import nl.mattix.alpaca.wal.WalFilled
import nl.mattix.alpaca.wal.WalGate
import nl.mattix.alpaca.wal.WalSharp
import kotlin.concurrent.thread

/**
 * Runs before everything else, whatever started the process. Which music sources this phone has is
 * read by the window, the view model and the home screen widget, and the widget can run in a
 * process the launcher started with no activity. So [PackSources] is attached here.
 *
 * A car can start the process too, by connecting to the playback service, and then asks for a
 * session before any window has built a player. So the service is told here how to get one.
 * The engine's classic skins have the base skin under them, from the app's assets
 * ([ClassicBase]).
 *
 * Whether modern skins may be worn ([WalGate]), whether they are drawn with sharp pixels
 * ([WalSharp]), whether their windows are set out apart ([WalApart]) and whether a classic skin
 * fills the screen ([WalFilled]) are read on a background thread, so the first screen does not
 * wait for them.
 */
class AlpacaApp : Application() {
    override fun onCreate() {
        super.onCreate()
        PackSources.attach(this)
        ClassicBase.attach(this)
        Media3PlaybackHost.wake = { PlaybackRoot.backend(it) }
        thread(name = "wal-gate") {
            WalGate.read(this)
            WalSharp.read(this)
            WalApart.read(this)
            WalFilled.read(this)
        }
    }
}
