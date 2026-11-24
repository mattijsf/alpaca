// SPDX-License-Identifier: GPL-3.0-or-later

//! More than one screen: the windows come back on the screen and at the place they were
//! left, a screen that is gone brings them back into view, and no window is ever put where
//! no screen shows it. The screens are rectangles on the desktop, as a shell reports them.

use std::path::{Path, PathBuf};
use std::time::Duration;

use andamp_core::app::{into_view, seen, App, Area, Options};
use andamp_core::player::Player;

/// The part of the main screen the windows move on: a 2560 by 1440 screen, less a menu bar.
const MAIN: Area = Area {
    x: 0,
    y: 30,
    w: 2560,
    h: 1410,
};
/// A smaller screen left of it, its top a little higher: a laptop's beside a monitor.
const LEFT: Area = Area {
    x: -1312,
    y: 25,
    w: 1312,
    h: 823,
};

fn resources() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/Resources")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("andamp-screens-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The app with its own folder at `config` on `screens`, started as a shell starts it: the
/// screens are said before the start.
fn start(config: &Path, screens: &[Area]) -> App {
    let mut app = App::with(
        Options::default(),
        &resources(),
        (1728, 1000),
        Some(config.to_path_buf()),
        Player::silent(),
    );
    app.set_screens(screens);
    app.start();
    app.step();
    app.step();
    app
}

/// Every window on the desktop, in points, the main one first.
fn windows(app: &mut App) -> Vec<Area> {
    let (zoom, (ox, oy)) = (app.zoom(), app.origin());
    let main = app.main_window();
    let mut all: Vec<(bool, Area)> = app
        .skin()
        .unwrap()
        .windows()
        .iter()
        .map(|w| {
            let r = w.rect;
            let area = Area::new(
                ox + r.left * zoom,
                oy + r.top * zoom,
                r.width * zoom,
                r.height * zoom,
            );
            (Some(w.id) != main, area)
        })
        .collect();
    all.sort_by_key(|(other, area)| (*other, area.y, area.x));
    all.into_iter().map(|(_, area)| area).collect()
}

fn inside(area: Area, screen: Area) -> bool {
    area.x >= screen.x
        && area.y >= screen.y
        && area.x + area.w <= screen.x + screen.w
        && area.y + area.h <= screen.y + screen.h
}

/// Drags the main window by its title bar by `dx`, `dy` points, as the mouse does: down, a
/// moment held, moved and let go; then the steps after.
fn drag_main(app: &mut App, dx: i32, dy: i32) {
    let (zoom, (ox, oy)) = (app.zoom(), app.origin());
    let main = windows(app)[0];
    // on the title bar, in layout pixels on the viewport
    let (x, y) = ((main.x - ox) / zoom + 100, (main.y - oy) / zoom + 6);
    app.pointer_down(x, y, 1);
    std::thread::sleep(Duration::from_millis(100));
    for i in 1..=4 {
        app.pointer_move(x + dx / zoom * i / 4, y + dy / zoom * i / 4);
        app.step();
    }
    app.pointer_up(x + dx / zoom, y + dy / zoom);
    app.step();
    app.step();
}

fn moved(areas: &[Area], dx: i32, dy: i32) -> Vec<Area> {
    areas.iter().map(|a| a.moved(dx, dy)).collect()
}

#[test]
fn a_place_on_a_second_screen_survives_a_restart() {
    let dir = scratch("second");
    let mut app = start(&dir, &[MAIN, LEFT]);
    let first = windows(&mut app);
    assert_eq!(first.len(), 3, "main, equalizer and playlist");
    assert_eq!(
        (first[0].x, first[0].y),
        (MAIN.x + 120, MAIN.y + 80),
        "a first start opens a little in from the main screen's corner"
    );
    assert_eq!(app.origin(), (MAIN.x, MAIN.y));

    // the player dragged onto the screen on the left, its docked windows along
    drag_main(&mut app, -1000, 200);
    let there = windows(&mut app);
    assert_eq!(there, moved(&first, -1000, 200));
    assert!(there.iter().all(|w| inside(*w, LEFT)));
    assert_eq!(
        app.origin(),
        (LEFT.x, LEFT.y),
        "the engine's viewport is the screen the main window is on now"
    );
    // so a window snaps to that screen's edges: 6 points from its left edge, it lands on it
    drag_main(&mut app, LEFT.x + 6 - there[0].x, 0);
    let snapped = windows(&mut app);
    assert_eq!(snapped[0].x, LEFT.x, "snapped to the left screen's edge");
    assert_eq!(snapped, moved(&there, LEFT.x - there[0].x, 0));

    app.save();
    drop(app);
    let mut again = start(&dir, &[MAIN, LEFT]);
    assert_eq!(
        windows(&mut again),
        snapped,
        "back on the left screen, where it was"
    );
    assert_eq!(again.origin(), (LEFT.x, LEFT.y));

    // and at double size the same way
    again.set_zoom(2);
    again.step();
    again.step();
    let double = windows(&mut again);
    assert_eq!((double[0].x, double[0].y), (snapped[0].x, snapped[0].y));
    again.save();
    drop(again);
    let mut third = start(&dir, &[MAIN, LEFT]);
    assert_eq!(third.zoom(), 2);
    assert_eq!(windows(&mut third), double);
}

