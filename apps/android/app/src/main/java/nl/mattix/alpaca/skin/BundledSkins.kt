// SPDX-License-Identifier: GPL-3.0-or-later

package nl.mattix.alpaca.skin

/** A skin that ships inside the app: an id, a name, the asset holding its bytes, and whether it is a modern skin. */
data class BundledSkin(
    val id: String,
    val name: String,
    val asset: String,
    val modern: Boolean = false,
)

/**
 * The skins Alpaca ships, and the one it falls back to.
 *
 * They are walkit's two example skins, built by `examples/skins/make-skins.py` in the walkit
 * repository and copied here as they are.
 *
 * [BASE] is what a fresh install wears and what a skin missing a sheet borrows from.
 */
object BundledSkins {
    /** The id stored in preferences for [BASE]. */
    const val BASE_ID = "base"

    val BASE = BundledSkin(BASE_ID, "walkit classic", "skins/walkit-classic.wsz")

    /** A modern skin, listed while modern skins are on. */
    val MODERN = BundledSkin("walkit-modern", "walkit modern", "skins/walkit-modern.wal", modern = true)

    val all = listOf(BASE, MODERN)

    /** Ids a phone may have stored as a choice that name no skin the app ships; each reads as [BASE_ID]. */
    private val retired = setOf("andamp-light", "andamp-spot")

    fun of(id: String) = all.firstOrNull { it.id == id }

    /** [id] as it is read back from a stored choice: a skin the app no longer ships gives [BASE_ID]. */
    fun current(id: String): String = if (id in retired) BASE_ID else id
}
