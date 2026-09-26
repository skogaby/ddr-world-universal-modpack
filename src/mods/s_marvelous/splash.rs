//! Full-combo splash override (design §4.6): an all-S-Marvelous MFC plays
//! the S-MARVELOUS splash segment instead of the stock MARVELOUS one.
//!
//! Mechanism: one post-original `GenericDetour` on the FullcomboActor
//! message handler (`fullcombo_actor_on_message` — the actor's ONLY
//! handled message is 0x1034, fired once at song end when a full combo was
//! achieved; payload dword = MFC type 0..3 computed inside judge_submit).
//! Post-original, for `msg == 0x1034 && type == 0 && combo_is_all_smarv`:
//! re-drive the splash clip's label to `s_marbelous_in` via `mc_op(0xF09)`
//! — the engine's child-redirect + per-clip label lookup (the Step-4
//! flash's proven op; all four templates carry the label in BOTH timeline
//! sections via the multi-shape clone). Stock already played
//! `se_game_fullcombo` and set play/visibility — only the goto is
//! re-driven.
//!
//! Fail-open: unresolved signature ⇒ no detour; un-patched templates ⇒ the
//! goto is a benign no-op on stock labels (0xF09 swallows misses — the
//! stock MFC splash shows).
//!
//! Targets: World's `dance_fullcombo_v3` (staged by [`activate`]) and, while
//! DDR SELECTION is enabled, each legacy skin with art ([`add_legacy`]). The
//! four patch closures pick the target by the song (`legacy_package` + the
//! armed skin — skins 2 / 3 and 4 / 5 ship byte-identical templates), then
//! byte-gate on that target's stock bytes. The legacy `marbelous_in`
//! segment plays A3's `XAC_full_combo2` itself; the placements-only clone
//! copies that DoAction, so `s_marbelous_in` sounds the same. A legacy
//! re-drive also needs THIS skin's template patched this session
//! ([`LEGACY_APPLIED`]); World's keeps its historic gate.
//!
//! Actor layout (display-side RE §5): `+0x88` side-info ptr (first dword =
//! side), `+0x98` splash clip wrapper.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;

use once_cell::sync::Lazy;
use retour::GenericDetour;

use crate::core::signatures::SignatureStore;
use crate::services::{afp_patcher, bm2d_api};
use crate::{log_info, log_warn};

use super::assets::StagedFcPatch;
use super::{assets, state, targets};

type FcOnMessageFn = unsafe extern "C" fn(*mut u8, u32, *mut u8) -> u64;

static DETOUR: once_cell::sync::OnceCell<GenericDetour<FcOnMessageFn>> =
    once_cell::sync::OnceCell::new();

/// Mod enabled with the four patch closures registered.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// World's splash templates/assets staged (set at the first successful
/// enable). World's re-drive declines without it (stock splash).
static WORLD_READY: AtomicBool = AtomicBool::new(false);
/// Bit per legacy skin ([`targets::skin_bit`]): one of its templates was
/// patched this session — the legacy re-drive's gate.
static LEGACY_APPLIED: AtomicU8 = AtomicU8::new(0);
/// Per legacy skin: its template differed from the staged one (one WARN).
static WARN_LEGACY_VARIANT: AtomicU8 = AtomicU8::new(0);
static FIRST_REDRIVE_LOGGED: AtomicBool = AtomicBool::new(false);
/// One-time patch registration latch (afp_patcher registrations persist).
static PATCHES_REGISTERED: AtomicBool = AtomicBool::new(false);
/// Every staged template patch, all targets.
static STAGED: Lazy<Mutex<Vec<StagedFcPatch>>> = Lazy::new(|| Mutex::new(Vec::new()));

const ACTOR_SIDE_INFO: usize = 0x88;
const ACTOR_SPLASH_WRAPPER: usize = 0x98;
const MSG_FULLCOMBO: u32 = 0x1034;
const LABEL: &std::ffi::CStr = c"s_marbelous_in";

