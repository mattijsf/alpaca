// SPDX-License-Identifier: GPL-3.0-or-later

package nl.mattix.alpaca.wal

import android.app.Application
import androidx.test.core.app.ApplicationProvider
import nl.mattix.alpaca.skin.BundledSkins
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.File

/**
 * The bundled skins as files for the engine: a copy of the shipped file, written once, and only
 * the newest two of a skin kept.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class WalBundledTest {
    private val app = ApplicationProvider.getApplicationContext<Application>()

    private val dir get() = File(app.noBackupFilesDir, "wal-bundled")

    @Before
    fun clean() {
        dir.deleteRecursively()
    }

    @Test
    fun `a bundled skin is the file it ships, written once`() {
        val file = WalBundled.fileOf(app, BundledSkins.BASE)
        val shipped = app.assets.open(BundledSkins.BASE.asset).use { it.readBytes() }
        assertArrayEquals(shipped, file.readBytes())
        file.setLastModified(0)

        assertEquals(file, WalBundled.fileOf(app, BundledSkins.BASE))
        assertEquals("not written again", 0L, file.lastModified())
    }

    @Test
    fun `the modern skin keeps the extension of its asset`() {
        val file = WalBundled.fileOf(app, BundledSkins.MODERN)

        assertTrue(file.name, file.name.startsWith("${BundledSkins.MODERN.id}-") && file.name.endsWith(".wal"))
        assertArrayEquals(app.assets.open(BundledSkins.MODERN.asset).use { it.readBytes() }, file.readBytes())
    }

    @Test
    fun `only the newest two files of a skin stay, and files of skins no longer shipped go`() {
        dir.mkdirs()
        val oldest = File(dir, "${BundledSkins.BASE_ID}-1.wsz").apply { writeText("") }
        oldest.setLastModified(1_000)
        val before = File(dir, "${BundledSkins.BASE_ID}-2.wsz").apply { writeText("") }
        before.setLastModified(2_000)
        val retired = File(dir, "andamp-light-live-1a2b.wsz").apply { writeText("") }

        val now = WalBundled.fileOf(app, BundledSkins.BASE)

        assertFalse("the oldest goes", oldest.isFile)
        assertTrue("the one before stays for a look still showing it", before.isFile && now.isFile)
        assertFalse("a skin the app no longer ships goes", retired.isFile)
    }
}
