// SPDX-License-Identifier: GPL-3.0-or-later

package nl.mattix.alpaca.skin

import android.content.Context
import java.io.ByteArrayOutputStream
import java.util.zip.ZipEntry
import java.util.zip.ZipInputStream
import java.util.zip.ZipOutputStream

/** Classic skins for tests that need more than the one the app ships. */
object TestSkins {
    /**
     * The base skin's archive with a `readme.txt` that says [tag], so each tag gives other bytes
     * and so another library id. It draws as the base does.
     */
    fun classic(
        context: Context,
        tag: String,
    ): ByteArray {
        val out = ByteArrayOutputStream()
        ZipOutputStream(out).use { zip ->
            ZipInputStream(context.assets.open(BundledSkins.BASE.asset)).use { base ->
                while (true) {
                    val entry = base.nextEntry ?: break
                    zip.putNextEntry(ZipEntry(entry.name))
                    base.copyTo(zip)
                    zip.closeEntry()
                }
            }
            zip.putNextEntry(ZipEntry("readme.txt"))
            zip.write(tag.toByteArray())
            zip.closeEntry()
        }
        return out.toByteArray()
    }
}