pub fn install(signatures: &SignatureStore) -> bool {
    let Some(target) = signatures.get_address("fullcombo_actor_on_message") else {
        log_warn!("SMarvelous: fullcombo_actor_on_message unresolved — splash stays stock");
        return false;
    };
    let target: FcOnMessageFn = unsafe { std::mem::transmute(target) };
    match unsafe { GenericDetour::new(target, fc_message_hook) } {
        Ok(detour) => {
            if unsafe { detour.enable() }.is_err() {
                log_warn!("SMarvelous: splash detour enable failed — splash stays stock");
                return false;
            }
            let _ = DETOUR.set(detour);
            log_info!("SMarvelous: fullcombo splash detour installed");
            true
        }
        Err(e) => {
            log_warn!(
                "SMarvelous: splash detour failed: {:?} — splash stays stock",
                e
            );
            false
        }
    }
}

/// Stage World's splash assets + register the four template patches.
/// Called from enable. Staging runs once per boot (inputs can't change
/// mid-session; a failed World staging is retried on the next enable);
/// re-enable just re-arms the re-drive.
pub fn activate() {
    if !WORLD_READY.load(Ordering::Acquire) {
        let staged = assets::stage_fullcombo();
        if !staged.is_empty() {
            if let Ok(mut g) = STAGED.lock() {
                g.extend(staged);
            }
            WORLD_READY.store(true, Ordering::Release);
        }
        // (An empty result WARNed already; World's splash stays stock.)
    }
    if !PATCHES_REGISTERED.swap(true, Ordering::AcqRel) {
        for template in assets::FC_TEMPLATES {
            afp_patcher::register_patch(
                template,
                Box::new(move |afp: &[u8], _bsi: &[u8]| patch_template(template, afp)),
            );
        }
    }
    ACTIVE.store(true, Ordering::Release);
}

/// Stage a DDR SELECTION legacy skin's S-MFC splash (its
/// `dance_fullcombo000N` package). Idempotent per skin. `false` when the
/// splash surface is not active or staging failed (WARNed) — that skin's
/// full combos keep A3's MARVELOUS splash.
pub fn add_legacy(skin: u8) -> bool {
    if !ACTIVE.load(Ordering::Acquire) {
        return false;
    }
    if STAGED
        .lock()
        .map(|g| g.iter().any(|p| p.skin == skin))
        .unwrap_or(false)
    {
        return true;
    }
    let Some(target) = assets::legacy_target("dance_fullcombo", skin) else {
        log_warn!(
            "SMarvelous: no dance_fullcombo{:04} package on this install -- skin {} keeps A3's splash",
            skin,
            skin
        );
        return false;
    };
    let staged = assets::stage_fullcombo_for(&target);
    if staged.is_empty() {
        return false;
    }
    match STAGED.lock() {
        Ok(mut g) => {
            g.extend(staged);
            true
        }
        Err(_) => false,
    }
}

/// The afp_patcher callback for one splash template (all four share it).
/// Loading thread, descrambled bytes; Option-chained, panic-free.
fn patch_template(template: &'static str, afp: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    if !ACTIVE.load(Ordering::Acquire) {
        return None;
    }
    // World's package or this song's DDR SELECTION legacy skin (same
    // export names).
    let skin = targets::target_skin(
        crate::mods::ddr_selection::legacy_package("dance_fullcombo"),
        crate::mods::ddr_selection::armed_skin(),
    )?;
    let guard = STAGED.lock().ok()?;
    // No entry: World's staging WARNed at enable, or the legacy skin has no
    // art (logged at staging) — stream stock.
    let patch = guard
        .iter()
        .find(|p| p.skin == skin && p.template == template)?;
    if afp != patch.stock_bytes.as_slice() {
        if skin == 0 {
            log_warn!(
                "SMarvelous: {} variant differs from the staged template — streaming stock",
                template
            );
        } else if WARN_LEGACY_VARIANT.fetch_or(targets::skin_bit(skin), Ordering::Relaxed)
            & targets::skin_bit(skin)
            == 0
        {
            log_warn!(
                "SMarvelous: dance_fullcombo{:04} {} (skin {}) differs from the template staged at enable — A3's splash shows",
                skin,
                template,
                skin
            );
        }
        return None;
    }
    let mut doc = crate::core::ap2::Ap2Doc::parse(afp)?;
    let ids = doc.clone_segment_with_new_shapes(
        assets::FC_SRC_LABEL,
        assets::FC_NEW_LABEL,
        &patch.shape_ids,
    )?;
    if ids != patch.expected {
        log_warn!(
            "SMarvelous: {} allocated ids diverged from staging — streaming stock",
            template
        );
        return None;
    }
    let out = doc.serialize()?;
    if skin == 0 {
        log_info!(
            "SMarvelous: {} patched ({} -> {} bytes, {})",
            template,
            afp.len(),
            out.len(),
            assets::FC_NEW_LABEL
        );
    } else {
        LEGACY_APPLIED.fetch_or(targets::skin_bit(skin), Ordering::AcqRel);
        log_info!(
            "SMarvelous: dance_fullcombo{:04} {} (skin {}) patched ({} -> {} bytes, {})",
            skin,
            template,
            skin,
            afp.len(),
            out.len(),
            assets::FC_NEW_LABEL
        );
    }
    Some((out, vec![0u8; 2]))
}

