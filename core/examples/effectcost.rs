// SPDX-License-Identifier: GPL-3.0-or-later

//! What each effect plug-in costs: `cargo run --release --example effectcost -- <folder of
//! .lua files>`. It runs ten seconds of stereo noise through each at 44.1 kHz and prints the
//! size of its program and the share of one core it takes while a song plays.

use std::path::Path;
use std::time::Instant;

use alpaca_core::dsp::graph::Engine;
use alpaca_core::dsp::lua::Plugin;

fn main() {
    let folder = std::env::args().nth(1).expect("a folder of plug-ins");
    let mut files: Vec<_> = std::fs::read_dir(Path::new(&folder))
        .expect("the folder")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "lua"))
        .collect();
    files.sort();
    let frames = 441_000;
    // noise at a quarter scale, the same for every plug-in
    let mut seed = 0x1234_5678u32;
    let input: Vec<f32> = (0..frames * 2)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 * 0.5 - 0.25
        })
        .collect();
    for file in files {
        let source = std::fs::read_to_string(&file).expect("the file");
        let began = Instant::now();
        let plugin = match Plugin::load(&source) {
            Ok(plugin) => plugin,
            Err(e) => {
                println!("{}: refused: {e}", file.display());
                continue;
            }
        };
        let load = began.elapsed();
        let mut engine = Engine::new(plugin.build(44_100, 2).expect("it built when it loaded"));
        let mut samples = input.clone();
        let began = Instant::now();
        engine.run(&mut samples);
        let took = began.elapsed().as_secs_f64();
        println!(
            "{:32} {:4} instructions, loads in {:5.1} ms, {:5.2}% of a core ({:.0} times as fast as it plays)",
            plugin.info.name,
            engine.program().len(),
            load.as_secs_f64() * 1000.0,
            took / 10.0 * 100.0,
            10.0 / took
        );
    }
}
