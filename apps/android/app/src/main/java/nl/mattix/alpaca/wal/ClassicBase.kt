// SPDX-License-Identifier: GPL-3.0-or-later

package nl.mattix.alpaca.wal

import android.content.Context
import android.util.Log
import nl.mattix.alpaca.skin.BundledSkins
import nl.mattix.walkit.SkinFiles
import nl.mattix.walkit.ZipSkinFiles

/**
 * What lies under a classic skin the engine opens: the base skin's files, then the default set's.
 * The engine draws a bitmap the skin leaves out from the base skin, as Winamp draws it from its
 * own; the default set behind it keeps the stand-in fonts readable.
 *
 * [attach] gives it the app once, at start. The base skin's file is written and read on the first
 * [files], on the thread that opens a skin. Until then, or when the file cannot be read, a classic
 * skin has the default set alone under it.
 */
internal object ClassicBase {
    private const val TAG = "ClassicBase"

    @Volatile
    private var app: Context? = null

    @Volatile
    private var base: SkinFiles? = null

    /** The base skin's files, read once; null before [attach], and tried again after a failure. */
    private fun baseFiles(): SkinFiles? {
        base?.let { return it }
        val context = app ?: return null
        return synchronized(this) {
            base ?: runCatching { ZipSkinFiles(WalBundled.fileOf(context, BundledSkins.BASE)) }
                .onFailure { Log.w(TAG, "the base skin has no file for the engine", it) }
                .getOrNull()
                ?.also { base = it }
        }
    }

    fun attach(context: Context) {
        app = context.applicationContext
    }

    /** The files under a classic skin. */
    val files: SkinFiles get() = baseFiles()?.let { Over(it, AlpacaDefaultSet) } ?: AlpacaDefaultSet

    /** [top]'s files, then [under]'s for a path [top] does not have. */
    private class Over(
        private val top: SkinFiles,
        private val under: SkinFiles,
    ) : SkinFiles {
        override fun read(path: String): ByteArray? = top.read(path) ?: under.read(path)

        override fun names(): List<String> = (top.names() + under.names()).distinct()
    }
}
