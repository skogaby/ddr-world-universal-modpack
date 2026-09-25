//! Real Speed Calculation Fix (`real-speed-fix`, default ON) — makes the REAL
//! SPEED scroll mode (speed type 0: "arrows travel at N") derive its
//! multiplier from the chart's Core BPM instead of its Max BPM, so songs with
//! a short high-BPM burst no longer play the whole chart slow.
//!
//! ## Mechanism
//!
//! Three byte patches inside `ddr::player::Option::SetScrollSpeed`, anchored
//! on the `real_speed_bpm_anchor` AOB (`divsd xmm0,[rcx]`) and applied by
//! `real_speed.rs`. The stock setter computes the normalized multiplier as
//! `clamp(trunc(target·100 / Max BPM), 25, 800)`; the patches swap the
//! divisor for Core BPM and leave the rest of the derivation untouched (see
//! `real_speed.rs` for the per-patch bytes). The original bytes are saved at
//! `enable()` and written back at `disable()`. The patch changes what the
//! setter produces, so a toggle takes effect the next time the game runs it.
//!
//! ## Interaction with song rate
//!
//! At a committed non-identity song rate, `services/song_rate/real_speed`
//! (owned by the Song Playback Speed mod, not by this one) recomputes the
//! multiplier for Real-Speed sides from `Core BPM × effective rate` and
//! writes it over the setter's outputs (the GamePlayActor multiplier cluster
//! and `Option+0x10`) at each side's first judge dispatch. That recompute
//! always uses Core BPM whatever this toggle says, so the toggle only matters
//! at 100 %, where the song-rate path does nothing.
//!
//! ## Degradation
//!
//! `required_signatures` is empty: a missing `real_speed_bpm_anchor` logs one
//! WARN at init and `enable()` patches nothing (stock Max-BPM behavior).
//!
//! RE notes: `docs/binary_modpack_research.md` §4. The hex-edit modpack's
//! R15/R16 "logf guard" is deliberately not ported: it actually patched the
//! pacemaker readout (`docs/pacemaker_display_research.md`).

pub mod real_speed;

use crate::mods::mod_trait::{Mod, ModContext};
use crate::{log_info, log_warn};

pub struct RealSpeedFixMod {
    real_speed_ready: bool,
    active: bool,
}

impl RealSpeedFixMod {
    pub fn new() -> Self {
        Self {
            real_speed_ready: false,
            active: false,
        }
    }
}

impl Mod for RealSpeedFixMod {
    fn id(&self) -> &str {
        "real-speed-fix"
    }
    fn name(&self) -> &str {
        "Real Speed Calculation Fix"
    }
    fn description(&self) -> &str {
        "Real Speed display uses Core BPM instead of Max BPM"
    }
    fn required_signatures(&self) -> &[&str] {
        &[]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        self.real_speed_ready = real_speed::init(ctx.signatures);
        if !self.real_speed_ready {
            log_warn!("RealSpeedFix: real_speed_bpm_anchor missing — disabled");
        }
        true
    }

    fn enable(&mut self) {
        if self.real_speed_ready {
            real_speed::enable();
            self.active = true;
            log_info!("RealSpeedFix: enabled");
        }
    }

    fn disable(&mut self) {
        if self.active {
            real_speed::disable();
            self.active = false;
        }
        log_info!("RealSpeedFix: disabled");
    }
}
