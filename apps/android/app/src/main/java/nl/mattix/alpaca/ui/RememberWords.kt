// SPDX-License-Identifier: GPL-3.0-or-later

package nl.mattix.alpaca.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalContext
import nl.mattix.alpaca.state.Words

/**
 * The app's strings for a composable that hands them to the state layer, which words prompts
 * and summaries itself. A composable that only shows a string uses `stringResource`.
 */
@Composable
fun rememberWords(): Words {
    val context = LocalContext.current
    return remember(context) { Words.of(context) }
}
