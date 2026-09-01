//! Real Speed Calculation Fix — Real Speed display divides by Core BPM
//! instead of Max BPM (the R24/R25/R26 divisor-swap patches in
//! `real_speed.rs`).
//!
//! # Retired: the logf(0) guard (R15/R16), 2026-09-01
//!
//! The original hex-edit modpack shipped two additional patches attributed
//! by its research notes to "the scroll-speed display function": R16
//! (redirect a log call through a zero-guarded wrapper) and R15 (a JMP
//! rel8 displacement rewrite `0x48 → 0x37`). Porting them here as
//! `logf_stub.rs` reproduced both faithfully — and R15 turned out to be
//! the root cause of the "pacemaker → ms-error shows no digit at exactly
//! 0" bug: the `real_speed_logf_anchor` AOB actually lands inside
//! `NoteResultActor::onMessage` case 0x1036 (the PACEMAKER readout, not
//! any scroll-speed code — the attribution was wrong in the original
//! mod's notes, verified on 20250805/20260616/20260721/20260825, single
//! match in the same function on all four). R15 rewrote the zero-branch's
//! `LEA R13D,[RSI+1]; JMP +0x48` to jump into the log10f path instead,
//! recomputing the sign-slot index as `trunc(guarded_log(0) + XMM6)` with
//! XMM6 STALE (only the nonzero branch loads it with 1.0f) — observed 0 at
//! runtime → sign slot = powf(10,0) = 1 = the ONES slot, so the ± sign
//! overwrote the 0 digit (live-confirmed via CE register captures:
//! R13D=0, R9D=1 despite the LEA provably setting R13D=1).
//!
//! The guard itself is also useless in this codebase's flow: the nonzero
//! branch only reaches log10f with |value| ≥ 1, and stock's zero branch
//! never calls it. Both patches removed outright; the Core-BPM divisor
//! swap below is the entire mod.

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
