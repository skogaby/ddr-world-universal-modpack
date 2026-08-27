//! Announcer Mute — Silences the in-game announcer during gameplay.
//!
//! DDR World's announcer audibly calls out combo milestones
//! (`vo_ingame_combo_%04d` / `vo_ingame_combo_other`), score-state
//! accolades (`vo_ingame_state_NN_*`) and plays the stage-clear cheer SFX
//! (`se_kansei_big/middle/small`). Many players find the callouts
//! distracting; this mod lets each player mute them from the in-game
//! options menu between songs.
//!
//! ## Mechanism
//!
//! One `GenericDetour` on the announcer/voice dispatcher — the single
//! per-frame function that plays every family above (RE:
//! `docs/hex_edit_porting.md` Hack 1; AOB `announcer_dispatcher` in
//! `signatures.rs`, unique on all four supported builds). When mute is
//! effective the callback returns without calling the original — the whole
//! announcer body is skipped, exactly the effect of the original hex
//! edit's entry-guard patch, but AOB-resolved and toggleable at runtime.
//! When not muted the original runs unmodified.
//!
//! The mute state is read live from atomics on every dispatch, so an
//! options-menu toggle between songs takes effect on the next song with no
//! latch step (the dispatcher only fires during gameplay).
//!
//! ## P1 precedence
//!
//! The dispatcher is a single cabinet-wide instance (it reads BOTH sides'
//! combo counters via `max()`), so the mute is inherently global — it
//! cannot be muted for one side and audible for the other. Policy:
//! whichever side is carded in governs; when both sides are entered
//! (versus), P1's choice wins. If the entered state is unavailable
//! (`stage_records` down) the effective mute falls back to `p1 || p2`.
//!
//! ## Persistence
//!
//! `PersistMode::Full`: the per-side value round-trips through the
//! `custom_options.p1/p2` JSON cache in `mod-config.json` and is emitted
//! on the network save as `mod_announcer_mute`. Server-side storage needs
//! a matching bemani-buddy column/migration (not part of this repo); until
//! then the JSON cache carries the value across sessions.
//!
//! ## Degradation
//!
//! `announcer_dispatcher` is in `required_signatures`, so a signature miss
//! skips the mod cleanly (stock announcer, no option row). A hook-install
//! failure logs one WARN and registers no option row.

use std::ffi::c_void;
use std::ptr::addr_of;
use std::sync::atomic::{AtomicBool, Ordering};

use retour::GenericDetour;

use crate::core::hooks;
use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::custom_options::{self, RegisterSpec};
use crate::services::stage_records;
use crate::{log_info, log_warn};

/// Per-side mute choices, written by the option row's change callback and
/// read live by the dispatcher hook.
static MUTE_ENABLED: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];

/// True once the detour is installed (drives `is_active()` and gates the
/// option-row registration).
static HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);

type AnnouncerFn = unsafe extern "C" fn(*mut c_void);

static mut ANNOUNCER_HOOK: Option<GenericDetour<AnnouncerFn>> = None;

/// Cabinet-wide effective mute. Entered side governs; P1 wins when both
/// sides are in. Falls back to `p1 || p2` when entered state is unknown.
fn effective_mute() -> bool {
    let p1 = MUTE_ENABLED[0].load(Ordering::Acquire);
    let p2 = MUTE_ENABLED[1].load(Ordering::Acquire);
    match (
        stage_records::side_entered(0),
        stage_records::side_entered(1),
    ) {
        (Some(true), _) => p1,
        (Some(false), Some(true)) => p2,
        (Some(false), Some(false)) => false,
        _ => p1 || p2,
    }
}

unsafe extern "C" fn announcer_hook(this: *mut c_void) {
    let Some(hook) = (&*addr_of!(ANNOUNCER_HOOK)).as_ref() else {
        return;
    };
    // `effective_mute` is panic-free (atomics + guarded pointer reads); the
    // muted branch is a plain return — the whole announcer body is skipped.
    if effective_mute() {
        return;
    }
    hook.call(this);
}

fn mute_on_change(player_side: u8, new_value: i32) {
    if player_side < 2 {
        let enabled = new_value != 0;
        MUTE_ENABLED[player_side as usize].store(enabled, Ordering::Release);
        log_info!(
            "AnnouncerMute: side={} {}",
            player_side,
            if enabled { "ON" } else { "OFF" }
        );
    }
}

pub struct AnnouncerMuteMod {
    dispatcher_addr: *const u8,
}

unsafe impl Send for AnnouncerMuteMod {}

impl AnnouncerMuteMod {
    pub fn new() -> Self {
        Self {
            dispatcher_addr: std::ptr::null(),
        }
    }
}

impl Mod for AnnouncerMuteMod {
    fn id(&self) -> &str {
        "announcer-mute"
    }

    fn name(&self) -> &str {
        "Announcer Mute"
    }

    fn description(&self) -> &str {
        "Per-player option to mute the in-game announcer (combo callouts, accolades, cheers)"
    }

    fn required_signatures(&self) -> &[&str] {
        &["announcer_dispatcher"]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        self.dispatcher_addr = ctx.signatures.require_address("announcer_dispatcher");
        true
    }

    fn enable(&mut self) {
        if HOOK_INSTALLED.load(Ordering::Acquire) {
            return;
        }
        let target: AnnouncerFn = unsafe { std::mem::transmute(self.dispatcher_addr) };
        if let Err(error) = unsafe {
            hooks::install_enabled(
                std::ptr::addr_of_mut!(ANNOUNCER_HOOK),
                target,
                announcer_hook,
            )
        } {
            log_warn!(
                "AnnouncerMute: dispatcher hook installation failed: {} -- mod inactive",
                error
            );
            return;
        }
        HOOK_INSTALLED.store(true, Ordering::Release);

        if custom_options::is_available() {
            let spec = RegisterSpec::bool_toggle("announcer_mute")
                .display_name("Announcer Mute")
                .description("Silences the announcer's combo callouts and stage cheers")
                .default_value(0)
                .on_change(mute_on_change);
            match custom_options::register_option(spec) {
                Ok(_handle) => {
                    log_info!("AnnouncerMute: registered custom option on Mods tab");
                }
                Err(e) => {
                    log_warn!("AnnouncerMute: custom option registration failed: {e}");
                }
            }
        } else {
            log_warn!(
                "AnnouncerMute: custom_options service unavailable -- option row will not render"
            );
        }

        log_info!("AnnouncerMute: enabled (announcer dispatcher hooked, per-player toggle)");
    }

    fn disable(&mut self) {
        // The detour stays installed (one detour per target, never
        // uninstalled at runtime); clearing both side flags makes the
        // callback pass every call straight through to the original.
        MUTE_ENABLED[0].store(false, Ordering::Release);
        MUTE_ENABLED[1].store(false, Ordering::Release);
        log_info!("AnnouncerMute: disabled (announcer passthrough)");
    }

    fn is_active(&self) -> bool {
        HOOK_INSTALLED.load(Ordering::Acquire)
    }
}
