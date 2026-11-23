// SPDX-License-Identifier: GPL-3.0-or-later

//! Effects: plug-ins written in Lua that describe a graph of built-in primitives
//! ([`lua`]), the engine that runs such a graph ([`graph`]), and the rack of effects the
//! player's audio passes through ([`rack`]).

pub mod graph;
pub mod lua;
pub mod rack;
