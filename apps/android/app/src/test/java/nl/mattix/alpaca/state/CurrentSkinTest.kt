// SPDX-License-Identifier: GPL-3.0-or-later

package nl.mattix.alpaca.state

import android.app.Application
import androidx.test.core.app.ApplicationProvider
import nl.mattix.alpaca.skin.BundledSkins
import nl.mattix.alpaca.skin.TestSkins
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * [CurrentSkin]'s fallback order: the stored skin, else the bundled base, and the cache key
 * follows the classic skin drawn.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35])
class CurrentSkinTest {
    private val app = ApplicationProvider.getApplicationContext<Application>()
    private val library = SkinLibrary(app)

    /** The listener's own choice, a stored skin. */
    private lateinit var light: String

    /** A music source's skin, stored too. */
    private lateinit var spot: String

    @Before
    fun clean() {
        app
            .getSharedPreferences("skins", 0)
            .edit()
            .clear()
            .commit()
        java.io.File(app.filesDir, "skins").deleteRecursively()
        light = library.save(TestSkins.classic(app, "Light"), "Light.wsz").id
        spot = library.save(TestSkins.classic(app, "Spot"), "Spot.wsz").id
    }

    /** A preference that says "base" loads the bundled base skin. */
    @Test
    fun `a preference of base loads the base skin`() {
        library.currentId = SkinEntry.BASE_ID

        val skin = CurrentSkin.load(app, library)

        assertNotNull(skin)
        assertEquals(BundledSkins.BASE.name, skin!!.name)
    }

    /** A preference naming a skin that is neither bundled nor stored falls through to the base. */
    @Test
    fun `an unknown skin id falls back to the base`() {
        library.currentId = "a-skin-from-a-build-that-no-longer-exists"

        val skin = CurrentSkin.load(app, library)

        assertNotNull(skin)
        assertEquals(BundledSkins.BASE.name, skin!!.name)
    }

    /** The parsed skin is cached under this key, so the key changes when a source's skin goes on. */
    @Test
    fun `the cache key changes when a source's skin goes on`() {
        library.currentId = light
        val own = CurrentSkin.cacheKey(library)
        assertEquals("Light.wsz", CurrentSkin.cached(app, library)!!.name)

        library.wearing = spot

        assertNotEquals("the key changes when a source's skin goes on", own, CurrentSkin.cacheKey(library))
        assertEquals("Spot.wsz", CurrentSkin.cached(app, library)!!.name)
    }

    @Test
    fun `taking a source's skin off puts the listener's own back`() {
        library.currentId = light
        library.wearing = spot
        assertEquals("Spot.wsz", CurrentSkin.cached(app, library)!!.name)

        library.wearing = null

        assertEquals("Light.wsz", CurrentSkin.cached(app, library)!!.name)
    }

    // modern skins: never parsed here; the last classic choice is drawn for them

    @Test
    fun `a classic id is its own classic skin and a modern one gives the last classic choice`() {
        library.choose(light)
        val modern = library.addModern(app, "Bento")

        assertEquals(spot, CurrentSkin.classicOf(library, spot))
        assertEquals(light, CurrentSkin.classicOf(library, modern))
    }

    /** The modern file is not a classic skin; had it reached the loader, the base would show. */
    @Test
    fun `a worn modern skin loads the last classic choice`() {
        library.choose(light)
        library.choose(library.addModern(app, "Bento"))

        assertEquals("Light.wsz", CurrentSkin.load(app, library)!!.name)
        assertEquals("the widget menu wears it too", "Light.wsz", CurrentSkin.cached(app, library)!!.name)
    }

    @Test
    fun `a modern skin under a base choice loads the base`() {
        library.choose(library.addModern(app, "Bento"))

        assertNull(CurrentSkin.stored(library, library.currentId, fallback = null))
        assertEquals(BundledSkins.BASE.name, CurrentSkin.load(app, library)!!.name)
    }

    @Test
    fun `the key of a worn modern skin carries the classic skin drawn`() {
        library.choose(light)
        library.choose(library.addModern(app, "Bento"))

        assertEquals(light, CurrentSkin.cacheKey(library))
    }

    @Test
    fun `a widget that follows the player wears the player's classic skin`() {
        library.choose(light)
        library.wearing = spot

        assertEquals(
            "a source's skin included",
            "Spot.wsz",
            CurrentSkin.widgetKeyed(app, library)!!.second.name,
        )
    }

    @Test
    fun `a widget following a modern skin wears the last classic choice`() {
        library.choose(light)
        library.choose(library.addModern(app, "Bento"))
        library.chooseWidgetSkin(null)

        assertEquals("Light.wsz", CurrentSkin.widgetKeyed(app, library)!!.second.name)
    }

    @Test
    fun `a widget set to a classic skin wears it whatever a source asks for`() {
        val mine = library.save(TestSkins.classic(app, "Mine"), "Mine.wsz")
        library.choose(light)
        library.chooseWidgetSkin(mine.id)
        library.wearing = spot

        val (key, skin) = CurrentSkin.widgetKeyed(app, library)!!

        assertEquals("Mine.wsz", skin.name)
        assertEquals(mine.id, key)
    }

    @Test
    fun `a widget set to a skin that is not an installed classic one follows the player`() {
        library.choose(light)
        library.chooseWidgetSkin(library.addModern(app, "Bento"))

        assertEquals("Light.wsz", CurrentSkin.widgetKeyed(app, library)!!.second.name)

        library.chooseWidgetSkin("a-skin-from-a-build-that-no-longer-exists")

        assertEquals("Light.wsz", CurrentSkin.widgetKeyed(app, library)!!.second.name)
    }

    /** The widget menu and the widget wear different skins here, and each keeps its own parse. */
    @Test
    fun `the widget and the player's skin are cached apart`() {
        library.choose(light)
        library.chooseWidgetSkin(spot)
        val widget = CurrentSkin.widgetKeyed(app, library)!!.second
        val player = CurrentSkin.cached(app, library)

        assertSame(widget, CurrentSkin.widgetKeyed(app, library)!!.second)
        assertSame(player, CurrentSkin.cached(app, library))
        assertEquals("Spot.wsz", widget.name)
        assertEquals("Light.wsz", player!!.name)
    }
}
