// SPDX-License-Identifier: GPL-3.0-or-later

//! An effect as a graph of built-in primitives, and the engine that runs it.
//!
//! This is the effect format of Alpaca on Android (its `docs/dsp-plugin-spec.md`), so that one
//! plug-in file sounds the same on both: the same primitives with the same arithmetic. A
//! [`Graph`] is checked and arranged into a flat [`Program`]; an [`Engine`] runs a program. Nodes
//! that only parameters and constants feed run once every [`CONTROL_PERIOD`] frames; the rest
//! run every frame.
//!
//! Given a buffer, the engine runs each instruction over a block of frames before the next
//! ([`Program::block`]), which saves the dispatch per instruction per frame. That is valid
//! where nothing in the block needs a sample the block has not made yet: a delay line read
//! sooner than the block is long, or a feedback pair, makes the block shorter, down to one
//! frame. The arithmetic is the same either way, so both give the same samples.

use std::collections::HashMap;
use std::f32::consts::LN_2;

/// Frames between control ticks.
pub const CONTROL_PERIOD: i32 = 32;
/// The most frames run through one instruction before the next. A control tick is never
/// inside a block, so the parameters move at the frames they move at a frame at a time.
const BLOCK: usize = CONTROL_PERIOD as usize;
/// How far a value written into a delay line or a feedback pair may go either way.
const LOOP_CEILING: f32 = 16.0;
/// The most nodes a graph may have, and the most delay memory in samples.
const MAX_NODES: usize = 512;
const MAX_DELAY_SAMPLES: f32 = 262_144.0;
/// A parameter this close to where it is going is there.
const CLOSE: f32 = 1e-6;
/// How many words of state a filter keeps: five coefficients, then x1 x2 y1 y2.
const BIQUAD_WORDS: usize = 9;
/// The noise generator: a step modulo 2^24, so its state is a whole number a float holds.
const NOISE_SEEDS: i64 = 1 << 22;
const NOISE_MULTIPLIER: i64 = 0x00EC_E66D;
const NOISE_MASK: i64 = (1 << 24) - 1;
const NOISE_HALF: f32 = (1 << 23) as f32;
/// Below this a value has gone denormal and is zero.
const DENORMAL: f32 = 1e-20;

/// One input to one node: a number, or the output of another node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Edge {
    /// A number.
    Const(f32),
    /// The output of the node with this place in the graph.
    Node(usize),
}

/// A build-time argument: a number or a word.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    /// A number.
    Num(f32),
    /// A word, such as a filter's kind.
    Text(String),
}

/// The built-in primitives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Prim {
    /// A channel of the audio coming in.
    Input,
    /// The value of one of the effect's controls, smoothed.
    Param,
    /// The sum of two inputs.
    Add,
    /// The difference of two inputs.
    Sub,
    /// The product of two inputs.
    Mul,
    /// One input divided by another.
    Div,
    /// The smaller of two inputs.
    Min,
    /// The larger of two inputs.
    Max,
    /// A mix of two inputs, by a third.
    Crossfade,
    /// An input kept between two bounds.
    Clip,
    /// An input with what is not a number, or too small to be one, made zero.
    Sanitise,
    /// A function of one input, by name: `tanh`, `abs`, `exp2` and the like.
    Math,
    /// An input made louder or softer by decibels.
    Gain,
    /// A second-order filter: lowpass, highpass, shelf and the like.
    Biquad,
    /// A first-order filter.
    Onepole,
    /// A first-order all-pass filter.
    Allpass1,
    /// A delay line: its input, a while later.
    Delay,
    /// A delay line read at another time.
    Tap,
    /// A slow oscillator.
    Lfo,
    /// How loud an input is, followed with an attack and a release.
    Envelope,
    /// Noise.
    Noise,
    /// An input bent towards a ceiling.
    Softclip,
    /// The writing end of a feedback pair.
    TapIn,
    /// The reading end of a feedback pair: what its writing end got a frame ago.
    TapOut,
}

const MATH_FNS: [&str; 12] = [
    "exp2", "log2", "sin", "cos", "tan", "tanh", "sqrt", "abs", "recip", "neg", "floor", "round",
];
const BIQUAD_KINDS: [&str; 8] = [
    "lowpass",
    "highpass",
    "bandpass",
    "notch",
    "allpass",
    "peaking",
    "lowshelf",
    "highshelf",
];

impl Prim {
    /// The primitive a plug-in names, as `g.<name>`.
    pub fn named(name: &str) -> Option<Prim> {
        Some(match name {
            "input" => Prim::Input,
            "param" => Prim::Param,
            "add" => Prim::Add,
            "sub" => Prim::Sub,
            "mul" => Prim::Mul,
            "div" => Prim::Div,
            "min" => Prim::Min,
            "max" => Prim::Max,
            "crossfade" => Prim::Crossfade,
            "clip" => Prim::Clip,
            "sanitise" => Prim::Sanitise,
            "math" => Prim::Math,
            "gain" => Prim::Gain,
            "biquad" => Prim::Biquad,
            "onepole" => Prim::Onepole,
            "allpass1" => Prim::Allpass1,
            "delay" => Prim::Delay,
            "tap" => Prim::Tap,
            "lfo" => Prim::Lfo,
            "envelope" => Prim::Envelope,
            "noise" => Prim::Noise,
            "softclip" => Prim::Softclip,
            "tapIn" => Prim::TapIn,
            "tapOut" => Prim::TapOut,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Prim::Input => "input",
            Prim::Param => "param",
            Prim::Add => "add",
            Prim::Sub => "sub",
            Prim::Mul => "mul",
            Prim::Div => "div",
            Prim::Min => "min",
            Prim::Max => "max",
            Prim::Crossfade => "crossfade",
            Prim::Clip => "clip",
            Prim::Sanitise => "sanitise",
            Prim::Math => "math",
            Prim::Gain => "gain",
            Prim::Biquad => "biquad",
            Prim::Onepole => "onepole",
            Prim::Allpass1 => "allpass1",
            Prim::Delay => "delay",
            Prim::Tap => "tap",
            Prim::Lfo => "lfo",
            Prim::Envelope => "envelope",
            Prim::Noise => "noise",
            Prim::Softclip => "softclip",
            Prim::TapIn => "tapIn",
            Prim::TapOut => "tapOut",
        }
    }

    /// The options that take a parameter, a node or a number.
    fn edge_keys(self) -> &'static [&'static str] {
        match self {
            Prim::Add | Prim::Sub | Prim::Mul | Prim::Div | Prim::Min | Prim::Max => &["a", "b"],
            Prim::Crossfade => &["a", "b", "t"],
            Prim::Clip => &["input", "min", "max"],
            Prim::Sanitise | Prim::Math | Prim::Envelope => &["input"],
            Prim::Gain => &["input", "db"],
            Prim::Biquad => &["input", "freq", "q", "gainDb"],
            Prim::Onepole | Prim::Allpass1 => &["input", "coeff"],
            Prim::Delay => &["input", "time"],
            Prim::Tap => &["time"],
            Prim::Lfo => &["rate", "phase"],
            Prim::Softclip => &["input", "drive"],
            Prim::TapIn => &["source"],
            Prim::Input | Prim::Param | Prim::Noise | Prim::TapOut => &[],
        }
    }

