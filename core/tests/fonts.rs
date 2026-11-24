// SPDX-License-Identifier: GPL-3.0-or-later

//! The fonts under every skin: Arial and Tahoma from the system's folders where they are
//! there, the free stand-ins the app brings where they are not.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use andamp_core::fonts::Fonts;
use andamp_core::skin::Skin;
use walkit::files::SkinFiles;
use walkit::player::FakePlayer;

fn own() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/Resources/fonts")
}

/// A folder of its own under the system's temporary folder, emptied first.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("andamp-fonts-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The pixels of every window of the built-in classic skin after its first step.
fn classic_pixels(fonts: Fonts) -> Vec<Vec<u32>> {
    let name = "walkit-classic.wsz";
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../app/Resources/skins")
            .join(name),
    )
    .unwrap();
    let mut skin = Skin::open(
        bytes,
        name,
        None,
        None,
        &Rc::new(fonts),
        None,
        (1728, 1000),
        None,
        |clock| Box::new(FakePlayer::new(clock)),
    )
    .expect("the skin opens");
    skin.step();
    skin.windows()
        .iter()
        .map(|w| w.canvas.pixels.clone())
        .collect()
}

#[test]
fn the_systems_arial_is_drawn_in_place_of_its_stand_in() {
    let system = scratch("system");
    // any other font under Arial's Windows name: the playlist's letters change
    std::fs::copy(
        own().join("DejaVuSansCondensed-Bold.ttf"),
        system.join("Arial.ttf"),
    )
    .unwrap();
    let fonts = Fonts::with_system(&own(), vec![PathBuf::from("/nowhere"), system.clone()]);
    assert_eq!(
        fonts.source("fonts/LiberationSans.ttf"),
        Some(system.join("Arial.ttf"))
    );
    // a font the system lacks is the stand-in
    assert_eq!(
        fonts.source("fonts/DejaVuSansCondensed.ttf"),
        Some(own().join("DejaVuSansCondensed.ttf"))
    );
    assert_eq!(
        fonts.read("fonts/LiberationSans.ttf").as_deref(),
        Some(&std::fs::read(system.join("Arial.ttf")).unwrap()[..])
    );

    let real = classic_pixels(Fonts::with_system(&own(), vec![system.clone()]));
    let stand_ins = classic_pixels(Fonts::stand_ins(&own()));
    assert_ne!(
        real, stand_ins,
        "the playlist is drawn in the system's font"
    );
    let _ = std::fs::remove_dir_all(&system);
}

#[test]
fn only_the_fonts_skins_name_come_from_the_system() {
    let system = scratch("names");
    for name in ["Arial.ttf", "verdana.ttf"] {
        std::fs::copy(own().join("LiberationSans.ttf"), system.join(name)).unwrap();
    }
    let fonts = Fonts::with_system(&own(), vec![system.clone()]);
    assert_eq!(fonts.source("fonts/verdana.ttf"), None);
    assert_eq!(fonts.source("arial.ttf"), None, "only under fonts/");
    assert_eq!(fonts.source("fonts/../arial.ttf"), None);
    // a path is asked for in any case
    assert_eq!(
        fonts.source("Fonts/liberationsans.TTF"),
        Some(system.join("Arial.ttf"))
    );
    let _ = std::fs::remove_dir_all(&system);
}

#[test]
fn without_the_systems_fonts_the_stand_ins_draw() {
    let fonts = Fonts::with_system(&own(), vec![PathBuf::from("/nowhere")]);
    for (path, file) in [
        ("fonts/LiberationSans.ttf", "LiberationSans.ttf"),
        ("fonts/LiberationSans-Bold.ttf", "LiberationSans-Bold.ttf"),
        ("fonts/DejaVuSansCondensed.ttf", "DejaVuSansCondensed.ttf"),
        (
            "fonts/DejaVuSansCondensed-Bold.ttf",
            "DejaVuSansCondensed-Bold.ttf",
        ),
    ] {
        assert_eq!(fonts.source(path), Some(own().join(file)), "{path}");
    }
}

/// The Mac has both fonts, in their bold cuts too, as Monotype made them for Windows.
#[cfg(target_os = "macos")]
#[test]
fn the_mac_gives_arial_and_tahoma() {
    let fonts = Fonts::new(&own());
    let supplemental = Path::new("/System/Library/Fonts/Supplemental");
    for (path, file) in [
        ("fonts/LiberationSans.ttf", "Arial.ttf"),
        ("fonts/LiberationSans-Bold.ttf", "Arial Bold.ttf"),
        ("fonts/DejaVuSansCondensed.ttf", "Tahoma.ttf"),
        ("fonts/DejaVuSansCondensed-Bold.ttf", "Tahoma Bold.ttf"),
    ] {
        assert_eq!(fonts.source(path), Some(supplemental.join(file)), "{path}");
    }
}
