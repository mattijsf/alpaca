// SPDX-License-Identifier: Apache-2.0

package nl.mattix.alpaca.core.packapi;

import nl.mattix.alpaca.core.packapi.PackAccount;
import nl.mattix.alpaca.core.packapi.PackState;

/**
 * What the pack says about playback and its account, whenever it changes.
 *
 * Oneway, so a pack is never held up by a player that is busy or gone.
 *
 * The descriptor is the name this interface had before its package was renamed from
 * nl.mattix.andamp. It goes on the wire, so apps built against the old name still bind.
 */
@Descriptor(value="nl.mattix.andamp.core.packapi.IPackListener")
oneway interface IPackListener {
    void onState(in PackState state);

    /**
     * Somebody signed in or out on the pack's own screen. The pack sends this
     * when it happens, so the player does not have to ask account() again.
     */
    void onAccount(in PackAccount account);
}
