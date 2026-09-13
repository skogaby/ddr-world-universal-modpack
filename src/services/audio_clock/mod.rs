//! Deterministic, DAC-authority gameplay clock — the game-system integration
//! behind the `gameplay-timing-fixes` mod (design:
//! `.agents/scratchpad/2026-09-08-frame-timing/clock-rate-correction/design.md`,
//! RE: `docs/audio_clock_research.md`).
//!
//! Layout (design §0):
//! - [`fit`] — PURE sliding-window LSQ of the DirectSound play cursor vs QPC.
//! - [`onset`] — PURE arm / sanity-gate state machine (and the `anchor`
//!   mode's latched-Δ hold).
//! - [`engine`] — render-thread observers (shared `0x435A50` cursor dispatcher,
//!   the `0x43CAC0` produce hook, voice-start identity for streaming AND
//!   in-memory waves), publishing the [`fit::Line`] and the song/aux
//!   [`onset::Onset`] through lock-free seqlocks.
//! - [`game`] — game-thread side: the `(T, QPC)` pairing detour on the
//!   input-manager tick, the clock-patch call-out (`corrected_rbx`), the
//!   `song_reset` content-origin feed and scene gating.
//!
//! Ownership rules: the XACT engine detours stay owned by
//! `audio_sync_diag::xact` (one detour per target); this module subscribes
//! through the hooks it exposes. `song_rate::clock_patch` stays the sole owner
//! of the playhead redirect; its stub calls [`game::corrected_rbx`]. Everything
//! is fail-open: any missing seam ⇒ the clock never arms and the stock
//! `T − A` is what the game computes. No score/taint interaction.

pub mod engine;
pub mod fit;
pub mod game;
pub mod onset;
pub mod seqpub;

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU8, Ordering};

pub use seqpub::SeqPub;

use crate::log_warn;

/// Clock mode (config `gameplay_timing_fixes.audio_clock.mode`). All three
/// share the same arm: the song voice's exact onset `F0` plus the DirectSound
/// play-cursor line. They differ in what drives the count AFTER the arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Sliding-window LSQ of the play cursor drives the count every frame
    /// (default): fixes the onset error AND follows the DAC rate in-song.
    Fit,
    /// Newest cursor read extrapolated at the nominal rate drives the count.
    /// Only for platforms whose cursor is already smooth — on a staircase
    /// cursor this injects the staircase into the game clock.
    Raw,
    /// The cursor is consulted ONCE per voice: at the arm the onset error
    /// `E − (T − A)` is latched and thereafter the count is the stock
    /// `T − A` plus that constant. Fixes the 0–10 ms onset error, never lets
    /// the cursor steer the in-song clock (the conservative mode: no
    /// exposure to DirectSound-emulation cursor wander, no stock-reversion
    /// step when the fit loses its history mid-song). Gives up the in-song
    /// tick-vs-DAC drift correction.
    Anchor,
}

impl Mode {
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "fit" => Some(Self::Fit),
            "raw" => Some(Self::Raw),
            "anchor" => Some(Self::Anchor),
            _ => None,
        }
    }

    /// Config-file spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fit => "fit",
            Self::Raw => "raw",
            Self::Anchor => "anchor",
        }
    }

    fn to_u8(self) -> u8 {
        match self {
            Self::Fit => 0,
            Self::Raw => 1,
            Self::Anchor => 2,
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Raw,
            2 => Self::Anchor,
            _ => Self::Fit,
        }
    }
}

/// Boot-latched configuration (design §6), read once before the engine
/// seams install.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    pub mode: Mode,
    pub window_seconds: u32,
    pub latency_bias_ms: f32,
    pub assist_tick_alignment: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: Mode::Fit,
            window_seconds: 10,
            latency_bias_ms: 0.0,
            assist_tick_alignment: true,
        }
    }
}

/// `mods["gameplay-timing-fixes"]` per the config map (default ON since 2026-09-10).
static WANTED: AtomicBool = AtomicBool::new(false);
/// Live enable/disable (the mod's toggle) — a passthrough flag; seams are
/// never uninstalled.
static ENABLED: AtomicBool = AtomicBool::new(false);
static CONFIG_LATCHED: AtomicBool = AtomicBool::new(false);
static MODE: AtomicU8 = AtomicU8::new(0);
static WINDOW_SECONDS: AtomicU32 = AtomicU32::new(10);
static LATENCY_BIAS_MILLI: AtomicI32 = AtomicI32::new(0);
static TICK_ALIGNMENT: AtomicBool = AtomicBool::new(true);

/// Latch the config. Called from `lib.rs` right after the config store loads
/// (BEFORE the XACT factory bootstrap): the seams only install when the mod
/// is enabled in `mod-config.json`.
pub fn configure_from_config() {
    let Some(cfg) = crate::mods::config::get() else {
        return;
    };
    let wanted = crate::mods::mod_trait::mod_enabled_in_config(&cfg.mods, "gameplay-timing-fixes");
    WANTED.store(wanted, Ordering::Release);
    let section = cfg.gameplay_timing_fixes.clone().unwrap_or_default();
    let mode = match Mode::parse(&section.audio_clock.mode) {
        Some(mode) => mode,
        None => {
            if wanted {
                log_warn!(
                    "audio_clock: unknown mode '{}' -- using \"fit\"",
                    section.audio_clock.mode
                );
            }
            Mode::Fit
        }
    };
    MODE.store(mode.to_u8(), Ordering::Release);
    WINDOW_SECONDS.store(
        section.audio_clock.window_seconds.clamp(2, 60),
        Ordering::Release,
    );
    LATENCY_BIAS_MILLI.store(
        (section.audio_clock.latency_bias_ms.clamp(-500.0, 500.0) * 1000.0) as i32,
        Ordering::Release,
    );
    TICK_ALIGNMENT.store(section.assist_tick_alignment, Ordering::Release);
    CONFIG_LATCHED.store(true, Ordering::Release);
}

/// Whether the engine seams should install (mod enabled in config).
#[must_use]
pub fn wants_engine() -> bool {
    WANTED.load(Ordering::Acquire)
}

/// The latched config.
#[must_use]
pub fn config() -> Config {
    Config {
        mode: Mode::from_u8(MODE.load(Ordering::Acquire)),
        window_seconds: WINDOW_SECONDS.load(Ordering::Acquire),
        latency_bias_ms: LATENCY_BIAS_MILLI.load(Ordering::Acquire) as f32 / 1000.0,
        assist_tick_alignment: TICK_ALIGNMENT.load(Ordering::Acquire),
    }
}

/// Live arm permission (the mod's enable/disable).
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Release);
    if !on {
        game::on_disabled();
    }
}

#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

/// Whether the engine observers are installed AND the mod is enabled — the
/// condition under which the render-thread hooks do any work.
#[must_use]
pub fn is_available() -> bool {
    engine::installed() && is_enabled()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_parses_case_insensitively() {
        assert_eq!(Mode::parse(" FIT "), Some(Mode::Fit));
        assert_eq!(Mode::parse("raw"), Some(Mode::Raw));
        assert_eq!(Mode::parse("Anchor"), Some(Mode::Anchor));
        assert_eq!(Mode::parse("smooth"), None);
    }

    #[test]
    fn mode_round_trips_through_the_atomic_encoding() {
        for mode in [Mode::Fit, Mode::Raw, Mode::Anchor] {
            assert_eq!(Mode::from_u8(mode.to_u8()), mode);
            assert_eq!(Mode::parse(mode.as_str()), Some(mode));
        }
        assert_eq!(Mode::from_u8(200), Mode::Fit);
    }
}
