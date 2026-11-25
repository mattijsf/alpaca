// SPDX-License-Identifier: GPL-3.0-or-later

//! The built-in skins on a desktop: which windows show, that only what changed is drawn again,
//! and that a drag moves a window and what is docked to it.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use andamp_core::fonts::Fonts;
use andamp_core::skin::{Skin, DRAWN, MOVED};
use walkit::player::{FakePlayer, Player};

fn resources() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("resources")
}

fn open(name: &str, playing: bool) -> Skin {
    let bytes = std::fs::read(resources().join("skins").join(name)).expect("the built-in skin");
    let fonts = Rc::new(Fonts::new(&resources().join("fonts")));
    Skin::open(
        bytes,
        name,
        None,
        None,
        &fonts,
        None,
        (1728, 1000),
        None,
        |clock| {
            let mut player = FakePlayer::new(clock);
            if playing {
                player.play();
            }
            Box::new(player)
        },
    )
    .expect("the skin opens")
}

#[test]
fn the_classic_skin_shows_three_windows_with_pixels() {
    let mut skin = open("walkit-classic.wsz", false);
    let flags = skin.step();
    assert_eq!(flags & DRAWN, DRAWN);
    let windows = skin.windows();
    assert_eq!(windows.len(), 3, "main, equalizer and playlist");
    let main = &windows[0];
    assert_eq!((main.rect.width, main.rect.height), (275, 116));
    for w in windows {
        assert_eq!(
            w.canvas.pixels.len(),
            (w.rect.width * w.rect.height) as usize
        );
        let opaque = w
            .canvas
            .pixels
            .iter()
            .filter(|p| (*p >> 24) == 0xff)
            .count();
        assert!(
            opaque > w.canvas.pixels.len() / 2,
            "window {} is mostly drawn",
            w.id
        );
    }
}

#[test]
fn andamps_own_skins_open_and_draw() {
    for name in ["AndAmp Dark.wsz", "AndAmp Light.wsz", "AndAmp Spot.wsz"] {
        let mut skin = open(name, false);
        skin.step();
        assert_eq!(
            skin.windows().len(),
            3,
            "{name}: main, equalizer and playlist"
        );
        for w in skin.windows() {
            let drawn = w.canvas.pixels.iter().filter(|p| (*p >> 24) != 0).count();
            assert!(
                drawn > w.canvas.pixels.len() / 2,
                "{name}: window {} is mostly drawn",
                w.id
            );
        }
    }
}

#[test]
fn the_modern_skin_opens_and_draws() {
    let mut skin = open("walkit-modern.wal", false);
    skin.step();
    assert!(!skin.windows().is_empty());
    for w in skin.windows() {
        assert!(w.canvas.pixels.iter().any(|p| (*p >> 24) != 0));
    }
}

#[test]
fn only_the_window_that_changes_is_drawn_again() {
    let mut skin = open("walkit-classic.wsz", true);
    skin.step();
    let before: Vec<(u32, u64)> = skin
        .windows()
        .iter()
        .map(|w| (w.id, w.generation))
        .collect();
    let main = skin.runtime.main_window().expect("a main window").0;
    let mut waits = Vec::new();
    let began = std::time::Instant::now();
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        skin.step();
        waits.push(skin.next_due_ms());
    }
    // the main window's visualizer and time move; the equalizer has nothing that does
    for (w, (id, generation)) in skin.windows().iter().zip(&before) {
        assert_eq!(w.id, *id);
        if w.id == main {
            assert!(
                w.generation > *generation,
                "the main window moves while a song plays"
            );
            // the visualizer steps every 25 ms: no more draws than steps fit in the time
            let steps = began.elapsed().as_millis() as u64 / 25 + 2;
            assert!(
                w.generation - generation <= steps,
                "drawn {} times in {:?}",
                w.generation - generation,
                began.elapsed()
            );
        }
    }
    let still = skin
        .windows()
        .iter()
        .zip(&before)
        .filter(|(w, b)| w.generation == b.1)
        .count();
    assert!(still >= 1, "a window with nothing moving keeps its pixels");
    assert!(
        waits.iter().any(|w| *w > 0),
        "the skin says when it next changes: {waits:?}"
    );
}