pub fn deactivate() {
    ACTIVE.store(false, Ordering::Release);
}

pub fn reset_latches() {
    FIRST_REDRIVE_LOGGED.store(false, Ordering::Relaxed);
}

unsafe extern "C" fn fc_message_hook(actor: *mut u8, msg: u32, payload: *mut u8) -> u64 {
    let ret = match DETOUR.get() {
        Some(detour) => detour.call(actor, msg, payload),
        None => 0,
    };
    if let Err(e) = std::panic::catch_unwind(|| redrive_if_smfc(actor, msg, payload)) {
        let _ = e;
    }
    ret
}

/// Whether this song's splash template carries `s_marbelous_in`: World's
/// staged (historic gate), or the armed legacy skin's patched this session.
fn splash_ready() -> bool {
    if !ACTIVE.load(Ordering::Acquire) {
        return false;
    }
    match targets::target_skin(
        crate::mods::ddr_selection::legacy_package("dance_fullcombo"),
        crate::mods::ddr_selection::armed_skin(),
    ) {
        Some(0) => WORLD_READY.load(Ordering::Acquire),
        Some(skin) => LEGACY_APPLIED.load(Ordering::Acquire) & targets::skin_bit(skin) != 0,
        None => false,
    }
}

fn redrive_if_smfc(actor: *mut u8, msg: u32, payload: *mut u8) {
    if msg != MSG_FULLCOMBO || actor.is_null() || payload.is_null() || !splash_ready() {
        return;
    }
    unsafe {
        // MFC type 0 (all-Marvelous) only — anything else already shows a
        // lower-tier splash.
        let mfc_type = (payload as *const i32).read_unaligned();
        if mfc_type != 0 {
            return;
        }
        let side_info = (actor.add(ACTOR_SIDE_INFO) as *const *const u8).read_unaligned();
        if side_info.is_null() {
            return;
        }
        let side = (side_info as *const i32).read_unaligned();
        if !(0..=1).contains(&side) || !state::combo_is_all_smarv(side as usize) {
            return;
        }
        let wrapper = (actor.add(ACTOR_SPLASH_WRAPPER) as *const *const u8).read_unaligned();
        if wrapper.is_null() {
            return;
        }
        let mc_id = (wrapper.add(0x110) as *const u32).read_unaligned();
        if mc_id < 1 {
            return;
        }
        if bm2d_api::mc_op_str(mc_id, 0xF09, LABEL)
            && !FIRST_REDRIVE_LOGGED.swap(true, Ordering::Relaxed)
        {
            log_info!(
                "SMarvelous: S-MFC splash re-drive (side {}, mc {:08X}{})",
                side,
                mc_id,
                match crate::mods::ddr_selection::armed_skin() {
                    s if s > 0 && crate::mods::ddr_selection::legacy_package("dance_fullcombo") =>
                        format!(", DDR SELECTION skin {}", s),
                    _ => String::new(),
                }
            );
        }
    }
}
