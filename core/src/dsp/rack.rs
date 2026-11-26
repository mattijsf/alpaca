// SPDX-License-Identifier: GPL-3.0-or-later

//! The effect rack: the plug-ins the listener has, each on or off with its controls where
//! they were left, and the chain of engines the audio passes through.
//!
//! The [`Rack`] lives on the main thread with the player. The engines run on the audio
//! thread, in a [`RackVoice`]; what the listener changes crosses over as commands in a
//! ring, and a chain the audio thread is done with comes back the same way, so nothing is
//! allocated or freed there. Switching an effect fades it in or out over the dry signal, so a
//! switch never clicks.

use super::graph::Engine;
use super::lua::{Kind, Plugin};

/// How long an effect takes to fade in or out when it is switched, in milliseconds.
const FADE_MS: f32 = 20.0;
/// How many changes may wait for the audio thread.
const COMMANDS: usize = 1024;
/// Samples run through an effect at once, left and right in turn: 256 frames.
const PART: usize = 512;

/// One effect in the rack.
#[derive(Debug, Clone)]
pub struct Effect {
    /// The plug-in it was loaded from.
    pub plugin: Plugin,
    /// Whether the audio passes through it.
    pub on: bool,
    /// Where each control stands, in the plug-in's own units.
    pub values: Vec<f32>,
    /// Why the effect is left out of the stream playing, when it could not be built for it.
    pub error: String,
}

/// What crosses from the rack to the audio thread.
enum Command {
    /// A new chain, in place of the one running.
    Chain(Box<Chain>),
    /// A control moved.
    Value {
        effect: usize,
        index: usize,
        value: f32,
    },
    /// The controls of an effect jump to where they are going: a preset was applied.
    Settle { effect: usize },
    /// An effect was switched.
    On { effect: usize, on: bool },
}

/// One effect as the audio thread runs it.
struct Stage {
    /// Which effect of the rack it is.
    effect: usize,
    engine: Engine,
    on: bool,
    /// How much of the effect is heard now, 0..1.
    mix: f32,
}

/// The engines of the effects that could be built for the stream, in the rack's order.
pub struct Chain {
    stages: Vec<Stage>,
    /// How far an effect's mix moves in one frame while it fades.
    fade: f32,
}

/// The rack's end on the audio thread.
pub struct RackVoice {
    commands: rtrb::Consumer<Command>,
    done: rtrb::Producer<Box<Chain>>,
    chain: Option<Box<Chain>>,
}

impl RackVoice {
    /// A voice with nothing in it and nobody to hear from.
    pub fn empty() -> RackVoice {
        let (_, commands) = rtrb::RingBuffer::new(1);
        let (done, _) = rtrb::RingBuffer::new(1);
        RackVoice {
            commands,
            done,
            chain: None,
        }
    }

    /// Runs `samples` (left and right in turn) through the effects that are on.
    pub fn run(&mut self, samples: &mut [f32]) {
        while let Ok(command) = self.commands.pop() {
            match command {
                Command::Chain(chain) => {
                    if let Some(old) = self.chain.replace(chain) {
                        // full only if the rack stopped collecting: then it is freed here
                        let _ = self.done.push(old);
                    }
                }
                Command::Value {
                    effect,
                    index,
                    value,
                } => {
                    if let Some(stage) = self.stage(effect) {
                        stage.engine.set_parameter(index, value);
                    }
                }
                Command::Settle { effect } => {
                    if let Some(stage) = self.stage(effect) {
                        stage.engine.settle();
                    }
                }
                Command::On { effect, on } => {
                    if let Some(stage) = self.stage(effect) {
                        // an effect that comes on from silence starts with nothing remembered
                        if on && stage.mix == 0.0 {
                            stage.engine.clear();
                        }
                        stage.on = on;
                    }
                }
            }
        }
        let Some(chain) = &mut self.chain else {
            return;
        };
        let fade = chain.fade;
        for stage in &mut chain.stages {
            if !stage.on && stage.mix == 0.0 {
                continue;
            }
            let target = if stage.on { 1.0 } else { 0.0 };
            for part in samples.chunks_mut(PART) {
                let even = part.len() / 2 * 2;
                let part = &mut part[..even];
                if stage.mix == 1.0 && target == 1.0 {
                    stage.engine.run(part);
                    continue;
                }
                // fading: what came in is mixed with what the effect made, frame by frame
                let mut dry = [0.0f32; PART];
                dry[..part.len()].copy_from_slice(part);
                stage.engine.run(part);
                for (frame, dry) in part.chunks_exact_mut(2).zip(dry.chunks_exact(2)) {
                    if stage.mix != target {
                        stage.mix = if stage.on {
                            (stage.mix + fade).min(1.0)
                        } else {
                            (stage.mix - fade).max(0.0)
                        };
                    }
                    if stage.mix < 1.0 {
                        frame[0] = dry[0] + (frame[0] - dry[0]) * stage.mix;
                        frame[1] = dry[1] + (frame[1] - dry[1]) * stage.mix;
                    }
                }
            }
        }
    }

