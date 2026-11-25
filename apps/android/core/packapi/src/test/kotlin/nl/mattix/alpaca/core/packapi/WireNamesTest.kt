// SPDX-License-Identifier: Apache-2.0

package nl.mattix.alpaca.core.packapi

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The names a source app and the player meet under. They are from before the package was
 * renamed from nl.mattix.andamp, and a source built against them still has to bind.
 */
class WireNamesTest {
    @Test
    fun `the interfaces keep their old descriptors`() {
        assertEquals("nl.mattix.andamp.core.packapi.IMusicSourcePack", IMusicSourcePack.DESCRIPTOR)
        assertEquals("nl.mattix.andamp.core.packapi.IPackListener", IPackListener.DESCRIPTOR)
    }

    @Test
    fun `the actions and meta-data keep their old names`() {
        assertEquals("nl.mattix.andamp.source.BIND", PackApi.ACTION_BIND)
        assertEquals("nl.mattix.andamp.source.SETTINGS", PackApi.ACTION_SETTINGS)
        assertEquals("nl.mattix.andamp.source.MARK", PackApi.META_MARK)
    }
}
