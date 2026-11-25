// SPDX-License-Identifier: GPL-3.0-or-later

package nl.mattix.alpaca.ui.online

import android.content.res.Resources
import nl.mattix.alpaca.R
import nl.mattix.alpaca.state.online.SkinCatalog

/**
 * How many skins there are, to read: the count, or what is loaded with a plus while a
 * search that gives no count still has more to hand out.
 */
internal fun SkinCatalog.countText(resources: Resources): String =
    if (growing) {
        resources.getString(R.string.museum_count_growing, counted.asCount())
    } else {
        total.coerceAtLeast(0).asCount()
    }
