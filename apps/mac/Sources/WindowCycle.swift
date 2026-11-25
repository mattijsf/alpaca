// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit

/// Command-` through the app's windows, and back with Shift.
///
/// The Mac's own cycling leaves out a window that belongs to another window's group, and the
/// skin's windows are one group around the main one so that Mission Control shows the player
/// as one. So the app goes through its windows itself: the skin's from the top of the screen
/// down, then its other windows in the order they were opened.
enum WindowCycle {
    /// The key left of 1 on a keyboard laid out as in the United States.
    private static let graveKey: UInt16 = 50

    /// Starts answering the key. The answer holds for as long as the app runs.
    static func install() {
        NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
            let held = event.modifierFlags.intersection([.command, .control, .option, .shift])
            guard held == .command || held == [.command, .shift], isCycleKey(event) else { return event }
            go(back: held.contains(.shift))
            return nil
        }
    }

    private static func isCycleKey(_ event: NSEvent) -> Bool {
        let typed = event.charactersIgnoringModifiers ?? ""
        return event.keyCode == graveKey || typed == "`" || typed == "~"
    }

    /// The windows the keys can go to, in the order the cycle takes them.
    private static var windows: [NSWindow] {
        let all = NSApp.windows.filter { $0.isVisible && $0.canBecomeKey && !($0 is NSPanel) }
        let skin = all.filter { $0 is SkinWindow }.sorted {
            ($0.frame.maxY, -$0.frame.minX) > ($1.frame.maxY, -$1.frame.minX)
        }
        let others = all.filter { !($0 is SkinWindow) }.sorted { $0.windowNumber < $1.windowNumber }
        return skin + others
    }

    /// Gives the keys to the window after the one that has them, or the one before.
    static func go(back: Bool) {
        let all = windows
        guard all.count > 1 else { return }
        let step = back ? all.count - 1 : 1
        let from = all.firstIndex { $0 === NSApp.keyWindow } ?? (back ? 0 : all.count - 1)
        let next = all[(from + step) % all.count]
        // the skin's windows keep the order the engine gave them; another window comes forward
        if next is SkinWindow { next.makeKey() } else { next.makeKeyAndOrderFront(nil) }
    }
}
