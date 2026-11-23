// SPDX-License-Identifier: GPL-3.0-or-later

//! An effect plug-in: a Lua file in the format of Andamp on Android.
//!
//! The script runs here, off the audio thread, in an interpreter with no files, no system and
//! no way to load more code, for a limited time and in limited memory. It leaves a
//! description of its controls and, for one audio format at a time, a graph of primitives
//! ([`graph`](super::graph)). No Lua runs while audio plays.

use std::time::{Duration, Instant};

use mlua::{HookTriggers, Lua, LuaOptions, StdLib, Table, Value, VmState};

use super::graph::{compile, Arg, Edge, Graph, Node, ParamRate, Prim, Program};

/// What the script sees, written in Lua.
const PRELUDE: &str = include_str!("prelude.lua");
/// The most a plug-in's source may be, in bytes.
const MAX_SOURCE: usize = 256 * 1024;
/// How long a script may run, and how much memory it may hold.
const LOAD_TIME: Duration = Duration::from_secs(3);
const LOAD_MEMORY: usize = 64 * 1024 * 1024;
/// The schema version a plug-in may ask for.
const API: i64 = 1;
/// Milliseconds a control's change is smoothed over when the plug-in does not say.
const DEFAULT_SMOOTH_MS: f32 = 20.0;
/// The unit symbols a control may carry.
const UNITS: [&str; 10] = [
    "db",
    "hz",
    "khz",
    "ms",
    "s",
    "pc",
    "semitone12TET",
    "coef",
    "degree",
    "bar",
];
/// The sample rates a plug-in must build at to be taken.
const CHECKED_RATES: [u32; 3] = [44_100, 48_000, 22_050];

/// What a plug-in says of itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Info {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub about: String,
}

/// What kind of control a parameter is.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// A slider.
    Number,
    /// One of several named values, lowest first.
    Choice(Vec<(String, f32)>),
    /// A switch: 0 or 1.
    Toggle,
}

/// A control of a plug-in.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub smooth_ms: f32,
    /// The unit its value is read in; empty for a bare number.
    pub unit: String,
    pub help: String,
    /// What the value is multiplied by to be shown.
    pub scale: f32,
    /// Decimals shown; worked out from the range when the plug-in gave none.
    pub decimals: Option<u32>,
    /// What to show at the minimum.
    pub zero: String,
}

/// Named settings a plug-in comes with: for each, the parameters it sets, by their place.
#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    pub name: String,
    pub values: Vec<(usize, f32)>,
}

/// A loaded plug-in: what it is, its controls and its source, from which a program is built
/// for each audio format.
#[derive(Debug, Clone, PartialEq)]
pub struct Plugin {
    pub info: Info,
    pub params: Vec<Param>,
    pub presets: Vec<Preset>,
    source: String,
}

impl Plugin {
    /// Loads a plug-in from its source. It is refused, with the reason, when the script fails,
    /// leaves something out, or cannot be built for stereo at 44.1, 48 and 22.05 kHz.
    pub fn load(source: &str) -> Result<Plugin, String> {
        if source.len() > MAX_SOURCE {
            return Err(format!("the file is larger than {} KiB", MAX_SOURCE / 1024));
        }
        let lua = run(source)?;
        let described = table(&lua)?;
        let info = info(&described)?;
        let params = params(&described)?;
        let presets = presets(&described, &params)?;
        let plugin = Plugin {
            info,
            params,
            presets,
            source: source.to_string(),
        };
        for rate in CHECKED_RATES {
            plugin
                .build(rate, 2)
                .map_err(|e| format!("at {rate} Hz: {e}"))?;
        }
        Ok(plugin)
    }

    /// The plug-in's program for audio at `sample_rate` with `channels` channels.
    pub fn build(&self, sample_rate: u32, channels: usize) -> Result<Program, String> {
        let lua = run(&self.source)?;
        let build: mlua::Function = lua
            .globals()
            .get("__andamp_build")
            .map_err(|e| message(&e))?;
        build
            .call::<()>((sample_rate, channels))
            .map_err(|e| message(&e))?;
        let described = table(&lua)?;
        let graph = Graph {
            nodes: nodes(&described)?,
            params: self
                .params
                .iter()
                .map(|p| ParamRate {
                    default: p.default,
                    smooth_ms: p.smooth_ms,
                })
                .collect(),
            outputs: outputs(&described)?,
            channels,
            sample_rate,
        };
        compile(&graph).map_err(|errors| errors.join("; "))
    }
}