    fn stage(&mut self, effect: usize) -> Option<&mut Stage> {
        self.chain
            .as_mut()?
            .stages
            .iter_mut()
            .find(|s| s.effect == effect)
    }
}

/// The rack's line to the audio thread of the output that plays now.
struct Link {
    commands: rtrb::Producer<Command>,
    done: rtrb::Consumer<Box<Chain>>,
    sample_rate: u32,
}

/// The effects the listener has.
#[derive(Default)]
pub struct Rack {
    effects: Vec<Effect>,
    link: Option<Link>,
    /// Set when a change did not fit in the ring: the audio thread's chain is behind, and a
    /// new one is built as soon as there is room.
    behind: bool,
}

impl Rack {
    /// The effects, in the order the audio passes through them.
    pub fn effects(&self) -> &[Effect] {
        &self.effects
    }

    /// Adds a plug-in from its source, off and at its defaults; its place in the rack. A
    /// plug-in with the id of one already there takes that one's place, and keeps its settings
    /// where the controls still exist.
    pub fn add(&mut self, source: &str) -> Result<usize, String> {
        let plugin = Plugin::load(source)?;
        let mut effect = Effect {
            values: plugin.params.iter().map(|p| p.default).collect(),
            plugin,
            on: false,
            error: String::new(),
        };
        let at = if let Some(at) = self
            .effects
            .iter()
            .position(|e| e.plugin.info.id == effect.plugin.info.id)
        {
            let old = &self.effects[at];
            effect.on = old.on;
            for (param, value) in effect.plugin.params.iter().zip(&mut effect.values) {
                if let Some(i) = old.plugin.params.iter().position(|p| p.id == param.id) {
                    *value = old.values[i];
                }
            }
            self.effects[at] = effect;
            at
        } else {
            self.effects.push(effect);
            self.effects.len() - 1
        };
        self.rebuild();
        Ok(at)
    }

    /// Adds every `.lua` file of the folder `dir`, by name. Gives each file's name with its
    /// place in the rack, or why it was refused.
    pub fn add_folder(&mut self, dir: &std::path::Path) -> Vec<(String, Result<usize, String>)> {
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .map(|entries| entries.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        files.retain(|path: &std::path::PathBuf| {
            path.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("lua"))
        });
        files.sort();
        files
            .iter()
            .map(|file| {
                let name = file
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let added = std::fs::read_to_string(file)
                    .map_err(|e| e.to_string())
                    .and_then(|source| self.add(&source));
                (name, added)
            })
            .collect()
    }

    /// Which effects are on and where their controls stand, as text for the next run: a line
    /// for each effect, its id, 1 or 0 for on, and its controls as `id=value`, with tabs between.
    pub fn state(&self) -> String {
        self.effects
            .iter()
            .map(|effect| {
                let values: Vec<String> = effect
                    .plugin
                    .params
                    .iter()
                    .zip(&effect.values)
                    .map(|(param, value)| format!("{}={value}", param.id))
                    .collect();
                format!(
                    "{}\t{}\t{}\n",
                    effect.plugin.info.id,
                    u8::from(effect.on),
                    values.join("\t")
                )
            })
            .collect()
    }

    /// Puts back what [`Rack::state`] gave, for the effects that are in the rack: a control
    /// the text does not name stays where it is, and one the plug-in no longer has is ignored.
    pub fn restore(&mut self, state: &str) {
        for line in state.lines() {
            let mut parts = line.split('\t');
            let (Some(id), Some(on)) = (parts.next(), parts.next()) else {
                continue;
            };
            let Some(index) = self.effects.iter().position(|e| e.plugin.info.id == id) else {
                continue;
            };
            for part in parts {
                let Some((param, value)) = part.split_once('=') else {
                    continue;
                };
                let place = self.effects[index]
                    .plugin
                    .params
                    .iter()
                    .position(|p| p.id == param);
                if let (Some(place), Ok(value)) = (place, value.parse::<f32>()) {
                    self.set_value(index, place, value);
                }
            }
            self.send(Command::Settle { effect: index });
            self.set_on(index, on == "1");
        }
    }

