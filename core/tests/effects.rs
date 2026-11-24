// SPDX-License-Identifier: GPL-3.0-or-later

//! Effect plug-ins: the ones that come with the app and Andamp's examples load and build,
//! the ones made to be refused are refused with a reason, and the engine does to a signal
//! what the primitives say.

use std::f32::consts::PI;
use std::path::{Path, PathBuf};

use andamp_core::dsp::graph::Engine;
use andamp_core::dsp::lua::{Kind, Plugin};

fn folder(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
}

fn load(path: &Path) -> Result<Plugin, String> {
    Plugin::load(&std::fs::read_to_string(path).expect("the plug-in's file"))
}

/// A plug-in from source, and its engine for stereo at 44.1 kHz.
fn engine(source: &str) -> Engine {
    let plugin = Plugin::load(source).expect("the plug-in loads");
    Engine::new(plugin.build(44_100, 2).expect("the plug-in builds"))
}

/// Runs `seconds` of a `hz` tone at half scale through `engine` and gives how loud the last
/// half of what comes out of the left side is against what went in, in dB.
fn gain_db(engine: &mut Engine, hz: f32, seconds: f32) -> f32 {
    let frames = (seconds * 44_100.0) as usize;
    let mut out = Vec::with_capacity(frames);
    for i in 0..frames {
        let v = (2.0 * PI * hz * i as f32 / 44_100.0).sin() * 0.5;
        let mut frame = [v, v];
        engine.process(&mut frame);
        out.push(frame[0]);
    }
    let rms = |s: &[f32]| (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt();
    20.0 * (rms(&out[frames / 2..]) / (0.5 / 2f32.sqrt())).log10()
}

#[test]
fn the_plug_ins_that_come_with_the_app_load_and_build() {
    let mut found = 0;
    for dir in [folder("../app/Resources/plugins"), folder("tests/plugins")] {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "lua") {
                let plugin = load(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                assert!(!plugin.info.id.is_empty() && !plugin.info.name.is_empty());
                // mono too, and a rate the load did not try
                for (rate, channels) in [(96_000, 2), (44_100, 1)] {
                    let program = plugin
                        .build(rate, channels)
                        .unwrap_or_else(|e| panic!("{} at {rate} Hz, {channels} channels: {e}", path.display()));
                    assert_eq!(program.channels(), channels);
                    assert_eq!(program.params(), plugin.params.len());
                }
                found += 1;
            }
        }
    }
    assert!(found >= 11, "six that come with the app and five examples: {found}");
}

#[test]
fn what_is_made_to_be_refused_is_refused_with_a_reason() {
    let broken = load(&folder("tests/plugins/refused/broken.lua")).unwrap_err();
    assert!(broken.contains("plugin"), "{broken}");
    let began = std::time::Instant::now();
    let runaway = load(&folder("tests/plugins/refused/runaway.lua")).unwrap_err();
    assert!(runaway.contains("too long"), "{runaway}");
    assert!(began.elapsed().as_secs() < 10, "the loader gives up on a script that never ends");

    let head = r#"plugin { id = "t.t", name = "T", version = "1" }"#;
    let refuse = |body: &str| Plugin::load(&format!("{head}\n{body}")).unwrap_err();
    let two = "function build(g, ctx) local o = {} for ch = 0, ctx.channels - 1 do o[ch] = BODY end return o end";
    let with = |node: &str| refuse(&two.replace("BODY", node));
    assert!(with("g.reverb { input = g.input(ch) }").contains("not a primitive"));
    assert!(with("g.biquad { input = g.input(ch), kind = \"lowpass\", freq = 100 }").contains("\"q\" is missing"));
    assert!(with("g.biquad { input = g.input(ch), kind = \"brickwall\", freq = 100, q = 1 }").contains("brickwall"));
    assert!(with("g.gain { input = g.input(ch), db = 0, colour = 3 }").contains("unknown option \"colour\""));
    assert!(with("g.input(7)").contains("channel 7"));
    assert!(refuse("function build(g, ctx) return { [0] = g.input(0) } end").contains("channel 1"));
    assert!(refuse("io.open('/etc/passwd')\nfunction build(g, ctx) end").contains("io"));
    assert!(refuse("local p = param.number { id = \"p\", min = 0, max = 1 }\nfunction build() end").contains("no default"));
    assert!(refuse("local p = param.number { id = \"p\", default = 0, unit = \"Hz\" }\nfunction build() end").contains("not a unit"));
    // a loop with nothing in it to wait a frame
    let cycle = refuse(
        "function build(g, ctx) local o = {} for ch = 0, 1 do \
         g.tapIn('a' .. ch, g.input(ch)) local x = g.tapOut('b' .. ch) o[ch] = x end return o end",
    );
    assert!(cycle.contains("tapIn") && cycle.contains("tapOut"), "{cycle}");
}

