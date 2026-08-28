//! SMX hardware service — a pure-Rust port of the used subset of the
//! StepManiaX SDK (`stepmaniax-sdk`, cabinet-IO fork) plus SpiceManiaX's
//! DDR→SMX light mapping, talking to the cabinet over raw Windows HID.
//!
//! Layering (design "Components and Interfaces"):
//!
//! - [`protocol`] — pure HID framing + wire encoders (no IO).
//! - [`input_map`] — pure SMX panel mask → DDR arrows.
//! - [`light_map`] — pure DDR light frame → SMX stage payloads.
//! - [`cabinet_map`] — pure DDR light frame → SMX cabinet payloads
//!   (marquee / vertical strips / spotlights).
//! - [`device`] — the impure edge: SetupAPI discovery + HidD filtering +
//!   overlapped `CreateFileW` handles.
//! - [`transport`] — the dedicated `ABOVE_NORMAL` IO thread: discovery
//!   polling, input reads → atomic masks, serial command flow control, and
//!   the ~30 Hz lights drain.
//!
//! The service is started by the `smx-hardware` mod's `enable()` (not from
//! `lib.rs` init) — hardware-specific machinery must not spin up when the
//! mod is disabled.

pub mod cabinet_map;
pub mod device;
pub mod input_map;
pub mod light_map;
pub mod protocol;
pub mod transport;
