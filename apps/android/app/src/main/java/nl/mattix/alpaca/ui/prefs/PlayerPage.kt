// SPDX-License-Identifier: GPL-3.0-or-later

package nl.mattix.alpaca.ui.prefs

import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource
import nl.mattix.alpaca.R

/** Preferences > Player: the player window's own switches, grouped by what they touch; [onRows] opens Song rows. */
@Composable
internal fun PlayerPage(
    sections: PrefsSections,
    onRows: () -> Unit = {},
) {
    Section(stringResource(R.string.prefs_player_windows_section))
    Group {
        OverlayRow(sections.overlay)
        ShadeRow(sections.shade)
    }
    Section(stringResource(R.string.prefs_player_lists_section))
    Group {
        ListFontRow(sections.lists)
        RowHeightRow(sections.lists)
        RowFormatRow(onRows)
        if (sections.lists.marksOffered) SourceMarksRow(sections.lists)
    }
    Section(stringResource(R.string.prefs_player_touch_section))
    Group { TapAssistRow(sections.tapAssist) }
    ModernSkinsSection(sections.modern)
}