#[test]
fn a_control_reaches_the_graph_smoothed_and_in_its_own_units() {
    let plugin = load(&folder("tests/plugins/gain.lua")).unwrap();
    assert_eq!(plugin.params.len(), 1);
    let level = &plugin.params[0];
    assert_eq!((level.id.as_str(), level.min, level.max, level.default), ("level", 0.0, 2.0, 1.0));
    assert_eq!(level.kind, Kind::Number);
    let mut engine = Engine::new(plugin.build(44_100, 2).unwrap());
    assert!(gain_db(&mut engine, 440.0, 0.2).abs() < 0.01, "at its default the gain leaves the signal alone");
    engine.set_parameter(0, 2.0);
    // 20 ms of smoothing: the first frames are not at the new level yet
    let mut frame = [0.5, 0.5];
    engine.process(&mut frame);
    assert!(frame[0] < 0.9, "the change is smoothed: {}", frame[0]);
    assert!((gain_db(&mut engine, 440.0, 0.5) - 6.02).abs() < 0.05, "twice as loud is 6 dB");
    engine.set_parameter(0, 0.0);
    engine.settle();
    let mut frame = [0.5, 0.5];
    engine.process(&mut frame);
    assert_eq!(frame, [0.0, 0.0], "settled, a parameter is where it is going at once");
}

#[test]
fn filters_delays_and_feedback_do_what_the_primitives_say() {
    let head = r#"plugin { id = "t.t", name = "T", version = "1" }
        function build(g, ctx) local o = {} for ch = 0, ctx.channels - 1 do o[ch] = NODE end return o end"#;
    let make = |node: &str| engine(&head.replace("NODE", node));

    let mut lowpass = make("g.biquad { input = g.input(ch), kind = 'lowpass', freq = 1000, q = 0.707 }");
    assert!(gain_db(&mut lowpass, 100.0, 0.3).abs() < 0.2);
    let high = gain_db(&mut lowpass, 8000.0, 0.3);
    assert!((-40.0..-34.0).contains(&high), "three octaves above a 12 dB an octave filter: {high}");

    let mut shelf = make("g.biquad { input = g.input(ch), kind = 'lowshelf', freq = 200, q = 0.707, gainDb = 6 }");
    assert!((gain_db(&mut shelf, 40.0, 0.5) - 6.0).abs() < 0.3);
    assert!(gain_db(&mut shelf, 5000.0, 0.3).abs() < 0.2);

    assert!((gain_db(&mut make("g.gain { input = g.input(ch), db = -12 }"), 440.0, 0.2) + 12.0).abs() < 0.01);

    // a delay of ten frames: an impulse comes out ten frames later, and nothing before
    let mut delay = make("g.delay { input = g.input(ch), time = 10, maxTime = 64, interp = 'none' }");
    let mut out = Vec::new();
    for i in 0..16 {
        let mut frame = [if i == 0 { 1.0 } else { 0.0 }; 2];
        delay.process(&mut frame);
        out.push(frame[0]);
    }
    assert_eq!(out.iter().position(|v| *v != 0.0), Some(10), "{out:?}");
    assert_eq!(out[10], 1.0);

    // a feedback pair with a gain above one: what goes round is held, and never runs away
    let mut runaway = make("(function() local n = 'f' .. ch local x = g.add(g.input(ch), g.mul(g.tapOut(n), 1.5)) g.tapIn(n, x) return x end)()");
    let mut loudest = 0f32;
    for _ in 0..5000 {
        let mut frame = [0.5, 0.5];
        runaway.process(&mut frame);
        assert!(frame[0].is_finite());
        loudest = loudest.max(frame[0].abs());
    }
    assert!(loudest > 10.0 && loudest <= 0.5 + 16.0 * 1.5, "held at the ceiling: {loudest}");

    // the same noise for the same seed, every run
    let noise = |seed: u32| {
        let mut e = make(&format!("g.noise {{ seed = {seed} }}"));
        (0..8).map(|_| { let mut f = [0.0, 0.0]; e.process(&mut f); f[0] }).collect::<Vec<_>>()
    };
    assert_eq!(noise(3), noise(3));
    assert_ne!(noise(3), noise(4));
    assert!(noise(3).iter().all(|v| (-1.0..1.0).contains(v)));
}