#[test]
fn a_missing_screen_brings_the_windows_back_into_view() {
    let dir = scratch("missing");
    let mut app = start(&dir, &[MAIN, LEFT]);
    drag_main(&mut app, -1000, 200);
    let there = windows(&mut app);
    // 432 in from the left screen's left edge, 285 down from its top
    let offset = (there[0].x - LEFT.x, there[0].y - LEFT.y);
    assert_eq!(offset, (432, 285));
    app.save();
    drop(app);

    // the next start has the main screen only: the player comes back whole on it, at the
    // place it had on the screen it was on, docked as it was
    let mut alone = start(&dir, &[MAIN]);
    let back = windows(&mut alone);
    assert_eq!(back, moved(&there, MAIN.x - LEFT.x, MAIN.y - LEFT.y));
    assert!(back.iter().all(|w| inside(*w, MAIN)));
    assert_eq!(alone.origin(), (MAIN.x, MAIN.y));
    drop(alone);

    // the same screen put on the other side, right of the main one: the player comes back
    // on it, at its place there
    let right = Area::new(MAIN.x + MAIN.w, 300, LEFT.w, LEFT.h);
    let mut moved_screen = start(&dir, &[MAIN, right]);
    let back = windows(&mut moved_screen);
    assert_eq!(back, moved(&there, right.x - LEFT.x, right.y - LEFT.y));
    assert!(back.iter().all(|w| inside(*w, right)));
    drop(moved_screen);

    // while the app runs: the screen goes, and at the next step the player is on the main
    // screen
    let mut app = start(&dir, &[MAIN, LEFT]);
    assert_eq!(windows(&mut app), there);
    app.set_screens(&[MAIN]);
    app.step();
    let back = windows(&mut app);
    assert_eq!(back, moved(&there, MAIN.x - LEFT.x, MAIN.y - LEFT.y));
    assert_eq!(app.origin(), (MAIN.x, MAIN.y));
    // and a screen that comes back leaves the player where it is now
    app.set_screens(&[MAIN, LEFT]);
    app.step();
    assert_eq!(windows(&mut app), back);
}

#[test]
fn a_window_is_never_put_where_no_screen_shows_it() {
    // kept far off every screen, as by a screen long gone, in places kept before the app
    // knew of screens
    let dir = scratch("never");
    std::fs::write(
        dir.join("settings.txt"),
        "at.AndAmp Dark.wsz.1\t9000,9000\nat.AndAmp Dark.wsz.3\t9000,9116\nat.AndAmp Dark.wsz.5\t-7000,9232\n",
    )
    .unwrap();
    let mut app = start(&dir, &[MAIN, LEFT]);
    let screens = [MAIN, LEFT];
    for window in windows(&mut app) {
        assert!(
            inside(window, MAIN),
            "{window:?} is on the main screen, whole"
        );
    }

    // a drag into the corner no screen shows, under the left screen and left of the main
    // one: let go, the player comes back whole on a screen
    let main = windows(&mut app)[0];
    drag_main(&mut app, -400 - main.x, 1100 - main.y);
    let after = windows(&mut app);
    assert!(
        after.iter().all(|w| screens.iter().any(|s| inside(*w, *s))),
        "{after:?}"
    );

    // whatever the places and the screens, each window comes out seen
    let mut seed = 7u32;
    let mut next = |range: i32| {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (seed >> 8) as i32 % range
    };
    for _ in 0..2000 {
        let screens: Vec<Area> = (0..=next(3))
            .map(|_| {
                Area::new(
                    next(6000) - 3000,
                    next(4000) - 2000,
                    300 + next(2500),
                    200 + next(1500),
                )
            })
            .collect();
        let before: Vec<Area> = (0..next(3))
            .map(|_| Area::new(next(6000) - 3000, next(4000) - 2000, 800, 600))
            .collect();
        let windows: Vec<Area> = (0..=next(4))
            .map(|_| {
                Area::new(
                    next(12000) - 6000,
                    next(8000) - 4000,
                    20 + next(600),
                    10 + next(400),
                )
            })
            .collect();
        let main = next(windows.len() as i32) as usize;
        let placed = into_view(&windows, main, &screens, &before);
        for (window, was) in placed.iter().zip(&windows) {
            assert_eq!((window.w, window.h), (was.w, was.h));
            assert!(
                seen(*window, &screens),
                "{window:?} from {windows:?} on {screens:?}"
            );
        }
    }
}

