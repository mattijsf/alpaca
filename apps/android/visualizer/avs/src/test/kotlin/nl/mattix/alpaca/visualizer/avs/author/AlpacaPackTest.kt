// SPDX-License-Identifier: GPL-3.0-or-later

package nl.mattix.alpaca.visualizer.avs.author

import nl.mattix.alpaca.visualizer.avs.AvsComponent
import nl.mattix.alpaca.visualizer.avs.AvsEngine
import nl.mattix.alpaca.visualizer.avs.AvsParser
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The shipped pack: every file parses, every component is one this build has
 * a renderer for, and the writer's bytes survive a round trip. The scripts
 * compile only where the evaluator exists, so `AlpacaPackRenderTest` covers
 * them on a device.
 */
class AlpacaPackTest {
    private val files = AlpacaPack.files()

    @Test
    fun `the pack ships twelve presets, all avs files`() {
        assertEquals(12, files.size)
        files.keys.forEach { name ->
            // the prefix is the app's old name: the files are kept in the preset library under it
            assertTrue("'$name' carries the pack's own prefix", name.startsWith("andamp - "))
            assertTrue("'$name' is an avs file", name.endsWith(".avs"))
        }
    }

    @Test
    fun `every preset parses back from its own bytes`() {
        files.forEach { (name, bytes) ->
            val preset = AvsParser.parse(bytes)
            assertTrue("$name parses to a preset with components", preset.components.isNotEmpty())
        }
    }

    @Test
    fun `every component in the pack is one this build can run`() {
        files.forEach { (name, bytes) ->
            assertTrue("every component of $name is one this build runs", AvsEngine.canRun(AvsParser.parse(bytes)))
        }
    }

    @Test
    fun `a parsed preset re-writes to the same bytes`() {
        files.forEach { (name, bytes) ->
            assertArrayEquals("$name round-trips to the same bytes", bytes, AvsWriter.write(AvsParser.parse(bytes)))
        }
    }

    @Test
    fun `the pack uses no ape except channel shift`() {
        files.forEach { (name, bytes) ->
            AvsParser.parse(bytes).flatten().forEach { component ->
                val foreign = component is AvsComponent.Ape && component.name != "Channel Shift"
                assertTrue("$name carries no third-party component: ${component.name}", !foreign)
            }
        }
    }
}