    /// Takes effect `index` out of the rack.
    pub fn remove(&mut self, index: usize) {
        if index < self.effects.len() {
            self.effects.remove(index);
            self.rebuild();
        }
    }

    /// Switches effect `index`.
    pub fn set_on(&mut self, index: usize, on: bool) {
        if let Some(effect) = self.effects.get_mut(index) {
            effect.on = on;
            self.send(Command::On { effect: index, on });
        }
    }

    /// Moves control `param` of effect `index` to `value`, held to the control's range; a
    /// choice goes to the nearest of its options.
    pub fn set_value(&mut self, index: usize, param: usize, value: f32) {
        let Some(effect) = self.effects.get_mut(index) else {
            return;
        };
        let Some(spec) = effect.plugin.params.get(param) else {
            return;
        };
        let value = match &spec.kind {
            Kind::Number => value.clamp(spec.min, spec.max),
            Kind::Toggle => f32::from(u8::from(value >= 0.5)),
            Kind::Choice(options) => options
                .iter()
                .map(|o| o.1)
                .min_by(|a, b| (a - value).abs().total_cmp(&(b - value).abs()))
                .unwrap_or(value),
        };
        if !value.is_finite() {
            return;
        }
        effect.values[param] = value;
        self.send(Command::Value {
            effect: index,
            index: param,
            value,
        });
    }

    /// Applies preset `preset` of effect `index`: its values in one step, so nothing sweeps.
    pub fn apply_preset(&mut self, index: usize, preset: usize) {
        let Some(values) = self
            .effects
            .get(index)
            .and_then(|e| e.plugin.presets.get(preset))
            .map(|p| p.values.clone())
        else {
            return;
        };
        for (param, value) in values {
            self.set_value(index, param, value);
        }
        self.send(Command::Settle { effect: index });
    }

    /// Puts every control of effect `index` back at its default, in one step.
    pub fn reset(&mut self, index: usize) {
        let Some(defaults) = self.effects.get(index).map(|e| {
            e.plugin
                .params
                .iter()
                .map(|p| p.default)
                .collect::<Vec<_>>()
        }) else {
            return;
        };
        for (param, value) in defaults.into_iter().enumerate() {
            self.set_value(index, param, value);
        }
        self.send(Command::Settle { effect: index });
    }

    /// Makes the audio thread's end for an output at `sample_rate`, with the rack's effects
    /// built for it. The rack talks to this end from now on.
    pub fn connect(&mut self, sample_rate: u32) -> RackVoice {
        let (commands, incoming) = rtrb::RingBuffer::new(COMMANDS);
        let (done, collected) = rtrb::RingBuffer::new(16);
        self.link = Some(Link {
            commands,
            done: collected,
            sample_rate,
        });
        self.rebuild();
        RackVoice {
            commands: incoming,
            done,
            chain: None,
        }
    }

    /// Frees the chains the audio thread is done with, and catches the audio thread up when
    /// it fell behind.
    pub fn collect(&mut self) {
        if let Some(link) = &mut self.link {
            while link.done.pop().is_ok() {}
            if self.behind && link.commands.slots() > COMMANDS / 2 {
                self.behind = false;
                self.rebuild();
            }
        }
    }

    /// Builds every effect for the stream playing and hands the audio thread the new chain.
    fn rebuild(&mut self) {
        let Some(rate) = self.link.as_ref().map(|l| l.sample_rate) else {
            return;
        };
        let mut stages = Vec::new();
        for (index, effect) in self.effects.iter_mut().enumerate() {
            match effect.plugin.build(rate, 2) {
                Ok(program) => {
                    effect.error.clear();
                    let mut engine = Engine::new(program);
                    for (param, value) in effect.values.iter().enumerate() {
                        engine.set_parameter(param, *value);
                    }
                    engine.settle();
                    stages.push(Stage {
                        effect: index,
                        engine,
                        on: effect.on,
                        mix: if effect.on { 1.0 } else { 0.0 },
                    });
                }
                // left out of this stream; playback goes on
                Err(message) => effect.error = message,
            }
        }
        let fade = 1.0 / (FADE_MS / 1000.0 * rate as f32).max(1.0);
        self.send(Command::Chain(Box::new(Chain { stages, fade })));
    }

    fn send(&mut self, command: Command) {
        if let Some(link) = &mut self.link {
            while link.done.pop().is_ok() {}
            // full only when the output does not run and changes piled up: a chain built
            // later carries where everything stands
            if link.commands.push(command).is_err() {
                self.behind = true;
            }
        }
    }
}