    /// The options fixed when the graph is built: a number or a word.
    fn const_keys(self) -> &'static [&'static str] {
        match self {
            Prim::Input => &["ch"],
            Prim::Param => &["index"],
            Prim::Math => &["fn"],
            Prim::Biquad => &["kind"],
            Prim::Allpass1 => &["ceiling"],
            Prim::Delay => &["maxTime", "interp"],
            Prim::Tap => &["line", "interp"],
            Prim::Lfo => &["shape"],
            Prim::Envelope => &["attack", "release", "kind"],
            Prim::Noise => &["seed"],
            Prim::Softclip => &["knee"],
            Prim::TapIn | Prim::TapOut => &["name"],
            _ => &[],
        }
    }

    fn optional(self) -> &'static [&'static str] {
        match self {
            Prim::Biquad => &["gainDb"],
            Prim::Delay | Prim::Tap => &["interp"],
            Prim::Lfo => &["phase"],
            Prim::Envelope => &["kind"],
            Prim::Noise => &["seed"],
            Prim::Softclip => &["knee"],
            _ => &[],
        }
    }

    /// The words option `key` may be, when it is a word from a list.
    fn words(self, key: &str) -> Option<&'static [&'static str]> {
        match (self, key) {
            (Prim::Math, "fn") => Some(&MATH_FNS),
            (Prim::Biquad, "kind") => Some(&BIQUAD_KINDS),
            (Prim::Delay | Prim::Tap, "interp") => Some(&["none", "linear"]),
            (Prim::Lfo, "shape") => Some(&["sine", "triangle"]),
            (Prim::Envelope, "kind") => Some(&["peak", "rms"]),
            _ => None,
        }
    }

    fn is_text(self, key: &str) -> bool {
        self.words(key).is_some() || (matches!(self, Prim::TapIn | Prim::TapOut) && key == "name")
    }

    /// Whether its output depends on its own past, so that it runs every frame.
    fn stateful(self) -> bool {
        matches!(
            self,
            Prim::Biquad
                | Prim::Onepole
                | Prim::Allpass1
                | Prim::Delay
                | Prim::Tap
                | Prim::Lfo
                | Prim::Envelope
                | Prim::Noise
                | Prim::TapIn
                | Prim::TapOut
        )
    }
}

/// One node of a graph.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// What it does.
    pub prim: Prim,
    /// Its inputs, by name.
    pub edges: Vec<(String, Edge)>,
    /// Its build-time arguments, by name.
    pub consts: Vec<(String, Arg)>,
}

impl Node {
    fn edge(&self, key: &str) -> Option<Edge> {
        self.edges.iter().find(|(k, _)| k == key).map(|(_, e)| *e)
    }

    fn number(&self, key: &str) -> Option<f32> {
        self.consts.iter().find_map(|(k, a)| match a {
            Arg::Num(v) if k == key => Some(*v),
            _ => None,
        })
    }

    fn text(&self, key: &str) -> Option<&str> {
        self.consts.iter().find_map(|(k, a)| match a {
            Arg::Text(v) if k == key => Some(v.as_str()),
            _ => None,
        })
    }
}

/// What the engine needs of a parameter: where it starts and how fast it follows a change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamRate {
    /// Where it starts.
    pub default: f32,
    /// Milliseconds of smoothing; 0 jumps.
    pub smooth_ms: f32,
}

/// An effect for one audio format: its nodes, its parameters and the node each channel leaves by.
#[derive(Debug, Clone, PartialEq)]
pub struct Graph {
    /// The nodes; an edge names one by its place here.
    pub nodes: Vec<Node>,
    /// The controls, in the plug-in's order.
    pub params: Vec<ParamRate>,
    /// What each channel leaves by.
    pub outputs: Vec<Edge>,
    /// How many channels the audio has.
    pub channels: usize,
    /// Frames a second.
    pub sample_rate: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
    Crossfade,
    Clip,
    Sanitise,
    Math,
    Gain,
    Onepole,
    Allpass1,
    BiquadCoefficients,
    Biquad,
    LfoSine,
    LfoTriangle,
    Envelope,
    Softclip,
    DelayReadLinear,
    DelayReadNearest,
    DelayWrite,
    TapRead,
    TapWrite,
    Noise,
}

/// One instruction: what it does, the bus slot it writes, up to three slots it reads, where
/// its state or its delay line is, and one more small number (a math function, a filter's
/// kind, an envelope's kind, a noise seed).
#[derive(Debug, Clone, Copy)]
struct Instr {
    op: Op,
    dst: usize,
    a: usize,
    b: usize,
    c: usize,
    state: usize,
    aux: u32,
}

/// A checked graph as the engine runs it: a flat list of instructions over one bus of floats.
#[derive(Debug, Clone)]
pub struct Program {
    instrs: Vec<Instr>,
    /// Instructions before this run once per control tick; the rest run every frame.
    control_end: usize,
    /// The bus as it starts: the input and parameter slots, then the constants.
    constants: Vec<f32>,
    in_slot: Vec<usize>,
    param_slot: Vec<usize>,
    /// The share of the distance left that each parameter moves per control tick.
    param_step: Vec<f32>,
    param_default: Vec<f32>,
    out_slot: Vec<usize>,
    state_size: usize,
    /// Where each delay line starts in the shared memory, and how long it is.
    lines: Vec<(usize, usize)>,
    memory_size: usize,
    sample_rate: u32,
    /// How many frames may run through one instruction before the next.
    block: usize,
}

impl Program {
    /// How many frames a buffer may run through one instruction before the next: up to
    /// 32, fewer when a delay line is read sooner than that, and one with a feedback pair or
    /// a delay whose time is worked out as it plays.
    pub fn block(&self) -> usize {
        self.block
    }

    /// How many parameters the program reads.
    pub fn params(&self) -> usize {
        self.param_slot.len()
    }

    /// How many channels it was built for.
    pub fn channels(&self) -> usize {
        self.in_slot.len()
    }

    /// How many instructions it runs, for a sense of an effect's cost.
    pub fn len(&self) -> usize {
        self.instrs.len()
    }

    /// Whether it runs no instruction at all.
    pub fn is_empty(&self) -> bool {
        self.instrs.is_empty()
    }
}

/// A slot while the program is put together: fixed in the bus already, or the result of an
/// instruction whose place is known only at the end.
#[derive(Debug, Clone, Copy)]
enum Slot {
    Fixed(usize),
    Result(usize),
}

struct Pending {
    op: Op,
    a: Slot,
    b: Slot,
    c: Slot,
    state: usize,
    aux: u32,
    control: bool,
}