#[test]
fn places_kept_before_the_screens_were_known_count_from_the_main_screen() {
    // until now places were kept from the top left of the main screen's part the windows
    // move on
    let dir = scratch("old");
    std::fs::write(
        dir.join("settings.txt"),
        "at.AndAmp Dark.wsz.1\t478,294\nat.AndAmp Dark.wsz.3\t478,410\nat.AndAmp Dark.wsz.5\t478,526\nat.AndAmp Dark.wsz.zoom\t1\n",
    )
    .unwrap();
    let mut app = start(&dir, &[MAIN, LEFT]);
    let at = windows(&mut app);
    assert_eq!((at[0].x, at[0].y), (MAIN.x + 478, MAIN.y + 294));
    assert_eq!(app.origin(), (MAIN.x, MAIN.y));
}

#[test]
fn a_window_moved_alone_stays_on_its_own_screen() {
    // the playlist apart from the main window, on the other screen: both stay where they are
    let dir = scratch("apart");
    let mut app = start(&dir, &[MAIN, LEFT]);
    let main = app.main_window().unwrap();
    let (zoom, (ox, oy)) = (app.zoom(), app.origin());
    let playlist = app
        .skin()
        .unwrap()
        .windows()
        .iter()
        .map(|w| w.id)
        .filter(|id| *id != main)
        .max()
        .unwrap();
    let to = (LEFT.x + 100, LEFT.y + 100);
    app.skin()
        .unwrap()
        .place_window(playlist, (to.0 - ox) / zoom, (to.1 - oy) / zoom);
    app.step();
    app.step();
    let places = app.places();
    assert!(places.contains(&(playlist, to)), "{places:?}");
    assert_eq!(
        app.origin(),
        (MAIN.x, MAIN.y),
        "the main window did not move"
    );
    app.save();
    drop(app);
    let again = start(&dir, &[MAIN, LEFT]);
    assert!(again.places().contains(&(playlist, to)));
    drop(again);
    // without the left screen the playlist comes onto the main window's screen, the main
    // window stays
    let mut alone = start(&dir, &[MAIN]);
    let back: Vec<(u32, (i32, i32))> = alone.places();
    let main_at = places.iter().find(|(id, _)| *id == main).unwrap().1;
    assert!(back.contains(&(main, main_at)));
    let list = windows(&mut alone);
    assert!(list.iter().all(|w| inside(*w, MAIN)), "{list:?}");
}

#[test]
fn every_skin_opens_where_the_last_one_was() {
    // the player's place is one for every skin: another skin opens with its main window at
    // the corner of the one before, in this run and the next
    let dir = scratch("one-place");
    let classic = resources().join("skins/walkit-classic.wsz");
    let modern = resources().join("skins/walkit-modern.wal");
    let mut app = start(&dir, &[MAIN, LEFT]);
    drag_main(&mut app, -1000, 200);
    let corner = (windows(&mut app)[0].x, windows(&mut app)[0].y);
    assert!(app.wear(&modern));
    app.step();
    app.step();
    let at = windows(&mut app)[0];
    assert_eq!(
        (at.x, at.y),
        corner,
        "the modern skin opens where the classic one was"
    );
    assert_eq!(app.origin(), (LEFT.x, LEFT.y));
    // moved on, and the classic skin worn again comes there, not where it was itself
    drag_main(&mut app, 300, -100);
    let corner = (windows(&mut app)[0].x, windows(&mut app)[0].y);
    assert!(app.wear(&classic));
    app.step();
    let at = windows(&mut app)[0];
    assert_eq!((at.x, at.y), corner);
    // the next run: the skin worn last, at the same corner, and another skin there too
    app.save();
    drop(app);
    let mut again = start(&dir, &[MAIN, LEFT]);
    assert_eq!(again.skin_name(), "walkit-classic.wsz");
    let at = windows(&mut again)[0];
    assert_eq!((at.x, at.y), corner);
    assert!(again.wear(&modern));
    again.step();
    let at = windows(&mut again)[0];
    assert_eq!((at.x, at.y), corner);
}
