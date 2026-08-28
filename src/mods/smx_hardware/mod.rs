//! SMX Hardware — native StepManiaX Dedicated Cabinet support.
//!
//! Talks directly to the SMX cabinet from inside the game process over raw
//! HID (the `services::smx` transport), replacing the external SpiceManiaX
//! app + SpiceAPI loopback. Step 1 scope (see
//! `.agents/planning/2026-08-27-native-smx-hardware-support/`):
//!
//! - **Input**: SMX stage panel sensors → the ark IO singleton's vtable
//!   panel getters (additive injection through `input_manager`'s seam,
//!   installed lazily once the game's IO layer is live — covers gameplay,
//!   the ark's panel counters, and the test menu; cabinet inputs keep
//!   working alongside).
//! - **Stage lights**: DDR's Gold-Cab light output (`arkMDXChangeTapeled`
//!   per-LED tape + `arkMDXChangeDimlamp` stage corners) → the SMX pads'
//!   9-panel LED grids at ~30 Hz, with SpiceManiaX's exact mapping.
//! - **Cabinet lights** (Step 2): the same captured frame's top-panel /
//!   monitor tape strips + woofer-corner dimlamps → the SMX Dedicated
//!   Cabinet's marquee, vertical strips, and spotlights (the separate
//!   "SMXArcade" HID controller, `L`/`Q` wire commands).
//!
//! Step 3 adds the touchscreen overlay (menu nav / pinpad / card-in).
//!
//! ## Lifecycle
//!
//! Default **OFF** (hardware-specific — see `DEFAULT_OFF_MODS`). `enable()`
//! installs the light-capture detours, starts the transport thread, and
//! activates input injection; `disable()` deactivates injection, gates off
//! capture, and stops the transport (detours stay installed per the repo's
//! one-detour rule — their bodies become passthrough). `is_active()`
//! reports false if the load-bearing pieces failed so the mod menu never
//! shows a false ON (NFR-4: no connected cabinet is NOT a failure — the
//! transport hot-plugs when one appears).

pub mod cabinet_force;
pub mod config;
pub mod input_inject;
pub mod lights_read;

use std::sync::atomic::{AtomicBool, Ordering};

use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::smx::transport;
use crate::{log_info, log_warn};

static ACTIVE: AtomicBool = AtomicBool::new(false);

pub struct SmxHardwareMod;

impl SmxHardwareMod {
    pub fn new() -> Self {
        Self
    }
}

impl Mod for SmxHardwareMod {
    fn id(&self) -> &str {
        "smx-hardware"
    }

    fn name(&self) -> &str {
        "SMX Hardware"
    }

    fn description(&self) -> &str {
        "Native StepManiaX cabinet support: pads as stage input, DDR lights on the pads"
    }

    fn required_signatures(&self) -> &[&str] {
        // Everything resolves via GetProcAddress on the ark module at
        // enable-time (loader-agnostic; no gamemdx signatures needed).
        &[]
    }

    fn init(&mut self, _ctx: &ModContext) -> bool {
        true
    }

    fn enable(&mut self) {
        let settings = config::load();

        // 0. Force GOLD cabinet mode so gamemdx drives the per-LED arrow
        //    tape (`arkMDXChangeTapeled`) + stage corners
        //    (`arkMDXChangeDimlamp`) instead of the SD satellite path. On
        //    this cabinet the game auto-detects a non-GOLD machine type and
        //    would otherwise emit only `arkMDXChangeSatellite` cabinet-light
        //    data (arrows never lit correctly). Best-effort: a miss degrades
        //    to the game's own cabinet mode with a WARN, it doesn't disable
        //    the mod. Installed first so it's live before gamemdx queries the
        //    machine type on its first light frame.
        if settings.force_gold_cabinet {
            if cabinet_force::install() {
                cabinet_force::set_force_enabled(true);
                // Source lights by polling the ark's GOLD output buffers so
                // the operator test-menu LAMP CHECK (ark-driven, bypasses the
                // arkMDX* exports) reaches the pads alongside gameplay.
                transport::set_poll_ark(true);
            } else {
                log_warn!(
                    "SmxHardware: could not force GOLD cabinet mode -- pad lights may be wrong"
                );
            }
        }

        // 1. Light-output capture (Tapeled + Dimlamp detours; once).
        if !lights_read::install() {
            log_warn!("SmxHardware: lights capture unavailable -- mod inactive");
            return;
        }
        lights_read::set_capture_enabled(true);

        // 2. The SMX transport thread (discovery, input reads, lights drain).
        if !transport::init(settings.output_lights, settings.output_cabinet_lights) {
            lights_read::set_capture_enabled(false);
            log_warn!("SmxHardware: transport failed to start -- mod inactive");
            return;
        }

        // 3. Stage input injection (SMX panels → the ark vtable panel
        //    getters; the detours install lazily at the first poll after
        //    the game's IO singleton goes live).
        input_inject::activate();

        ACTIVE.store(true, Ordering::Release);
        log_info!(
            "SmxHardware: enabled (output_lights={}, cabinet_lights={}, force_gold={}; cabinet hot-plugs at runtime)",
            settings.output_lights,
            settings.output_cabinet_lights,
            settings.force_gold_cabinet
        );
    }

    fn disable(&mut self) {
        input_inject::deactivate();
        lights_read::set_capture_enabled(false);
        cabinet_force::set_force_enabled(false);
        transport::set_poll_ark(false);
        transport::shutdown();
        ACTIVE.store(false, Ordering::Release);
        log_info!("SmxHardware: disabled (game input/lights back to stock)");
    }

    fn is_active(&self) -> bool {
        ACTIVE.load(Ordering::Acquire)
    }
}
