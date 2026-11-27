// SPDX-License-Identifier: GPL-3.0-or-later

package nl.mattix.alpaca.state

import android.content.Context
import android.util.Log
import nl.mattix.alpaca.skin.BundledSkins
import nl.mattix.alpaca.skin.Skin
import nl.mattix.alpaca.skin.SkinLoader
import nl.mattix.alpaca.wal.WalBundled
import java.io.File

/**
 * The skin the app is wearing, for anything that draws without the player's state.
 *
 * [SkinOps] holds the skin while the app is on screen. A home screen widget is woken by the
 * launcher with no such state, so it loads the same skin from the same library through
 * this object. The order of fallbacks is here once, for both: the stored skin, and the bundled
 * one when it fails or none is stored.
 *
 * Everything here is a classic skin. A modern skin is drawn by its engine only, so where one
 * is worn this draws the listener's last classic choice; see [classicOf].
 */
object CurrentSkin {
    private const val TAG = "CurrentSkin"

    /** The base skin. A user skin that lacks a sheet borrows it from this. */
    fun base(context: Context): Skin = SkinLoader.loadBase(context)

    /**
     * The classic skin drawn for [id]: [id] itself when it is classic, the listener's last
     * classic choice when it is modern.
     */
    fun classicOf(
        library: SkinLibrary,
        id: String,
    ): String = if (library.kindOf(id) == SkinKind.MODERN) library.lastClassic else id

    /**
     * The base skin, or what is worn over it: the listener's own choice, or a music
     * source's skin while one is worn. Null only when the bundled skin does not decode.
     */
    fun load(
        context: Context,
        library: SkinLibrary = SkinLibrary(context),
        /** The worn skin's id; a caller that has already read it passes it on. */
        id: String = library.worn,
    ): Skin? {
        val base = runCatching { base(context) }.getOrNull()
        if (base == null) {
            Log.w(TAG, "the bundled skin would not decode; there is nothing to draw with")
            return null
        }
        return stored(library, id, base) ?: base
    }

    /** One cached skin, with the [cacheKey] it was loaded under. */
    private class Slot {
        @Volatile
        var entry: Pair<String, Skin>? = null
    }

    /** The skin of [keyed], for the player's skin outside the player. */
    private val worn = Slot()

    /** The skin of [widgetKeyed], apart, so that opening the widget menu does not evict the widget's. */
    private val widget = Slot()

    /**
     * What a cached skin depends on: the classic id drawn for the worn id, a source's skin
     * while one is on, the listener's own otherwise.
     */
    fun cacheKey(
        library: SkinLibrary,
        /** The worn skin's id; a caller that has already read it passes it on. */
        id: String = library.worn,
    ): String = classicOf(library, id)

    /**
     * [load], cached. The widget's clock asks for the skin every tick, and parsing a skin
     * is expensive. The cache is keyed by [cacheKey], so another skin or a source's skin
     * going on is picked up on the next draw.
     */
    fun cached(
        context: Context,
        library: SkinLibrary = SkinLibrary(context),
    ): Skin? = keyed(context, library)?.second

    /**
     * [cached], with the [cacheKey] it belongs to. The worn id is read once and both values
     * come from that read, so the skin and the key always match.
     */
    fun keyed(
        context: Context,
        library: SkinLibrary = SkinLibrary(context),
    ): Pair<String, Skin>? = keyedIn(worn, context, library, classicOf(library, library.worn))

    /**
     * [keyed] for the home screen widget, which keeps the key to decide whether to send the
     * whole window again. It wears the skin picked for it when that is an installed classic
     * skin, whatever a source asks for. Otherwise it follows the player: the classic skin
     * drawn for what is worn.
     */
    fun widgetKeyed(
        context: Context,
        library: SkinLibrary = SkinLibrary(context),
    ): Pair<String, Skin>? {
        val id = library.widgetSkin?.takeIf(library::hasClassic) ?: classicOf(library, library.worn)
        return keyedIn(widget, context, library, id)
    }

    /**
     * The file of the skin [widgetKeyed] chooses, as the engine opens it, with the [cacheKey] it
     * goes with: the stored file, or for a skin that ships with the app the one [WalBundled]
     * writes. Null when the skin is gone.
     */
    fun widgetFile(
        context: Context,
        library: SkinLibrary = SkinLibrary(context),
    ): Pair<String, File>? {
        val id = library.widgetSkin?.takeIf(library::hasClassic) ?: classicOf(library, library.worn)
        val file = fileOf(context, id, library) ?: return null
        return cacheKey(library, id) to file
    }

    /**
     * The file of the classic skin drawn for [id] ([classicOf]), as the engine opens it: the stored
     * file, or for a skin that ships with the app the one [WalBundled] writes. Null when the skin
     * is gone. Writes, so it is called off main.
     */
    fun fileOf(
        context: Context,
        id: String,
        library: SkinLibrary = SkinLibrary(context),
    ): File? {
        val drawn = classicOf(library, id)
        return library.fileOf(drawn) ?: BundledSkins.of(drawn)?.let { WalBundled.fileOf(context, it) }
    }

    private fun keyedIn(
        slot: Slot,
        context: Context,
        library: SkinLibrary,
        id: String,
    ): Pair<String, Skin>? {
        val key = cacheKey(library, id)
        slot.entry?.let { if (it.first == key) return it }
        val loaded = load(context, library, id) ?: return null
        return (key to loaded).also { slot.entry = it }
    }

    /**
     * The stored skin [id], or null when there is none or it does not parse, in which case
     * the caller uses the bundled skin. A modern [id] gives the last classic choice, so a
     * modern skin's file never reaches the classic loader.
     */
    @Suppress("TooGenericExceptionCaught") // a corrupt skin file can throw any exception
    fun stored(
        library: SkinLibrary,
        id: String,
        fallback: Skin?,
    ): Skin? {
        val drawn = classicOf(library, id)
        if (drawn == SkinEntry.BASE_ID) return null
        return try {
            library.open(drawn)?.use { SkinLoader.load(it, fallback = fallback, name = library.nameOf(drawn)) }
        } catch (e: Exception) {
            Log.w(TAG, "Stored skin '$drawn' failed to load", e)
            null
        }
    }
}