#[test]
fn a_drag_on_the_title_bar_moves_the_main_window_and_what_is_docked_to_it() {
    let mut skin = open("walkit-classic.wsz", false);
    skin.move_windows(300, 200);
    skin.step();
    let places = |skin: &Skin| -> Vec<(u32, i32, i32)> {
        let mut v: Vec<_> = skin
            .windows()
            .iter()
            .map(|w| (w.id, w.rect.left, w.rect.top))
            .collect();
        v.sort_unstable();
        v
    };
    let before = places(&skin);
    let generations: Vec<u64> = skin.windows().iter().map(|w| w.generation).collect();
    assert!(
        before.iter().any(|&(_, x, y)| (x, y) == (300, 200)),
        "the main window is where it was put"
    );

    // press on the main window's title bar and pull it 120 across and 80 down, in steps
    skin.runtime.pointer_down(300 + 100, 200 + 6, 1);
    for i in 1..=8 {
        skin.runtime.pointer_move(400 + i * 15, 206 + i * 10);
        skin.step();
    }
    skin.runtime.pointer_up(520, 286);
    let flags = skin.step();
    let _ = flags;
    let after = places(&skin);
    for (b, a) in before.iter().zip(&after) {
        assert_eq!(b.0, a.0);
        assert_eq!(
            (a.1 - b.1, a.2 - b.2),
            (120, 80),
            "window {} moved with the main window",
            b.0
        );
    }
    // a window that only moved is not drawn again, beyond the title bar's change of focus
    let drawn_again: u64 = skin
        .windows()
        .iter()
        .zip(&generations)
        .map(|(w, g)| w.generation - g)
        .sum();
    assert!(
        drawn_again <= 6,
        "moving does not draw every step: {drawn_again}"
    );
    let _ = MOVED;
}

#[test]
fn the_title_bar_buttons_and_the_clutter_bar_ask_the_app() {
    let mut skin = open("walkit-classic.wsz", false);
    skin.step();
    let main = skin.runtime.main_window().expect("a main window");
    let click = |skin: &mut Skin, x: i32, y: i32, touch: i32| {
        skin.runtime.pointer_down_in(main, x, y, touch);
        skin.runtime.pointer_up_in(main, x, y);
        skin.step();
        skin.take_actions()
            .into_iter()
            .map(|a| a.name)
            .collect::<Vec<_>>()
    };
    // Winamp's places: close at 264,3, minimize at 244,3, the D of the clutter bar at 10,49
    assert_eq!(click(&mut skin, 268, 7, 1), ["CLOSE"]);
    assert_eq!(click(&mut skin, 248, 7, 2), ["MINIMIZE"]);
    assert_eq!(click(&mut skin, 13, 52, 3), ["DOUBLESIZE"]);
    assert!(
        skin.take_actions().is_empty(),
        "an action is handed over once"
    );
}

#[test]
fn drawing_only_what_changed_gives_the_pixels_drawing_everything_gives() {
    for name in ["AndAmp Dark.wsz", "walkit-classic.wsz", "walkit-modern.wal"] {
        let mut skin = open(name, true);
        skin.step();
        let (mut parts, mut wholes) = (0, 0);
        for step in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(12));
            // now and then a press on a button, which changes more than the visualizer
            if step == 10 {
                skin.runtime.pointer_down(40, 96, 1);
            }
            if step == 20 {
                skin.runtime.pointer_up(40, 96);
            }
            let before: Vec<u64> = skin.windows().iter().map(|w| w.generation).collect();
            skin.step();
            for (index, window) in skin.windows().iter().enumerate() {
                if before.get(index).is_some_and(|g| *g != window.generation) {
                    let whole = window.changed.width == window.rect.width
                        && window.changed.height == window.rect.height;
                    if whole {
                        wholes += 1;
                    } else {
                        parts += 1;
                    }
                }
                assert!(
                    window.canvas.pixels == skin.drawn_whole(index),
                    "{name}: window {} at step {step} differs from its drawing in full",
                    window.id
                );
            }
        }
        assert!(
            parts > 5,
            "{name}: most drawings are of a part: {parts} parts, {wholes} wholes"
        );
    }
}