/// Checks `graph` and arranges it into a program, or says everything that is wrong with it.
pub fn compile(graph: &Graph) -> Result<Program, Vec<String>> {
    let mut errors = Vec::new();
    let n = graph.nodes.len();
    if n > MAX_NODES {
        errors.push(format!("the graph has {n} nodes; {MAX_NODES} is the most"));
    }
    for (index, node) in graph.nodes.iter().enumerate() {
        check_node(graph, index, node, &mut errors);
    }
    // every feedback name once on each side
    let mut taps: HashMap<&str, (usize, usize)> = HashMap::new();
    for node in &graph.nodes {
        if let (Prim::TapIn | Prim::TapOut, Some(name)) = (node.prim, node.text("name")) {
            let entry = taps.entry(name).or_default();
            if node.prim == Prim::TapIn {
                entry.0 += 1;
            } else {
                entry.1 += 1;
            }
        }
    }
    let mut names: Vec<_> = taps.into_iter().collect();
    names.sort_unstable();
    for (name, (ins, outs)) in names {
        if ins != 1 || outs != 1 {
            errors.push(format!(
                "feedback \"{name}\" has {ins} tapIn and {outs} tapOut; each name needs one of each"
            ));
        }
    }
    let delay: f32 = graph
        .nodes
        .iter()
        .filter(|node| node.prim == Prim::Delay)
        .filter_map(|node| node.number("maxTime"))
        .sum();
    if delay > MAX_DELAY_SAMPLES {
        errors.push(format!(
            "the graph asks for {delay} samples of delay memory; {MAX_DELAY_SAMPLES} is the most"
        ));
    }
    if graph.outputs.len() != graph.channels {
        errors.push(format!(
            "build returned {} channels for a stream of {}",
            graph.outputs.len(),
            graph.channels
        ));
    }
    for (channel, out) in graph.outputs.iter().enumerate() {
        match out {
            Edge::Node(i) if *i >= n => errors.push(format!(
                "channel {channel} leaves by a node that is not there"
            )),
            Edge::Node(i) if graph.nodes[*i].prim == Prim::TapIn => {
                errors.push(format!(
                    "channel {channel} leaves by a tapIn, which gives nothing"
                ));
            }
            _ => {}
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    // what each node needs worked out before it this frame. A delay reads its line and a
    // tapOut its pair's last value, so neither waits for what is written into them: that is
    // what lets a loop go round.
    let needs = |index: usize| -> Vec<usize> {
        let node = &graph.nodes[index];
        node.edges
            .iter()
            .filter(|(key, _)| !(node.prim == Prim::Delay && key == "input"))
            .filter_map(|(_, edge)| match edge {
                Edge::Node(i) => Some(*i),
                Edge::Const(_) => None,
            })
            .collect()
    };
    let mut order = Vec::with_capacity(n);
    let mut placed = vec![false; n];
    // a tapIn only writes: it goes to the end of the frame with the delay lines' writes
    let writers: Vec<usize> = (0..n)
        .filter(|i| graph.nodes[*i].prim == Prim::TapIn)
        .collect();
    for &w in &writers {
        placed[w] = true;
    }
    loop {
        let before = order.len();
        for index in 0..n {
            if !placed[index]
                && needs(index)
                    .iter()
                    .all(|d| placed[*d] && graph.nodes[*d].prim != Prim::TapIn)
            {
                placed[index] = true;
                order.push(index);
            }
        }
        if order.len() == before {
            break;
        }
    }
    for index in 0..n {
        if needs(index)
            .iter()
            .any(|d| graph.nodes[*d].prim == Prim::TapIn)
        {
            errors.push(format!(
                "node {index} reads a tapIn, which gives nothing; read its tapOut"
            ));
        }
    }
    if order.len() + writers.len() < n && errors.is_empty() {
        let stuck: Vec<String> = (0..n)
            .filter(|i| !placed[*i])
            .map(|i| i.to_string())
            .collect();
        errors.push(format!(
            "nodes {} loop back on themselves with no delay line or feedback pair in the loop",
            stuck.join(", ")
        ));
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    // a node fed only by parameters, numbers and other such nodes runs once per control tick
    let mut control = vec![false; n];
    for &index in &order {
        let node = &graph.nodes[index];
        control[index] = match node.prim {
            Prim::Param => true,
            Prim::Input => false,
            prim if prim.stateful() => false,
            _ => node.edges.iter().all(|(_, edge)| match edge {
                Edge::Const(_) => true,
                Edge::Node(i) => control[*i],
            }),
        };
    }
    Ok(lower(graph, &order, &writers, &control))
}

fn check_node(graph: &Graph, index: usize, node: &Node, errors: &mut Vec<String>) {
    let prim = node.prim;
    let at = format!("node {index} ({})", prim.name());
    let known = |key: &str| prim.edge_keys().contains(&key) || prim.const_keys().contains(&key);
    for key in node
        .edges
        .iter()
        .map(|(k, _)| k)
        .chain(node.consts.iter().map(|(k, _)| k))
    {
        if !known(key) {
            errors.push(format!("{at}: unknown option \"{key}\""));
        }
    }
    for key in prim.edge_keys().iter().chain(prim.const_keys()) {
        let given =
            node.edges.iter().any(|(k, _)| k == key) || node.consts.iter().any(|(k, _)| k == key);
        if !given && !prim.optional().contains(key) {
            errors.push(format!("{at}: option \"{key}\" is missing"));
        }
    }
    for (key, edge) in &node.edges {
        if prim.const_keys().contains(&key.as_str()) {
            errors.push(format!(
                "{at}: \"{key}\" is fixed when the graph is built: a number or a word, not a node"
            ));
        }
        match edge {
            Edge::Node(i) if *i == index => {
                errors.push(format!("{at}: \"{key}\" reads the node itself"));
            }
            Edge::Node(i) if *i >= graph.nodes.len() => {
                errors.push(format!(
                    "{at}: \"{key}\" reads node {i}, which is not there"
                ));
            }
            _ => {}
        }
    }
    for (key, arg) in &node.consts {
        if prim.edge_keys().contains(&key.as_str()) {
            continue;
        }
        if prim.is_text(key) {
            match (arg, prim.words(key)) {
                (Arg::Text(word), Some(words)) if !words.contains(&word.as_str()) => {
                    errors.push(format!(
                        "{at}: \"{key}\" is \"{word}\"; it is one of {}",
                        words.join(", ")
                    ));
                }
                (Arg::Text(_), _) => {}
                (Arg::Num(_), _) => errors.push(format!("{at}: \"{key}\" is a word, not a number")),
            }
            continue;
        }
        let Arg::Num(value) = arg else {
            errors.push(format!("{at}: \"{key}\" is a number, not a word"));
            continue;
        };
        let whole_below =
            |count: usize| value.fract() == 0.0 && *value >= 0.0 && (*value as usize) < count;
        match (prim, key.as_str()) {
            (Prim::Input, "ch") if !whole_below(graph.channels) => {
                errors.push(format!(
                    "{at}: channel {value} of a stream with {}",
                    graph.channels
                ));
            }
            (Prim::Param, "index") if !whole_below(graph.params.len()) => {
                errors.push(format!("{at}: parameter {value} of {}", graph.params.len()));
            }
            (Prim::Allpass1, "ceiling") if *value <= 0.0 || *value > LOOP_CEILING => {
                errors.push(format!(
                    "{at}: a ceiling of {value}; it is above 0 and at most {LOOP_CEILING}"
                ));
            }
            (Prim::Delay, "maxTime") if value.is_nan() || *value < 1.0 => {
                errors.push(format!(
                    "{at}: a maxTime of {value} samples; it is at least 1"
                ));
            }
            (Prim::Tap, "line") => {
                let delay = whole_below(graph.nodes.len())
                    && graph.nodes[*value as usize].prim == Prim::Delay;
                if !delay {
                    errors.push(format!(
                        "{at}: \"line\" is the handle a g.delay returned, and nothing else"
                    ));
                }
            }
            _ => {}
        }
    }
}

/// The slot of the constant `value`, added to the pool when it is not in it yet.
fn constant(constants: &mut Vec<f32>, pool_start: usize, value: f32) -> Slot {
    if let Some(at) = constants[pool_start..]
        .iter()
        .position(|c| c.to_bits() == value.to_bits())
    {
        return Slot::Fixed(pool_start + at);
    }
    constants.push(value);
    Slot::Fixed(constants.len() - 1)
}

/// Arranges a checked graph into a program: the control instructions, then the ones that run
/// every frame, then the writes into the delay lines and the feedback pairs.
fn lower(graph: &Graph, order: &[usize], writers: &[usize], control: &[bool]) -> Program {
    let rate = graph.sample_rate.max(1) as f32;
    let mut constants: Vec<f32> = Vec::new();
    let in_slot: Vec<usize> = (0..graph.channels)
        .map(|_| {
            constants.push(0.0);
            constants.len() - 1
        })
        .collect();
    let param_slot: Vec<usize> = graph
        .params
        .iter()
        .map(|p| {
            constants.push(p.default);
            constants.len() - 1
        })
        .collect();
    let pool_start = constants.len();
    let mut pending: Vec<Pending> = Vec::new();
    let mut slot_of: HashMap<usize, Slot> = HashMap::new();
    let mut state_words = 0usize;
    let mut tap_state: HashMap<String, usize> = HashMap::new();
    let mut lines: Vec<(usize, usize)> = Vec::new();
    let mut line_of: HashMap<usize, usize> = HashMap::new();
    let mut memory = 0usize;
    let mut writes: Vec<(Op, Edge, usize)> = Vec::new();
    // every read of a delay line: the line, and its time when it is a number
    let mut reads: Vec<(usize, Option<f32>)> = Vec::new();

    // a time in milliseconds as the share of the distance an envelope keeps per frame
    let coefficient = |ms: f32| -> f32 {
        if ms <= 0.0 {
            0.0
        } else {
            (-1.0 / (f64::from(ms) / 1000.0 * f64::from(rate))).exp() as f32
        }
    };
    let zero = constant(&mut constants, pool_start, 0.0);

    for &index in order {
        let node = &graph.nodes[index];
        // the slot an option reads: a constant's, or the result of the node it names
        let slot = |key: &str, otherwise: f32, constants: &mut Vec<f32>| -> Slot {
            match node.edge(key) {
                Some(Edge::Node(i)) => slot_of[&i],
                Some(Edge::Const(v)) => constant(constants, pool_start, v),
                None => constant(constants, pool_start, otherwise),
            }
        };
        let steady = |key: &str| match node.edge(key) {
            Some(Edge::Node(i)) => control[i],
            _ => true,
        };
        let mut state = |words: usize| {
            let at = state_words;
            state_words += words;
            at
        };
        let emit = |pending: &mut Vec<Pending>,
                    op: Op,
                    a: Slot,
                    b: Slot,
                    c: Slot,
                    state: usize,
                    aux: u32,
                    control: bool| {
            pending.push(Pending {
                op,
                a,
                b,
                c,
                state,
                aux,
                control,
            });
            Slot::Result(pending.len() - 1)
        };
        let is_control = control[index];
        let result = match node.prim {
            Prim::Input => Slot::Fixed(in_slot[node.number("ch").unwrap_or(0.0) as usize]),
            Prim::Param => Slot::Fixed(param_slot[node.number("index").unwrap_or(0.0) as usize]),
            Prim::Add | Prim::Sub | Prim::Mul | Prim::Div | Prim::Min | Prim::Max => {
                let op = match node.prim {
                    Prim::Add => Op::Add,
                    Prim::Sub => Op::Sub,
                    Prim::Mul => Op::Mul,
                    Prim::Div => Op::Div,
                    Prim::Min => Op::Min,
                    _ => Op::Max,
                };
                let (a, b) = (
                    slot("a", 0.0, &mut constants),
                    slot("b", 0.0, &mut constants),
                );
                emit(&mut pending, op, a, b, zero, 0, 0, is_control)
            }
            Prim::Crossfade => {
                let (a, b, t) = (
                    slot("a", 0.0, &mut constants),
                    slot("b", 0.0, &mut constants),
                    slot("t", 0.0, &mut constants),
                );
                emit(&mut pending, Op::Crossfade, a, b, t, 0, 0, is_control)
            }
            Prim::Clip => {
                let (a, b, c) = (
                    slot("input", 0.0, &mut constants),
                    slot("min", 0.0, &mut constants),
                    slot("max", 0.0, &mut constants),
                );
                emit(&mut pending, Op::Clip, a, b, c, 0, 0, is_control)
            }
            Prim::Sanitise => {
                let a = slot("input", 0.0, &mut constants);
                emit(&mut pending, Op::Sanitise, a, zero, zero, 0, 0, is_control)
            }
            Prim::Math => {
                let a = slot("input", 0.0, &mut constants);
                let which = MATH_FNS
                    .iter()
                    .position(|f| Some(*f) == node.text("fn"))
                    .unwrap_or(0);
                emit(
                    &mut pending,
                    Op::Math,
                    a,
                    zero,
                    zero,
                    0,
                    which as u32,
                    is_control,
                )
            }
            Prim::Gain => {
                let (a, b) = (
                    slot("input", 0.0, &mut constants),
                    slot("db", 0.0, &mut constants),
                );
                emit(&mut pending, Op::Gain, a, b, zero, 0, 0, is_control)
            }
            Prim::Softclip => {
                let (a, b) = (
                    slot("input", 0.0, &mut constants),
                    slot("drive", 1.0, &mut constants),
                );
                let knee = constant(
                    &mut constants,
                    pool_start,
                    node.number("knee").unwrap_or(0.0),
                );
                emit(&mut pending, Op::Softclip, a, b, knee, 0, 0, is_control)
            }
            Prim::Onepole => {
                let (a, b) = (
                    slot("input", 0.0, &mut constants),
                    slot("coeff", 0.0, &mut constants),
                );
                let at = state(1);
                emit(&mut pending, Op::Onepole, a, b, zero, at, 0, false)
            }
            Prim::Allpass1 => {
                let (a, b) = (
                    slot("input", 0.0, &mut constants),
                    slot("coeff", 0.0, &mut constants),
                );
                let ceiling = constant(
                    &mut constants,
                    pool_start,
                    node.number("ceiling").unwrap_or(1.0),
                );
                let at = state(2);
                emit(&mut pending, Op::Allpass1, a, b, ceiling, at, 0, false)
            }
            Prim::Biquad => {
                let base = state(BIQUAD_WORDS);
                let kind = BIQUAD_KINDS
                    .iter()
                    .position(|k| Some(*k) == node.text("kind"))
                    .unwrap_or(0);
                // the coefficients follow a slider once per control tick, a signal every frame
                let slow = steady("freq") && steady("q") && steady("gainDb");
                let (f, q, g) = (
                    slot("freq", 1000.0, &mut constants),
                    slot("q", 0.707, &mut constants),
                    slot("gainDb", 0.0, &mut constants),
                );
                emit(
                    &mut pending,
                    Op::BiquadCoefficients,
                    f,
                    q,
                    g,
                    base,
                    kind as u32,
                    slow,
                );
                let a = slot("input", 0.0, &mut constants);
                emit(&mut pending, Op::Biquad, a, zero, zero, base, 0, false)
            }
            Prim::Lfo => {
                let op = if node.text("shape") == Some("triangle") {
                    Op::LfoTriangle
                } else {
                    Op::LfoSine
                };
                let (a, b) = (
                    slot("rate", 0.0, &mut constants),
                    slot("phase", 0.0, &mut constants),
                );
                let at = state(1);
                emit(&mut pending, op, a, b, zero, at, 0, false)
            }
            Prim::Envelope => {
                let a = slot("input", 0.0, &mut constants);
                let attack = constant(
                    &mut constants,
                    pool_start,
                    coefficient(node.number("attack").unwrap_or(0.0)),
                );
                let release = constant(
                    &mut constants,
                    pool_start,
                    coefficient(node.number("release").unwrap_or(0.0)),
                );
                let rms = u32::from(node.text("kind") == Some("rms"));
                let at = state(1);
                emit(
                    &mut pending,
                    Op::Envelope,
                    a,
                    attack,
                    release,
                    at,
                    rms,
                    false,
                )
            }
            Prim::Noise => {
                let seed =
                    (node.number("seed").unwrap_or(0.0) as i64).rem_euclid(NOISE_SEEDS) as u32;
                let at = state(1);
                emit(&mut pending, Op::Noise, zero, zero, zero, at, seed, false)
            }
            Prim::Delay => {
                // room for the longest delay asked for plus two frames
                let frames = node.number("maxTime").unwrap_or(1.0).ceil() as usize + 2;
                lines.push((memory, frames));
                memory += frames;
                let line = lines.len() - 1;
                line_of.insert(index, line);
                let op = if node.text("interp") == Some("none") {
                    Op::DelayReadNearest
                } else {
                    Op::DelayReadLinear
                };
                let time = slot("time", 1.0, &mut constants);
                reads.push((line, read_time(node)));
                // the write waits: every reader of the line runs before it, which makes a
                // loop through the line `time` frames long and not one more
                writes.push((
                    Op::DelayWrite,
                    node.edge("input").unwrap_or(Edge::Const(0.0)),
                    line,
                ));
                emit(&mut pending, op, time, zero, zero, line, 0, false)
            }
            Prim::Tap => {
                let line = line_of[&(node.number("line").unwrap_or(0.0) as usize)];
                let op = if node.text("interp") == Some("none") {
                    Op::DelayReadNearest
                } else {
                    Op::DelayReadLinear
                };
                let time = slot("time", 1.0, &mut constants);
                reads.push((line, read_time(node)));
                emit(&mut pending, op, time, zero, zero, line, 0, false)
            }
            Prim::TapOut => {
                let name = node.text("name").unwrap_or("").to_string();
                let at = *tap_state.entry(name).or_insert_with(|| state(1));
                emit(&mut pending, Op::TapRead, zero, zero, zero, at, 0, false)
            }
            Prim::TapIn => unreachable!("a tapIn is not in the order"),
        };
        slot_of.insert(index, result);
    }
    // a tap is lowered after its delay only when the plug-in made the delay first; the Lua
    // side hands out a line's handle from g.delay, so it always did
    for &index in writers {
        let node = &graph.nodes[index];
        let name = node.text("name").unwrap_or("").to_string();
        let at = *tap_state.entry(name).or_insert_with(|| {
            let at = state_words;
            state_words += 1;
            at
        });
        writes.push((
            Op::TapWrite,
            node.edge("source").unwrap_or(Edge::Const(0.0)),
            at,
        ));
    }
    for (op, source, state) in writes {
        let a = match source {
            Edge::Node(i) => slot_of[&i],
            Edge::Const(v) => constant(&mut constants, pool_start, v),
        };
        pending.push(Pending {
            op,
            a,
            b: zero,
            c: zero,
            state,
            aux: 0,
            control: false,
        });
    }

    // a channel that leaves by a number leaves by that constant's slot
    let outs: Vec<Slot> = graph
        .outputs
        .iter()
        .map(|out| match out {
            Edge::Node(i) => slot_of[i],
            Edge::Const(v) => constant(&mut constants, pool_start, *v),
        })
        .collect();
    // the control instructions first; a stable sort keeps each group in its own order
    let mut final_order: Vec<usize> = (0..pending.len()).collect();
    final_order.sort_by_key(|i| !pending[*i].control);
    let base = constants.len();
    let place = |slot: Slot| match slot {
        Slot::Fixed(at) => at,
        Slot::Result(p) => base + p,
    };
    let instrs: Vec<Instr> = final_order
        .iter()
        .map(|&p| {
            let at = &pending[p];
            Instr {
                op: at.op,
                dst: base + p,
                a: place(at.a),
                b: place(at.b),
                c: place(at.c),
                state: at.state,
                aux: at.aux,
            }
        })
        .collect();
    let control_end = pending.iter().filter(|p| p.control).count();
    let block = block_for(&instrs[control_end..], &lines, &reads, !writers.is_empty());
    let out_slot: Vec<usize> = outs.into_iter().map(place).collect();
    let ticks_per_second = rate / CONTROL_PERIOD as f32;
    Program {
        instrs,
        control_end,
        constants,
        in_slot,
        param_slot,
        param_step: graph
            .params
            .iter()
            .map(|p| {
                if p.smooth_ms <= 0.0 {
                    1.0
                } else {
                    (1.0 / (ticks_per_second * p.smooth_ms / 1000.0)).clamp(0.0, 1.0)
                }
            })
            .collect(),
        param_default: graph.params.iter().map(|p| p.default).collect(),
        out_slot,
        state_size: state_words,
        lines,
        memory_size: memory,
        sample_rate: graph.sample_rate.max(1),
        block,
    }
}

/// A delay or a tap's time when it is a number, the same every frame.
fn read_time(node: &Node) -> Option<f32> {
    match node.edge("time") {
        Some(Edge::Const(v)) => Some(v),
        None => Some(1.0),
        Some(Edge::Node(_)) => None,
    }
}

/// How many frames can run through one instruction before the next (see [`Program::block`]).
///
/// A read of a delay line at `time` frames back needs only what was written before the block
/// while the block is at most `time` long: a read between two frames takes the later one
/// with no weight when the time is whole. A time worked out as it plays may get as short as
/// a frame. A feedback pair hands a value on from one frame to the next, so it needs a frame
/// at a time.
fn block_for(
    audio: &[Instr],
    lines: &[(usize, usize)],
    reads: &[(usize, Option<f32>)],
    feedback: bool,
) -> usize {
    if feedback {
        return 1;
    }
    // filter coefficients worked out every frame run with their filter, frame by frame
    let paired = audio.iter().enumerate().all(|(i, instr)| {
        instr.op != Op::BiquadCoefficients
            || audio
                .get(i + 1)
                .is_some_and(|next| next.op == Op::Biquad && next.state == instr.state)
    });
    if !paired {
        return 1;
    }
    reads.iter().fold(BLOCK, |block, &(line, time)| {
        let longest = (lines[line].1 - 2) as f32;
        // read_line keeps the time between one frame and the line's length
        let frames = time.map_or(1, |t| t.max(1.0).min(longest).floor() as usize);
        block.min(frames.max(1))
    })
}

#[inline(always)]
fn sanitise(v: f32) -> f32 {
    if !v.is_finite() || v.abs() < DENORMAL {
        0.0
    } else {
        v
    }
}

#[inline(always)]
fn clip(v: f32, low: f32, high: f32) -> f32 {
    if v < low {
        low
    } else if v > high {
        high
    } else {
        v
    }
}

/// What may go into a delay line or a feedback pair: finite, and held within the ceiling.
#[inline(always)]
fn hold(v: f32) -> f32 {
    clip(sanitise(v), -LOOP_CEILING, LOOP_CEILING)
}

#[inline(always)]
fn fraction(v: f32) -> f32 {
    v - v.floor()
}

/// Up to `knee` unchanged; above it bent smoothly toward 1, which it never reaches.
fn softclip(x: f32, knee: f32) -> f32 {
    let magnitude = x.abs();
    if magnitude <= knee {
        return x;
    }
    let room = (1.0 - knee).max(1e-6);
    let over = magnitude - knee;
    let bent = knee + room * over / (over + room);
    if x < 0.0 {
        -bent
    } else {
        bent
    }
}

fn math(which: u32, x: f32) -> f32 {
    match which {
        0 => 2f32.powf(x),
        1 => x.ln() / LN_2,
        2 => x.sin(),
        3 => x.cos(),
        4 => x.tan(),
        5 => x.tanh(),
        6 => x.sqrt(),
        7 => x.abs(),
        8 => 1.0 / x,
        9 => -x,
        10 => x.floor(),
        _ => x.round(),
    }
}

/// The biquad coefficients of the Audio EQ Cookbook (Robert Bristow-Johnson), worked out in
/// double and written, divided by a0, into `out`: b0 b1 b2 a1 a2.
fn coefficients(
    out: &mut [f32],
    kind: u32,
    frequency: f32,
    q: f32,
    gain_db: f32,
    sample_rate: u32,
) {
    let rate = f64::from(sample_rate);
    let w = 2.0 * std::f64::consts::PI * f64::from(frequency).clamp(1.0, rate * 0.49) / rate;
    let alpha = w.sin() / (2.0 * f64::from(q).max(0.05));
    let cos = w.cos();
    let amp = 10f64.powf(f64::from(gain_db) / 40.0);
    let root = 2.0 * amp.sqrt() * alpha;
    let plain = [1.0 + alpha, -2.0 * cos, 1.0 - alpha];
    let (b, a) = match kind {
        0 => ([(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0], plain),
        1 => (
            [
                f64::midpoint(1.0, cos),
                -(1.0 + cos),
                f64::midpoint(1.0, cos),
            ],
            plain,
        ),
        2 => ([alpha, 0.0, -alpha], plain),
        3 => ([1.0, -2.0 * cos, 1.0], plain),
        4 => ([1.0 - alpha, -2.0 * cos, 1.0 + alpha], plain),
        5 => (
            [1.0 + alpha * amp, -2.0 * cos, 1.0 - alpha * amp],
            [1.0 + alpha / amp, -2.0 * cos, 1.0 - alpha / amp],
        ),
        6 => (
            [
                amp * ((amp + 1.0) - (amp - 1.0) * cos + root),
                2.0 * amp * ((amp - 1.0) - (amp + 1.0) * cos),
                amp * ((amp + 1.0) - (amp - 1.0) * cos - root),
            ],
            [
                (amp + 1.0) + (amp - 1.0) * cos + root,
                -2.0 * ((amp - 1.0) + (amp + 1.0) * cos),
                (amp + 1.0) + (amp - 1.0) * cos - root,
            ],
        ),
        _ => (
            [
                amp * ((amp + 1.0) + (amp - 1.0) * cos + root),
                -2.0 * amp * ((amp - 1.0) + (amp + 1.0) * cos),
                amp * ((amp + 1.0) + (amp - 1.0) * cos - root),
            ],
            [
                (amp + 1.0) - (amp - 1.0) * cos + root,
                2.0 * ((amp - 1.0) - (amp + 1.0) * cos),
                (amp + 1.0) - (amp - 1.0) * cos - root,
            ],
        ),
    };
    out[0] = (b[0] / a[0]) as f32;
    out[1] = (b[1] / a[0]) as f32;
    out[2] = (b[2] / a[0]) as f32;
    out[3] = (a[1] / a[0]) as f32;
    out[4] = (a[2] / a[0]) as f32;
}

/// Reads a delay line `delay` frames behind its head, between neighbors when `interpolate`.
fn read_line(line: &[f32], head: usize, delay: f32, interpolate: bool) -> f32 {
    let length = line.len();
    let wanted = delay.clamp(1.0, (length - 2) as f32);
    let back = (head % length) as f32 - wanted + length as f32;
    let whole = back as usize;
    let mut first = whole;
    if first >= length {
        first -= length;
    }
    if !interpolate {
        return line[first];
    }
    let mut second = first + 1;
    if second >= length {
        second = 0;
    }
    let a = line[first];
    a + (line[second] - a) * (back - whole as f32)
}

// What the instructions with state do to one frame, shared by the engine's two ways of
// running a program so that both make the same samples.

#[inline(always)]
fn crossfade(a: f32, b: f32, t: f32) -> f32 {
    if t == 0.0 {
        a
    } else if t == 1.0 {
        b
    } else {
        a + (b - a) * t
    }
}

#[inline(always)]
fn gain(x: f32, db: f32) -> f32 {
    x * (db * (std::f32::consts::LN_10 / 20.0)).exp()
}

#[inline(always)]
fn onepole(held: f32, x: f32, coefficient: f32) -> f32 {
    sanitise(x + (held - x) * coefficient)
}

/// `s` is the last input, then the last output.
#[inline(always)]
fn allpass(s: &mut [f32], x: f32, coefficient: f32, ceiling: f32) -> f32 {
    let next = clip(
        sanitise(coefficient * x + s[0] - coefficient * s[1]),
        -ceiling,
        ceiling,
    );
    s[0] = x;
    s[1] = next;
    next
}

/// `s` is the five coefficients, then x1 x2 y1 y2.
#[inline(always)]
fn biquad(s: &mut [f32], x: f32) -> f32 {
    let input = sanitise(x);
    let output = sanitise(s[0] * input + s[1] * s[5] + s[2] * s[6] - s[3] * s[7] - s[4] * s[8]);
    s[6] = s[5];
    s[5] = input;
    s[8] = s[7];
    s[7] = output;
    output
}

#[inline(always)]
fn lfo(at: &mut f32, hz: f32, phase: f32, sample_rate: u32, sine: bool) -> f32 {
    let turn = fraction(*at + phase);
    *at = fraction(*at + hz / sample_rate as f32);
    if sine {
        (2.0 * std::f32::consts::PI * turn).sin()
    } else {
        1.0 - 4.0 * (fraction(turn + 0.25) - 0.5).abs()
    }
}

#[inline(always)]
fn envelope(held: &mut f32, x: f32, attack: f32, release: f32, rms: bool) -> f32 {
    let level = if rms { x * x } else { x.abs() };
    *held = sanitise(level + (*held - level) * if level > *held { attack } else { release });
    if rms {
        held.sqrt()
    } else {
        *held
    }
}

#[inline(always)]
fn noise(at: &mut f32, seed: u32) -> f32 {
    *at = ((*at as i64 * NOISE_MULTIPLIER + 2 * i64::from(seed) + 1) & NOISE_MASK) as f32;
    *at / NOISE_HALF - 1.0
}

/// Runs a program. It allocates when it is made and never after.
pub struct Engine {
    program: Program,
    bus: Vec<f32>,
    state: Vec<f32>,
    memory: Vec<f32>,
    head: Vec<usize>,
    /// Where each parameter is going, and where it is now.
    target: Vec<f32>,
    here: Vec<f32>,
    moved: bool,
    until_tick: i32,
    /// A block of frames for each slot of the bus, [`BLOCK`] apart; empty when the program
    /// runs a frame at a time.
    rows: Vec<f32>,
    /// Whether the rows of the parameters and the control instructions hold what the bus
    /// does. A control tick of a single frame leaves them behind.
    rows_held: bool,
}

impl Engine {
    /// An engine that runs `program`, with its state cleared.
    pub fn new(program: Program) -> Engine {
        let slots = program.constants.len() + program.instrs.len();
        let mut rows = Vec::new();
        if program.block > 1 {
            rows = vec![0.0; slots * BLOCK];
            // a constant's row is filled once; nothing writes it after
            for (slot, value) in program.constants.iter().enumerate() {
                rows[slot * BLOCK..(slot + 1) * BLOCK].fill(*value);
            }
        }
        let mut engine = Engine {
            bus: vec![0.0; slots],
            state: vec![0.0; program.state_size.max(1)],
            memory: vec![0.0; program.memory_size.max(1)],
            head: vec![0; program.lines.len().max(1)],
            target: program.param_default.clone(),
            here: program.param_default.clone(),
            moved: false,
            until_tick: 0,
            rows,
            rows_held: false,
            program,
        };
        engine.reset();
        engine
    }

    /// The program it runs.
    pub fn program(&self) -> &Program {
        &self.program
    }

    /// Forgets everything heard so far and puts the parameters back at their defaults.
    pub fn reset(&mut self) {
        let constants = self.program.constants.len();
        self.bus[..constants].copy_from_slice(&self.program.constants);
        self.bus[constants..].fill(0.0);
        self.state.fill(0.0);
        self.memory.fill(0.0);
        self.head.fill(0);
        self.target.copy_from_slice(&self.program.param_default);
        self.here.copy_from_slice(&self.program.param_default);
        self.until_tick = 0;
        self.rows_held = false;
    }

    /// Forgets what was heard, and keeps the parameters where they are.
    pub fn clear(&mut self) {
        let constants = self.program.constants.len();
        self.bus[constants..].fill(0.0);
        self.state.fill(0.0);
        self.memory.fill(0.0);
        self.head.fill(0);
        self.until_tick = 0;
        self.settle();
    }

    /// Moves parameter `index` toward `value`, at its own pace.
    pub fn set_parameter(&mut self, index: usize, value: f32) {
        if let Some(target) = self.target.get_mut(index) {
            *target = value;
            self.moved = true;
        }
    }

    /// Puts every parameter where it is going at once.
    pub fn settle(&mut self) {
        self.here.copy_from_slice(&self.target);
        for (slot, value) in self.program.param_slot.iter().zip(&self.here) {
            self.bus[*slot] = *value;
        }
        self.moved = true;
        self.rows_held = false;
    }

    /// Runs one frame: `frame` has a sample for each channel coming in, and going out.
    pub fn process(&mut self, frame: &mut [f32]) {
        if frame.len() != self.program.in_slot.len() {
            return;
        }
        for (slot, sample) in self.program.in_slot.iter().zip(frame.iter()) {
            self.bus[*slot] = *sample;
        }
        if self.until_tick <= 0 || self.moved {
            self.tick();
            self.rows_held = false;
        }
        self.until_tick -= 1;
        self.step(self.program.control_end, self.program.instrs.len());
        // written only after every instruction ran, so a node that reads another channel
        // sees the frame as it arrived
        for (slot, sample) in self.program.out_slot.iter().zip(frame.iter_mut()) {
            *sample = self.bus[*slot];
        }
    }

    /// Runs `samples`, the channels in turn, through the program, a block of frames through
    /// each instruction before the next where the program allows it.
    pub fn run(&mut self, samples: &mut [f32]) {
        let channels = self.program.in_slot.len().max(1);
        if self.rows.is_empty() {
            for frame in samples.chunks_exact_mut(channels) {
                self.process(frame);
            }
            return;
        }
        let frames = samples.len() / channels;
        let mut at = 0;
        while at < frames {
            if self.until_tick <= 0 || self.moved {
                self.tick();
                self.rows_held = false;
            }
            if !self.rows_held {
                self.hold_rows();
            }
            // a block ends where the next control tick is due
            let count = self
                .program
                .block
                .min(frames - at)
                .min(self.until_tick as usize);
            self.until_tick -= count as i32;
            let part = &mut samples[at * channels..(at + count) * channels];
            for (channel, slot) in self.program.in_slot.iter().enumerate() {
                let row = &mut self.rows[slot * BLOCK..slot * BLOCK + count];
                for (value, frame) in row.iter_mut().zip(part.chunks_exact(channels)) {
                    *value = frame[channel];
                }
            }
            run_block(
                &self.program,
                &mut self.rows,
                &mut self.state,
                &mut self.memory,
                &mut self.head,
                count,
            );
            for (channel, slot) in self.program.out_slot.iter().enumerate() {
                let row = &self.rows[slot * BLOCK..slot * BLOCK + count];
                for (value, frame) in row.iter().zip(part.chunks_exact_mut(channels)) {
                    frame[channel] = *value;
                }
            }
            at += count;
        }
    }

    /// A control tick: every parameter a step on its way, and the control instructions run.
    fn tick(&mut self) {
        self.moved = false;
        for i in 0..self.here.len() {
            let wanted = self.target[i];
            let next = if (wanted - self.here[i]).abs() < CLOSE {
                wanted
            } else {
                // on its way: every control tick takes it a step further
                self.here[i] + (wanted - self.here[i]) * self.program.param_step[i]
            };
            self.here[i] = next;
            self.bus[self.program.param_slot[i]] = next;
        }
        self.step(0, self.program.control_end);
        self.until_tick = CONTROL_PERIOD;
    }

    /// Fills the rows of the parameters and the control instructions with what the bus holds,
    /// which stays so until the next control tick.
    fn hold_rows(&mut self) {
        let control = self.program.instrs[..self.program.control_end]
            .iter()
            .map(|instr| instr.dst);
        for slot in self.program.param_slot.iter().copied().chain(control) {
            self.rows[slot * BLOCK..(slot + 1) * BLOCK].fill(self.bus[slot]);
        }
        self.rows_held = true;
    }

    fn step(&mut self, from: usize, to: usize) {
        let rate = self.program.sample_rate;
        for index in from..to {
            let Instr {
                op,
                dst,
                a,
                b,
                c,
                state,
                aux,
            } = self.program.instrs[index];
            let (x, y, z) = (self.bus[a], self.bus[b], self.bus[c]);
            let out = match op {
                Op::Add => x + y,
                Op::Sub => x - y,
                Op::Mul => x * y,
                Op::Div => x / y,
                Op::Min => x.min(y),
                Op::Max => x.max(y),
                Op::Crossfade => crossfade(x, y, z),
                Op::Clip => clip(x, y, z),
                Op::Sanitise => sanitise(x),
                Op::Math => math(aux, x),
                Op::Gain => gain(x, y),
                Op::Softclip => softclip(x * y, z),
                Op::Onepole => {
                    self.state[state] = onepole(self.state[state], x, y);
                    self.state[state]
                }
                Op::Allpass1 => allpass(&mut self.state[state..state + 2], x, y, z),
                Op::BiquadCoefficients => {
                    coefficients(&mut self.state[state..state + 5], aux, x, y, z, rate);
                    0.0
                }
                Op::Biquad => biquad(&mut self.state[state..state + BIQUAD_WORDS], x),
                Op::LfoSine | Op::LfoTriangle => {
                    lfo(&mut self.state[state], x, y, rate, op == Op::LfoSine)
                }
                Op::Envelope => envelope(&mut self.state[state], x, y, z, aux == 1),
                Op::Noise => noise(&mut self.state[state], aux),
                Op::DelayReadLinear | Op::DelayReadNearest => {
                    let (offset, length) = self.program.lines[state];
                    read_line(
                        &self.memory[offset..offset + length],
                        self.head[state],
                        x,
                        op == Op::DelayReadLinear,
                    )
                }
                Op::DelayWrite => {
                    let (offset, length) = self.program.lines[state];
                    let at = self.head[state];
                    self.memory[offset + at] = hold(x);
                    self.head[state] = if at + 1 == length { 0 } else { at + 1 };
                    0.0
                }
                Op::TapRead => self.state[state],
                Op::TapWrite => {
                    self.state[state] = hold(x);
                    0.0
                }
            };
            self.bus[dst] = out;
        }
    }
}

/// Runs the instructions after the control ones over `count` frames of `rows`, each over every
/// frame before the next. The program's block says this is valid ([`Program::block`]).
fn run_block(
    program: &Program,
    rows: &mut [f32],
    state: &mut [f32],
    memory: &mut [f32],
    head: &mut [usize],
    count: usize,
) {
    let rate = program.sample_rate;
    let mut index = program.control_end;
    while index < program.instrs.len() {
        let Instr {
            op,
            dst,
            a,
            b,
            c,
            state: at,
            aux,
        } = program.instrs[index];
        let row = |slot: usize| &rows[slot * BLOCK..slot * BLOCK + count];
        let (x, y, z) = (row(a), row(b), row(c));
        let mut out = [0.0f32; BLOCK];
        let out = &mut out[..count];
        // a filter whose coefficients follow a signal is worked out and run frame by frame
        let mut filtered = None;
        macro_rules! each {
            (|$x:ident, $y:ident, $z:ident| $value:expr) => {
                for (((o, $x), $y), $z) in out.iter_mut().zip(x).zip(y).zip(z) {
                    let ($x, $y, $z) = (*$x, *$y, *$z);
                    *o = $value;
                }
            };
        }
        match op {
            Op::Add => each!(|x, y, _z| x + y),
            Op::Sub => each!(|x, y, _z| x - y),
            Op::Mul => each!(|x, y, _z| x * y),
            Op::Div => each!(|x, y, _z| x / y),
            Op::Min => each!(|x, y, _z| x.min(y)),
            Op::Max => each!(|x, y, _z| x.max(y)),
            Op::Crossfade => each!(|x, y, z| crossfade(x, y, z)),
            Op::Clip => each!(|x, y, z| clip(x, y, z)),
            Op::Sanitise => each!(|x, _y, _z| sanitise(x)),
            Op::Math => each!(|x, _y, _z| math(aux, x)),
            Op::Gain => each!(|x, y, _z| gain(x, y)),
            Op::Softclip => each!(|x, y, z| softclip(x * y, z)),
            Op::Onepole => {
                let mut held = state[at];
                each!(|x, y, _z| {
                    held = onepole(held, x, y);
                    held
                });
                state[at] = held;
            }
            Op::Allpass1 => {
                let s = &mut state[at..at + 2];
                each!(|x, y, z| allpass(s, x, y, z));
            }
            Op::BiquadCoefficients => {
                let filter = program.instrs[index + 1];
                let input = row(filter.a);
                let s = &mut state[at..at + BIQUAD_WORDS];
                for (f, o) in out.iter_mut().enumerate() {
                    coefficients(s, aux, x[f], y[f], z[f], rate);
                    *o = biquad(s, input[f]);
                }
                filtered = Some(filter.dst);
                index += 1;
            }
            Op::Biquad => {
                let s = &mut state[at..at + BIQUAD_WORDS];
                each!(|x, _y, _z| biquad(s, x));
            }
            Op::LfoSine | Op::LfoTriangle => {
                let sine = op == Op::LfoSine;
                let mut turn = state[at];
                each!(|x, y, _z| lfo(&mut turn, x, y, rate, sine));
                state[at] = turn;
            }
            Op::Envelope => {
                let mut held = state[at];
                each!(|x, y, z| envelope(&mut held, x, y, z, aux == 1));
                state[at] = held;
            }
            Op::Noise => {
                let mut step = state[at];
                each!(|_x, _y, _z| noise(&mut step, aux));
                state[at] = step;
            }
            Op::DelayReadLinear | Op::DelayReadNearest => {
                // every read of a line runs before its write, so the head is where the block
                // started and frame f reads from `head + f`
                let (offset, length) = program.lines[at];
                let line = &memory[offset..offset + length];
                let start = head[at];
                let interpolate = op == Op::DelayReadLinear;
                for (f, (o, time)) in out.iter_mut().zip(x).enumerate() {
                    *o = read_line(line, start + f, *time, interpolate);
                }
            }
            Op::DelayWrite => {
                let (offset, length) = program.lines[at];
                let mut place = head[at];
                for value in x {
                    memory[offset + place] = hold(*value);
                    place = if place + 1 == length { 0 } else { place + 1 };
                }
                head[at] = place;
            }
            // a program with a feedback pair runs a frame at a time
            Op::TapRead | Op::TapWrite => {}
        }
        let dst = filtered.unwrap_or(dst);
        rows[dst * BLOCK..dst * BLOCK + count].copy_from_slice(out);
        index += 1;
    }
}
