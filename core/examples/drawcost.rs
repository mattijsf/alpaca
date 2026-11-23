// SPDX-License-Identifier: GPL-3.0-or-later

//! What a window's draw list holds and what drawing it costs: `cargo run --release --example
//! drawcost -- <skin>`. It prints, for every window, how many commands of each kind there are
//! and how long the whole list takes to draw.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use andamp_core::host::Assets;
use andamp_core::render::{draw_window, Canvas, ShapeCache};
use andamp_core::skin::{read_fonts, Skin};
use walkit::draw::DrawCmd;
use walkit::player::{FakePlayer, Player};

fn main() {
    let skin_path = std::env::args().nth(1).expect("a skin file");
    let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/Resources");
    let bytes = std::fs::read(&skin_path).expect("the skin");
    let name = Path::new(&skin_path).file_name().unwrap().to_string_lossy().into_owned();
    let mut skin = Skin::open(bytes, &name, None, None, read_fonts(&resources.join("fonts")), (1728, 1000), |clock| {
        let mut player = FakePlayer::new(clock);
        player.play();
        Box::new(player)
    })
    .expect("the skin opens");
    skin.step();
    let ids = skin.runtime.drawn_window_ids();
    let drawn = skin.runtime.draw_windows_owned();
    let assets: &mut Assets = &mut skin.assets().borrow_mut();
    for (id, window) in ids.iter().zip(&drawn) {
        let mut kinds: BTreeMap<&str, (usize, i64)> = BTreeMap::new();
        for cmd in window.cmds.iter() {
            let (kind, area) = match cmd {
                DrawCmd::Image { dst, .. } => ("image", i64::from(dst.width) * i64::from(dst.height)),
                DrawCmd::Fill { rect, .. } => ("fill", i64::from(rect.width) * i64::from(rect.height)),
                DrawCmd::Pixels { rect, .. } => ("pixels", i64::from(rect.width) * i64::from(rect.height)),
                DrawCmd::Text(t) => ("text", i64::from(t.rect.width) * i64::from(t.rect.height)),
                DrawCmd::Clip(_) => ("clip", 0),
                DrawCmd::Unclip => ("unclip", 0),
                DrawCmd::WindowStart(_) => ("shape", 0),
                DrawCmd::WindowEnd => ("shape end", 0),
                DrawCmd::BitmapText(_) => ("bitmap text", 0),
            };
            let entry = kinds.entry(kind).or_default();
            entry.0 += 1;
            entry.1 += area;
        }
        let mut canvas = Canvas::new(window.rect.width, window.rect.height);
        let mut shapes = ShapeCache::default();
        let rounds = 2000;
        let began = Instant::now();
        for _ in 0..rounds {
            draw_window(&mut canvas, assets, &mut shapes, &window.cmds);
        }
        let each = began.elapsed().as_secs_f64() / f64::from(rounds) * 1e6;
        println!(
            "window {} {}x{}: {} commands, {:.0} us a draw",
            id.0,
            window.rect.width,
            window.rect.height,
            window.cmds.len(),
            each
        );
        for (kind, (count, area)) in kinds {
            println!("  {kind:12} {count:5}  {area:8} px");
        }
    }
}
