//! Present-mode policy detour (design §4.7 second half / R4, R5).
//!
//! `letterbox_rect_fn(this, int mode)` is the engine's own scaler for a
//! screen that is not 1280 wide: `screen_w == <render_w>` ⇒ 1:1 POINT copy;
//! `mode 1` ⇒ 960-px centre crop (what SD cabinets shipped with); anything
//! else ⇒ width-fit letterbox (full-screen for a matching aspect). The game
//! calls it with **mode 1 at every scene transition** and with mode 0 when
//! entering the TEST menu, so an operator-chosen mode has to be re-asserted
//! on every call — that is this detour: `mode' = plan::present_mode(policy,
//! mode)`, then the original.
//!
//! - `Sd(Crop)`: identity (stock SD behaviour).
//! - `Sd(Letterbox)`: 1 → 0 (the operator wants the full HUD on a 4:3 CRT);
//!   the TEST menu's own 0 is untouched.
//! - `ForceLetterbox` (16:9 output, smaller/larger render): every request →
//!   0, because the crop rect is hard-coded for a 1280-wide SOURCE and would
//!   chop a 16:9 picture that merely needs scaling.
//! - `Stock`: the detour is not installed at all.

use std::ptr::addr_of;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use retour::GenericDetour;

use crate::core::hooks;
use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

use super::plan::{present_mode, PresentPolicy, SdPresent};

type LetterboxFn = unsafe extern "C" fn(*mut u8, i32);

static mut DETOUR: Option<GenericDetour<LetterboxFn>> = None;
static INSTALLED: AtomicBool = AtomicBool::new(false);
static LOGGED: AtomicBool = AtomicBool::new(false);

const POLICY_STOCK: u8 = 0;
const POLICY_FORCE_LETTERBOX: u8 = 1;
const POLICY_SD_CROP: u8 = 2;
const POLICY_SD_LETTERBOX: u8 = 3;
static POLICY: AtomicU8 = AtomicU8::new(POLICY_STOCK);

fn encode(p: PresentPolicy) -> u8 {
    match p {
        PresentPolicy::Stock => POLICY_STOCK,
        PresentPolicy::ForceLetterbox => POLICY_FORCE_LETTERBOX,
        PresentPolicy::Sd(SdPresent::Crop) => POLICY_SD_CROP,
        PresentPolicy::Sd(SdPresent::Letterbox) => POLICY_SD_LETTERBOX,
    }
}

fn decode(v: u8) -> PresentPolicy {
    match v {
        POLICY_FORCE_LETTERBOX => PresentPolicy::ForceLetterbox,
        POLICY_SD_CROP => PresentPolicy::Sd(SdPresent::Crop),
        POLICY_SD_LETTERBOX => PresentPolicy::Sd(SdPresent::Letterbox),
        _ => PresentPolicy::Stock,
    }
}

/// Install for a non-`Stock` policy. `Stock` and `Sd(Crop)` need no detour
/// (both are the identity mapping) and return `Ok` without installing.
pub fn install(sigs: &SignatureStore, policy: PresentPolicy) -> Result<(), String> {
    POLICY.store(encode(policy), Ordering::Release);
    if matches!(
        policy,
        PresentPolicy::Stock | PresentPolicy::Sd(SdPresent::Crop)
    ) {
        return Ok(());
    }
    if INSTALLED.load(Ordering::Acquire) {
        return Ok(());
    }
    let target = sigs
        .get_address("letterbox_rect_fn")
        .ok_or("letterbox_rect_fn unresolved")?;
    unsafe {
        let target: LetterboxFn = std::mem::transmute(target);
        hooks::install_enabled(
            std::ptr::addr_of_mut!(DETOUR),
            target,
            letterbox_detour as LetterboxFn,
        )
        .map_err(|e| format!("letterbox_rect_fn detour install failed: {e}"))?;
    }
    INSTALLED.store(true, Ordering::Release);
    log_info!(
        "CustomResolution: present-mode detour installed ({:?})",
        policy
    );
    Ok(())
}

unsafe extern "C" fn letterbox_detour(this: *mut u8, mode: i32) {
    let policy = decode(POLICY.load(Ordering::Acquire));
    let mapped = present_mode(policy, mode);
    if mapped != mode && !LOGGED.swap(true, Ordering::AcqRel) {
        log_info!(
            "CustomResolution: present mode {} -> {} ({:?}); further remaps are silent",
            mode,
            mapped,
            policy
        );
    }
    if let Some(hook) = (*addr_of!(DETOUR)).as_ref() {
        hook.call(this, mapped);
    } else {
        log_warn!("CustomResolution: letterbox detour called without its original -- skipped");
    }
}