#[test]
fn the_rack_runs_what_is_on_and_fades_what_is_switched() {
    use andamp_core::dsp::rack::Rack;
    let gain = std::fs::read_to_string(folder("tests/plugins/gain.lua")).unwrap();
    let mut rack = Rack::default();
    assert_eq!(rack.add(&gain), Ok(0));
    assert!(rack.add("not lua at all {").is_err());
    assert_eq!(rack.effects().len(), 1);
    rack.set_value(0, 0, 5.0);
    assert_eq!(rack.effects()[0].values, [2.0], "a value is held to its control's range");
    let mut voice = rack.connect(44_100);
    // off: the audio passes untouched
    let mut samples = vec![0.25f32; 2 * 4410];
    voice.run(&mut samples);
    assert!(samples.iter().all(|s| *s == 0.25));
    // on: it fades in over 20 ms, to twice as loud
    rack.set_on(0, true);
    voice.run(&mut samples);
    assert!(samples[0] < 0.26, "the first frame is nearly dry: {}", samples[0]);
    assert!((samples[2 * 4409] - 0.5).abs() < 1e-3, "then the effect is all there: {}", samples[2 * 4409]);
    // a move of the control reaches the engine; off again fades back to dry
    rack.set_value(0, 0, 1.0);
    rack.set_on(0, false);
    let mut samples = vec![0.25f32; 2 * 4410];
    voice.run(&mut samples);
    assert!((samples[2 * 4409] - 0.25).abs() < 1e-6);
    // the same plug-in again takes the first one's place and keeps its settings
    assert_eq!(rack.add(&gain), Ok(0));
    assert_eq!(rack.effects().len(), 1);
    assert_eq!(rack.effects()[0].values, [1.0]);
}

#[test]
fn the_rack_is_left_and_put_back_and_a_control_reads_as_the_reference_says() {
    use andamp_core::dsp::rack::Rack;
    let mut rack = Rack::default();
    let loaded = rack.add_folder(&folder("../app/Resources/plugins"));
    assert_eq!(loaded.len(), 6);
    assert!(loaded.iter().all(|(_, result)| result.is_ok()), "{loaded:?}");
    let tone = rack.effects().iter().position(|e| e.plugin.info.id == "nl.mattix.andamp.tone").unwrap();
    rack.set_on(tone, true);
    rack.set_value(tone, 0, 4.0);
    let left = rack.state();

    let mut next = Rack::default();
    next.add_folder(&folder("../app/Resources/plugins"));
    next.restore(&left);
    next.restore("an.effect.that.is.gone\t1\tx=1\nrubbish");
    let effect = &next.effects()[tone];
    assert!(effect.on);
    assert_eq!(effect.values, [4.0, 0.0]);
    assert_eq!(next.effects().iter().filter(|e| e.on).count(), 1);

    // readouts: decibels with a decimal, a share of one as hundredths, the text for the minimum
    assert_eq!(effect.plugin.params[0].readout(4.0), "4.0 dB");
    let leveler = next.effects().iter().find(|e| e.plugin.info.id == "nl.mattix.andamp.leveler").unwrap();
    assert_eq!(leveler.plugin.params[0].readout(0.5), "50 %");
    assert_eq!(leveler.plugin.params[0].readout(0.0), "OFF");
}
