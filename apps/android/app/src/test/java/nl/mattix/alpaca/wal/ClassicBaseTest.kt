// SPDX-License-Identifier: GPL-3.0-or-later

package nl.mattix.alpaca.wal

import android.app.Application
import androidx.compose.ui.graphics.ImageBitmap
import androidx.test.core.app.ApplicationProvider
import nl.mattix.alpaca.skin.BundledSkins
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.ByteArrayOutputStream
import java.io.File
import java.util.zip.ZipEntry
import java.util.zip.ZipInputStream
import java.util.zip.ZipOutputStream

/** The base skin under a classic skin the engine opens, as Winamp lays its own under every skin. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35])
class ClassicBaseTest {
    private val app = ApplicationProvider.getApplicationContext<Application>()

    private fun base(): Map<String, ByteArray> =
        ZipInputStream(app.assets.open(BundledSkins.BASE.asset)).use { zip ->
            generateSequence { zip.nextEntry }.filter { !it.isDirectory }.associate { it.name to zip.readBytes() }
        }

    /** The base skin's archive less [left], written to a file. */
    private fun without(
        name: String,
        vararg left: String,
    ): File {
        val out = ByteArrayOutputStream()
        ZipOutputStream(out).use { zip ->
            base().filterKeys { it !in left }.forEach { (entry, bytes) ->
                zip.putNextEntry(ZipEntry(entry))
                zip.write(bytes)
                zip.closeEntry()
            }
        }
        // not in the cache folder, which an engine host tidies as it starts
        return File(app.filesDir, name).apply { writeBytes(out.toByteArray()) }
    }

    private fun pixels(image: ImageBitmap) = IntArray(image.width * image.height).also { image.readPixels(it) }

    @Test
    fun `a classic skin reads the bitmaps it leaves out from the base skin, and the fonts from the default set`() {
        WalFiles.open(without("no-eq.wsz", "eqmain.bmp"), emptyMap<String, File>()).use { files ->
            assertNull("the skin itself has none", files.read("eqmain.bmp"))
            assertArrayEquals(base().getValue("eqmain.bmp"), files.read("wasabi-base/eqmain.bmp"))
            assertNotNull(files.read("wasabi-base/fonts/LiberationSans.ttf"))
        }
    }

    @Test
    fun `a modern skin keeps the default set alone under it`() {
        val modern = WalBundled.fileOf(app, BundledSkins.MODERN)

        WalFiles.open(modern, emptyMap<String, File>()).use { files ->
            assertNotNull(files.read("wasabi-base/wasabi.xml"))
            assertNull(files.read("wasabi-base/main.bmp"))
        }
    }

    @Test
    fun `the engine draws the buttons a skin leaves out as the base skin draws them`() {
        val still = WalStill.Still(title = "A song", lengthMs = 200_000)

        val expected = drawn(without("whole.wsz"), still)
        val drawn = drawn(without("no-buttons.wsz", "cbuttons.bmp"), still)

        assertNotNull("the whole skin is drawn", expected)
        assertNotNull("the skin without buttons is drawn", drawn)
        assertArrayEquals(expected, drawn)
    }

    /** [skin]'s main window as [still] shows it, in pixels, or null when nothing is drawn. */
    private fun drawn(
        skin: File,
        still: WalStill.Still,
    ): IntArray? {
        val stills = WalStill.of(skin, app.cacheDir)
        return try {
            stills.main(still)?.let(::pixels)
        } finally {
            stills.close()
        }
    }
}
