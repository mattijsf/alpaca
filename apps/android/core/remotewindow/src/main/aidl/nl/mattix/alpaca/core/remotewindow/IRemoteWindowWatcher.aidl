// SPDX-License-Identifier: Apache-2.0

package nl.mattix.alpaca.core.remotewindow;

/**
 * The caller of IRemoteWindow, told which of the player's windows is up on its surface.
 * The caller makes this and hands it to IRemoteWindow.watch.
 *
 * The descriptor is the name this interface had before its package was renamed from
 * nl.mattix.andamp. It goes on the wire, so apps built against the old name still bind.
 */
@Descriptor(value="nl.mattix.andamp.core.remotewindow.IRemoteWindowWatcher")
oneway interface IRemoteWindowWatcher {
    /** Whether the main window is up: false while the equalizer or the list is. */
    void mainUp(boolean up);
}
