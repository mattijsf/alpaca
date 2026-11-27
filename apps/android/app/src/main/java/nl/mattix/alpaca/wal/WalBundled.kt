// SPDX-License-Identifier: GPL-3.0-or-later

package nl.mattix.alpaca.wal

import android.content.Context
import android.os.Build
import nl.mattix.alpaca.skin.BundledSkin
import nl.mattix.alpaca.skin.BundledSkins
import java.io.File

/**
 * The skins that ship with the app as files the engine can open ([WalFiles.open] takes a file).
 *
 * Each is a copy of the asset it ships as, made again when the app is updated. The files live in
 * the app's no-backup folder, and the [KEPT] newest of each skin stay, so a look still showing the
 * one before keeps its file. A file of a skin the app no longer ships is deleted when another is
 * written.
 */
internal object WalBundled {
    private const val DIR = "wal-bundled"

    /** How many files of one skin stay: the one on screen and the one before it. */
    private const val KEPT = 2

    private val lock = Any()

    /** The file of [bundled] for this build of the app, written first when it is not there yet. */
    fun fileOf(
        context: Context,
        bundled: BundledSkin,
    ): File {
        val dir = File(context.noBackupFilesDir, DIR)
        val name = "${bundled.id}-${java.lang.Long.toHexString(installedAt(context))}.${bundled.asset.substringAfterLast('.')}"
        synchronized(lock) {
            val file = File(dir, name)
            if (file.isFile) return file
            dir.mkdirs()
            val bytes = context.assets.open(bundled.asset).use { it.readBytes() }
            val part = File(dir, "$name.part")
            part.writeBytes(bytes)
            check(part.renameTo(file)) { "could not write $file" }
            prune(dir, bundled.id)
            return file
        }
    }

    /** Removes all but the [KEPT] newest files of the skin [id], and every file of a skin not shipped. */
    private fun prune(
        dir: File,
        id: String,
    ) {
        dir
            .listFiles { f -> f.name.startsWith("$id-") }
            ?.sortedByDescending { it.lastModified() }
            ?.drop(KEPT)
            ?.forEach { it.delete() }
        dir
            .listFiles { f -> BundledSkins.all.none { f.name.startsWith("${it.id}-") } }
            ?.forEach { it.delete() }
    }

    /** When this build of the app was installed: a new build may ship other skins. */
    @Suppress("DEPRECATION") // getPackageInfo(String, Int) is the call before Android 13
    private fun installedAt(context: Context): Long {
        val pm = context.packageManager
        val info =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                pm.getPackageInfo(
                    context.packageName,
                    android.content.pm.PackageManager.PackageInfoFlags
                        .of(0),
                )
            } else {
                pm.getPackageInfo(context.packageName, 0)
            }
        return info.lastUpdateTime
    }
}