/// Runs the prelude and then `source` in a fresh, shut-in interpreter.
fn run(source: &str) -> Result<Lua, String> {
    let lua = Lua::new_with(
        StdLib::MATH | StdLib::STRING | StdLib::TABLE,
        LuaOptions::default(),
    )
    .map_err(|e| message(&e))?;
    let _ = lua.set_memory_limit(LOAD_MEMORY);
    let deadline = Instant::now() + LOAD_TIME;
    lua.set_hook(
        HookTriggers::new().every_nth_instruction(10_000),
        move |_, _| {
            if Instant::now() > deadline {
                Err(mlua::Error::runtime("the plug-in took too long to load"))
            } else {
                Ok(VmState::Continue)
            }
        },
    );
    lua.load(PRELUDE)
        .set_name("=andamp")
        .exec()
        .map_err(|e| message(&e))?;
    lua.load(source)
        .set_name("=plugin")
        .exec()
        .map_err(|e| message(&e))?;
    Ok(lua)
}

/// An interpreter's error as one line, without the interpreter's own trace.
fn message(error: &mlua::Error) -> String {
    let text = match error {
        mlua::Error::CallbackError { cause, .. } => cause.to_string(),
        other => other.to_string(),
    };
    let first = text.lines().next().unwrap_or("the plug-in failed");
    first
        .trim_start_matches("runtime error: ")
        .trim_start_matches("syntax error: ")
        .to_string()
}

fn table(lua: &Lua) -> Result<Table, String> {
    lua.globals().get("__andamp").map_err(|e| message(&e))
}

fn text(table: &Table, key: &str) -> Option<String> {
    match table.get::<Value>(key).ok()? {
        Value::String(s) => Some(s.to_string_lossy()),
        _ => None,
    }
}

fn number(table: &Table, key: &str) -> Option<f32> {
    match table.get::<Value>(key).ok()? {
        Value::Integer(i) => Some(i as f32),
        Value::Number(n) => Some(n as f32),
        _ => None,
    }
}

fn info(described: &Table) -> Result<Info, String> {
    let Ok(Value::Table(plugin)) = described.get::<Value>("plugin") else {
        return Err("the plug-in has no plugin { } block".into());
    };
    let required = |key: &str| text(&plugin, key).ok_or(format!("plugin {{ }} has no {key}"));
    match plugin.get::<Value>("api") {
        Ok(Value::Nil) => {}
        Ok(Value::Integer(API)) => {}
        Ok(Value::Number(n)) if n == API as f64 => {}
        _ => return Err(format!("the plug-in asks for another api than {API}")),
    }
    if !matches!(described.get::<Value>("ui"), Ok(Value::Nil | Value::Table(_))) {
        return Err("ui takes a table of widgets".into());
    }
    Ok(Info {
        id: required("id")?,
        name: required("name")?,
        version: required("version")?,
        author: text(&plugin, "author").unwrap_or_default(),
        about: text(&plugin, "about").unwrap_or_default(),
    })
}

fn params(described: &Table) -> Result<Vec<Param>, String> {
    let list: Table = described.get("params").map_err(|e| message(&e))?;
    let mut params: Vec<Param> = Vec::new();
    for entry in list.sequence_values::<Table>() {
        let t = entry.map_err(|e| message(&e))?;
        let id = text(&t, "id").ok_or("a parameter has no id")?;
        if params.iter().any(|p| p.id == id) {
            return Err(format!("two parameters are called \"{id}\""));
        }
        let unit = text(&t, "unit").unwrap_or_default();
        if !unit.is_empty() && !UNITS.contains(&unit.as_str()) {
            return Err(format!("parameter \"{id}\": \"{unit}\" is not a unit; it is one of {}", UNITS.join(", ")));
        }
        let display = match t.get::<Value>("display") {
            Ok(Value::Table(d)) => Some(d),
            _ => None,
        };
        let kind = text(&t, "kind").unwrap_or_default();
        let mut param = Param {
            name: text(&t, "name").unwrap_or_else(|| id.clone()),
            kind: Kind::Number,
            min: number(&t, "min").unwrap_or(0.0),
            max: number(&t, "max").unwrap_or(1.0),
            default: 0.0,
            smooth_ms: number(&t, "smooth").unwrap_or(DEFAULT_SMOOTH_MS).max(0.0),
            unit,
            help: text(&t, "help").unwrap_or_default(),
            scale: display.as_ref().and_then(|d| number(d, "scale")).unwrap_or(1.0),
            decimals: display.as_ref().and_then(|d| number(d, "decimals")).map(|d| d.max(0.0) as u32),
            zero: display.as_ref().and_then(|d| text(d, "zero")).unwrap_or_default(),
            id,
        };
        let id = &param.id;
        match kind.as_str() {
            "number" => {
                param.default = number(&t, "default").ok_or(format!("parameter \"{id}\" has no default"))?;
                if param.default < param.min || param.default > param.max {
                    return Err(format!("parameter \"{id}\": its default is outside its min and max"));
                }
            }
            "toggle" => {
                param.kind = Kind::Toggle;
                (param.min, param.max) = (0.0, 1.0);
                param.default = match t.get::<Value>("default") {
                    Ok(Value::Boolean(on)) => f32::from(u8::from(on)),
                    _ => return Err(format!("parameter \"{id}\" has no default: true or false")),
                };
            }
            _ => {
                let Ok(Value::Table(options)) = t.get::<Value>("options") else {
                    return Err(format!("parameter \"{id}\" has no options"));
                };
                let mut found: Vec<(String, f32)> = Vec::new();
                for pair in options.pairs::<String, f32>() {
                    found.push(pair.map_err(|_| format!("parameter \"{id}\": an option is a name and a number"))?);
                }
                if found.is_empty() {
                    return Err(format!("parameter \"{id}\" has no options"));
                }
                // a Lua table with names for keys has no order: the options go by their value
                found.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
                let default = text(&t, "default").ok_or(format!("parameter \"{id}\" has no default: one of its options' names"))?;
                param.default = found
                    .iter()
                    .find(|(name, _)| *name == default)
                    .map(|(_, value)| *value)
                    .ok_or(format!("parameter \"{id}\": its default \"{default}\" is not one of its options"))?;
                param.min = found.first().map_or(0.0, |o| o.1);
                param.max = found.last().map_or(0.0, |o| o.1);
                // the values between two settings are other settings: a choice jumps
                param.smooth_ms = 0.0;
                param.kind = Kind::Choice(found);
            }
        }
        params.push(param);
    }
    Ok(params)
}

