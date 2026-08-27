//! Shared type definitions used across mods and services.
//!
//! - **scenes** — Scene ID → human-readable name mapping for DDR World's
//!   screen/sequence system (attract, gameplay, menus, etc.).
//! - **buttons** — Arcade button bitmasks, Player enum, and InputEvent types
//!   for the P1/P2 dance panels and operator buttons.
//! - **game_note** — Raw layout of the game's 0x60-byte per-note record,
//!   its kind/state/panel/result constants, and the read-only helpers for
//!   walking a gameplay actor's Results vector.

#[allow(dead_code)]
pub mod buttons;
#[allow(dead_code)]
pub mod game_note;
#[allow(dead_code)]
pub mod scenes;
