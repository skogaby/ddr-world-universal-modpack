//! Gameplay Timing Fixes — the deterministic, DAC-authority gameplay clock
//! (plus assist-tick alignment) packaged as ONE toggleable mod.
//!
//! What it fixes (RE: `docs/audio_clock_research.md`): the stock game anchors
//! its music clock ~0.1 ms after the song voice's `Start` returns, but the
//! XACT engine only *posts* that Start and mixes the first samples at the
//! next 10 ms render pass. The audible onset relative to the anchor therefore
//! varies uniformly over ±5 ms from play to play, and — because the same
//! count positions the arrows AND judges the steps — the visual and aural
//! offsets move together. Within a song the game tick (a boot-calibrated
//! RDTSC) and the DAC crystal drift apart by tens of ppm.
//!
//! What it does (design: `.agents/scratchpad/2026-09-08-frame-timing/
//! clock-rate-correction/design.md`): observes every mix pass (DirectSound
//! play cursor vs QPC), latches the exact output frame the song's sample 0
//! was mixed at (`F0`), and replaces `T − A` in the game's music count with
//! `(P̂(t_frame) − F0)/Hz + C` — a count that starts at the true DAC onset
//! and advances at the DAC's rate. `C` reproduces the MEAN stock latency so
//! existing SOUND_OFFSET calibrations stay valid. No windows, score math,
//! saves or rate semantics change; **no score taint** (maintainer decision).
//!
//! Lifecycle rules (design §0): the engine seams install at boot ONLY when
//! the mod is enabled in `mod-config.json` (`mods["gameplay-timing-fixes"]`,
//! default ON since 2026-09-10 — the first cabinet build shipped it OFF); a
//! live toggle is a passthrough flag. Toggling ON from the menu when the
//! config had it OFF therefore takes effect at the NEXT launch — and the mod
//! MUST report `is_active() == true` in that state (`armed_next_launch`), or
//! the registry records the toggle as self-disabled and the menu writes it
//! back to the config as `false` (the custom-resolution trap, 2026-09-09).
//! Config section `gameplay_timing_fixes` (mode / window / latency bias /
//! assist-tick alignment) — see `mods/config.rs`.
//!
//! Module layout: `mod.rs` (Mod trait, INFO/WARN lines), `tick_align.rs`
//! (assist-tick alignment glue over `services::audio_clock` + `assist_tick`).

pub mod tick_align;

use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::audio_clock;
use crate::{log_info, log_warn};

pub struct GameplayTimingFixesMod {
    /// The deterministic clock is running THIS session (seams installed at
    /// boot + game-side pieces present + `enable` ran).
    active: bool,
    /// `enable` ran on a boot whose config had the mod OFF (no seams this
    /// launch): the mod is functional and the operator's ON toggle is real —
    /// it just lands at the next launch. Counts as active for the registry.
    armed_next_launch: bool,
}

impl GameplayTimingFixesMod {
    pub fn new() -> Self {
        Self {
            active: false,
            armed_next_launch: false,
        }
    }
}

impl Mod for GameplayTimingFixesMod {
    fn id(&self) -> &str {
        "gameplay-timing-fixes"
    }
    fn name(&self) -> &str {
        "Gameplay Timing Fixes"
    }
    fn description(&self) -> &str {
        "Deterministic DAC-locked music clock: no play-to-play onset jitter, no in-song drift (applies next launch)"
    }
    fn required_signatures(&self) -> &[&str] {
        // Fail-open by design: every seam reports its own availability and the
        // clock simply never arms when one is missing.
        &[]
    }

    fn init(&mut self, _ctx: &ModContext) -> bool {
        if !audio_clock::wants_engine() {
            log_info!(
                "GameplayTimingFixes: disabled in config -- no engine seams installed this launch (enable + relaunch to activate)"
            );
        }
        true
    }

    fn enable(&mut self) {
        if !audio_clock::wants_engine() {
            log_warn!(
                "GameplayTimingFixes: enable requested but the boot-time seams were not installed (mod was OFF in config at launch) -- the toggle is saved; the clock activates at the NEXT launch"
            );
            self.active = false;
            self.armed_next_launch = true;
            return;
        }
        self.armed_next_launch = false;
        // Only the GAME-side pieces are knowable here: the engine observers
        // install when the GAME creates its XACT engine (the factory-return
        // window), which lands moments AFTER this enable on a normal boot —
        // gating on `engine::installed()` here self-disabled the mod on the
        // first CrossOver smoke (2026-09-09). The factory bootstrap is armed
        // (or not) before this point; `game.rs` WARNs at the first play-scene
        // entry if the observers still have not landed by then.
        let game_ok = audio_clock::game::pairing_installed();
        let callout_ok = crate::services::song_rate::clock_patch::installed_callout().is_some();
        if !(game_ok && callout_ok) {
            log_warn!(
                "GameplayTimingFixes: incomplete -- (T,QPC) pairing {}, clock-stub call-out {} -- the gameplay clock stays stock",
                if game_ok { "installed" } else { "MISSING" },
                if callout_ok { "present" } else { "MISSING" }
            );
            self.active = false;
            return;
        }
        audio_clock::set_enabled(true);
        self.active = true;
        let cfg = audio_clock::config();
        log_info!(
            "GameplayTimingFixes: enabled -- audio clock mode {:?}, fit window {} s, latency bias {:+.1} ms, assist-tick alignment {}; engine observers {}",
            cfg.mode,
            cfg.window_seconds,
            cfg.latency_bias_ms,
            if cfg.assist_tick_alignment { "on" } else { "off" },
            if audio_clock::engine::installed() {
                "installed"
            } else {
                "pending (land when the game creates its XACT engine)"
            }
        );
        audio_clock::game::log_status();
        tick_align::enable(cfg.assist_tick_alignment);
    }

    fn disable(&mut self) {
        tick_align::disable();
        audio_clock::set_enabled(false);
        if self.active {
            audio_clock::game::log_status();
        }
        self.active = false;
        self.armed_next_launch = false;
        log_info!("GameplayTimingFixes: disabled -- gameplay clock back to stock (seams stay installed as passthrough)");
    }

    /// Active when the clock runs this session OR the mod was enabled from
    /// the menu on an OFF-at-launch boot (functional, effect next launch).
    /// False only for the genuine self-disable (seams wanted at boot but the
    /// game-side pieces are missing on this build).
    fn is_active(&self) -> bool {
        self.active || self.armed_next_launch
    }
}