fn presets(described: &Table, params: &[Param]) -> Result<Vec<Preset>, String> {
    let list: Table = described.get("presets").map_err(|e| message(&e))?;
    let mut presets = Vec::new();
    for entry in list.sequence_values::<Table>() {
        let t = entry.map_err(|_| "a preset is a table with a name and values".to_string())?;
        let name = text(&t, "name").ok_or("a preset has no name")?;
        let Ok(Value::Table(given)) = t.get::<Value>("values") else {
            return Err(format!("preset \"{name}\" has no values"));
        };
        let mut values = Vec::new();
        for pair in given.pairs::<String, Value>() {
            let (id, value) = pair.map_err(|_| format!("preset \"{name}\": a value goes by a parameter's id"))?;
            let index = params
                .iter()
                .position(|p| p.id == id)
                .ok_or(format!("preset \"{name}\" names \"{id}\", which the plug-in does not have"))?;
            let value = match value {
                Value::Integer(i) => i as f32,
                Value::Number(n) => n as f32,
                Value::Boolean(on) => f32::from(u8::from(on)),
                _ => return Err(format!("preset \"{name}\": \"{id}\" is set to something that is not a number, true or false")),
            };
            values.push((index, value));
        }
        values.sort_by_key(|v| v.0);
        presets.push(Preset { name, values });
    }
    Ok(presets)
}

fn edge(value: &Table) -> Result<Edge, String> {
    if let Some(c) = number(value, "c") {
        return Ok(Edge::Const(c));
    }
    match value.get::<Value>("n") {
        Ok(Value::Integer(n)) if n >= 0 => Ok(Edge::Node(n as usize)),
        _ => Err("the graph names something that is not a node".into()),
    }
}

fn nodes(described: &Table) -> Result<Vec<Node>, String> {
    let list: Table = described.get("nodes").map_err(|e| message(&e))?;
    let mut nodes = Vec::new();
    for entry in list.sequence_values::<Table>() {
        let t = entry.map_err(|e| message(&e))?;
        let name = text(&t, "prim").unwrap_or_default();
        let prim = Prim::named(&name).ok_or(format!("g.{name} is not a primitive"))?;
        let mut edges = Vec::new();
        let given: Table = t.get("edges").map_err(|e| message(&e))?;
        for pair in given.pairs::<String, Table>() {
            let (key, value) = pair.map_err(|e| message(&e))?;
            edges.push((key, edge(&value)?));
        }
        let mut consts = Vec::new();
        let given: Table = t.get("consts").map_err(|e| message(&e))?;
        for pair in given.pairs::<String, Value>() {
            let (key, value) = pair.map_err(|e| message(&e))?;
            let arg = match value {
                Value::Integer(i) => Arg::Num(i as f32),
                Value::Number(n) => Arg::Num(n as f32),
                Value::String(s) => Arg::Text(s.to_string_lossy()),
                _ => return Err(format!("\"{key}\" of g.{name} is a number or a word")),
            };
            consts.push((key, arg));
        }
        // the keys of a Lua table come in no order: sorted, the same file gives the same program
        edges.sort_by(|a, b| a.0.cmp(&b.0));
        consts.sort_by(|a, b| a.0.cmp(&b.0));
        nodes.push(Node { prim, edges, consts });
    }
    Ok(nodes)
}

fn outputs(described: &Table) -> Result<Vec<Edge>, String> {
    let list: Table = described.get("outputs").map_err(|e| message(&e))?;
    list.sequence_values::<Table>()
        .map(|entry| edge(&entry.map_err(|e| message(&e))?))
        .collect()
}
