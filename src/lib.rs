//! A library target, so the pure parts (parser, timeline, clock, layout) can be
//! tested without a terminal or a running player.
// So an option that stops being read breaks the build instead of going quiet.
#![warn(unused, dead_code, unreachable_pub)]

pub mod cava;
pub mod cli;
pub mod clock;
pub mod config;
pub mod lrc;
pub mod lyrics;
pub mod offsets;
pub mod player;
pub mod prefs;
pub mod render;
pub mod sync;
pub mod timeline;
pub mod tui;
