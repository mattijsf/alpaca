// SPDX-License-Identifier: GPL-3.0-or-later

//! The rack as text a shell builds its Effects window from.

use super::lua::Kind;
use super::rack::Rack;

/// `text` as a JSON string.
fn json_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A number as JSON, which has no word for one that is not finite.
fn json_number(v: f32) -> String {
    if v.is_finite() {
        format!("{v}")
    } else {
        "0".into()
    }
}

/// The rack as JSON: `{"effects": [...]}`, an object for each effect with its id, name,
/// version, author, what it says about itself, whether it is on, why it failed if it did, its
/// controls (`params`: id, name, kind, range, default, value, unit, help, how the value is
/// read out, and the options of a choice) and its presets (name, and a value for each control
/// by its place).
pub fn json(rack: &Rack) -> String {
    let effects: Vec<String> = rack
        .effects()
        .iter()
        .map(|effect| {
            let info = &effect.plugin.info;
            let params: Vec<String> = effect
                .plugin
                .params
                .iter()
                .zip(&effect.values)
                .map(|(p, value)| {
                    let (kind, options) = match &p.kind {
                        Kind::Number => ("number", Vec::new()),
                        Kind::Toggle => ("toggle", Vec::new()),
                        Kind::Choice(options) => (
                            "choice",
                            options
                                .iter()
                                .map(|(name, v)| format!("{{\"name\":{},\"value\":{}}}", json_text(name), json_number(*v)))
                                .collect(),
                        ),
                    };
                    format!(
                        "{{\"id\":{},\"name\":{},\"kind\":\"{kind}\",\"min\":{},\"max\":{},\"default\":{},\"value\":{},\
                         \"unit\":{},\"help\":{},\"scale\":{},\"decimals\":{},\"zero\":{},\"readout\":{},\"options\":[{}]}}",
                        json_text(&p.id),
                        json_text(&p.name),
                        json_number(p.min),
                        json_number(p.max),
                        json_number(p.default),
                        json_number(*value),
                        json_text(&p.unit),
                        json_text(&p.help),
                        json_number(p.scale),
                        p.decimals.map_or(-1, |d| d as i32),
                        json_text(&p.zero),
                        json_text(&p.readout(*value)),
                        options.join(",")
                    )
                })
                .collect();
            let presets: Vec<String> = effect
                .plugin
                .presets
                .iter()
                .map(|preset| {
                    let values: Vec<String> = preset
                        .values
                        .iter()
                        .map(|(index, v)| format!("[{index},{}]", json_number(*v)))
                        .collect();
                    format!("{{\"name\":{},\"values\":[{}]}}", json_text(&preset.name), values.join(","))
                })
                .collect();
            format!(
                "{{\"id\":{},\"name\":{},\"version\":{},\"author\":{},\"about\":{},\"on\":{},\"error\":{},\
                 \"params\":[{}],\"presets\":[{}]}}",
                json_text(&info.id),
                json_text(&info.name),
                json_text(&info.version),
                json_text(&info.author),
                json_text(&info.about),
                effect.on,
                json_text(&effect.error),
                params.join(","),
                presets.join(",")
            )
        })
        .collect();
    format!("{{\"effects\":[{}]}}", effects.join(","))
}
