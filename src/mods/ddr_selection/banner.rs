//! The legacy end banners: A3's CLEARED / FAILED / PRAY FOR ALL, hosted in
//! World's ShutterActor banner kinds (RE `.agents/planning/`
//! `2026-09-22-ddr-selection/research/end-banners-sel-movies.md`; pure rules
//! in [`banner_logic`]).
//!
//! World requests kind CLEARED (a side alive) or FAILED (all dead) at the
//! song end (DancePlaySequence step 8; stage kind + 1 / + 2 on every build)
//! and shows one root clip per kind — nothing reads the clip's children.
//! A3 showed a root and an overlay from the era package
//! `common_shutter000N`. Mechanism (the Step 5 stage-panel pattern, on the
//! same `ShutterActor::onUpdate` detour — `panel.rs` calls in):
//!
//! * **Row patch, for one update only**: pre-original of the update whose
//!   state 0 reads a banner kind's row (the kind-art loader's only read of
//!   the table) while a legacy skin is armed, the row becomes `{pkg
//!   "common_shutter000N", root "shutter_clear" / "shutter_failed", SE in
//!   "", SE out "", voice in ""}` — World's own named-package path then
//!   loads the era package and creates A3's root; the legacy clips carry
//!   their own sounds (World's `se_game_clear` / `se_game_failed` /
//!   `vo_stage_clear` would double them). Restored right after the original
//!   (the loader copies the row; every patched pointer is static).
//! * **Overlay**: our own AFP layer from World's copy of that package
//!   (`bm2d_package::lookup_unowned`, re-validated each update — never a
//!   ticket: World's request would dedup onto our entry and World's
//!   refcount-less release would destroy it under us). Group 5, raw priority
//!   98 (A3 `SetPriority(2)`, on top of the root's 97); created parked when
//!   World's pending clip exists, `in` at World's swap, `out` when World
//!   plays the root's `out`, destroyed at its own `end` — and, as a safety
//!   net, pre-original of the update in which World is about to release the
//!   package (state 8, root at its release frame).
//! * **Lifetime**: the banner outlives the armed window (requested in
//!   gameplay, covered through the post-song loader, opened by the results
//!   — or later, after a quick-fail skip). The session is driven by the
//!   ShutterActor's own states only, never by the disarm.
//!
//! Fail-open: without the rows, the package, or a package state we can
//! trust, World's banner stays (one WARN per cause).

use std::ffi::CStr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::core::memory;
use crate::core::signatures::DdrSelBannerRows;
use crate::services::bm2d_api::{self, AfpLayer};
use crate::services::{bm2d_package, shutter};
use crate::{log_info, log_warn};

use super::banner_logic::{self as logic, Action, Art, Banner, Frame, Outcome, OverlayView};

const MC_OP_GOTO_LABEL_PLAY: i32 = 0xF09;
/// The standard post-create display attribute; bit 0 = visible.
const ATTR_DISPLAY_SETUP: u32 = 0x200;
const OVERLAY_GROUP: u16 = 5;
/// Row fields patched: pkg, root, SE in, SE out, voice in (every layout).
const PATCHED_FIELDS: usize = 5;

static BM2D_DIR: &CStr = c"bm2d";
/// Static: the loader keeps the row's POINTERS until its done callback.
static PACKAGES: [&CStr; 5] = [
    c"common_shutter0001",
    c"common_shutter0002",
    c"common_shutter0003",
    c"common_shutter0004",
    c"common_shutter0005",
];
static ROOT_CLEAR: &CStr = c"shutter_clear";
static ROOT_FAILED: &CStr = c"shutter_failed";

struct RowsSync {
    rows: DdrSelBannerRows,
    /// Stock pointers of the first [`PATCHED_FIELDS`] fields.
    cleared_stock: [usize; PATCHED_FIELDS],
    failed_stock: [usize; PATCHED_FIELDS],
}
unsafe impl Send for RowsSync {}
unsafe impl Sync for RowsSync {}

static ROWS: OnceLock<RowsSync> = OnceLock::new();
/// The current scene is GAMEPLAY (scene callback): only the song end's own
/// request (DancePlaySequence step 8) is hosted — ResultSequence requests a
/// CLEARED kind again at the end of the results (`FUN_1800bc120` case
/// `0x20`), right after World released the first banner's package, and a
/// re-request of that name during its deferred destroy is the known crash
/// class.
static IN_GAMEPLAY: AtomicBool = AtomicBool::new(false);
/// A banner session exists (lock-free for the update detour's watch).
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Our overlay or A3's root is on screen (the AFP sound route also routes).
static LIVE: AtomicBool = AtomicBool::new(false);
static WARNED: AtomicU32 = AtomicU32::new(0);
const W_PACKAGE: u32 = 1;
const W_HELD: u32 = 2;
const W_ROW: u32 = 4;
const W_OVERLAY: u32 = 8;
const W_SAFETY: u32 = 16;
const W_RESIDENT: u32 = 32;
const W_BUSY: u32 = 64;

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

struct Overlay {
    layer: AfpLayer,
    mc: u32,
    end: Option<u32>,
}

struct Session {
    skin: u8,
    outcome: Outcome,
    kind: i32,
    art: Art,
    package: &'static CStr,
    machine: Banner,
    overlay: Option<Overlay>,
    /// World's root was created from our row (the era package is resident
    /// under our name at its creation).
    legacy_root: bool,
    /// World's root (created from our row), once seen.
    root_mc: Option<u32>,
    root_target: Option<u32>,
}

unsafe impl Send for Session {}

fn warn_once(bit: u32) -> bool {
    WARNED.fetch_or(bit, Ordering::Relaxed) & bit == 0
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

/// Read the stock rows (mod init, from `panel::init`).
pub fn init(rows: Option<DdrSelBannerRows>) {
    let Some(rows) = rows else {
        log_warn!("DDR SELECTION: CLEARED / FAILED rows unresolved -- World's end banners stay");
        return;
    };
    let read = |row: *const u8| -> [usize; PATCHED_FIELDS] {
        let p = row as *const usize;
        let mut out = [0usize; PATCHED_FIELDS];
        for (i, v) in out.iter_mut().enumerate() {
            *v = unsafe { *p.add(i) };
        }
        out
    };
    let _ = ROWS.set(RowsSync {
        rows,
        cleared_stock: read(rows.cleared_row),
        failed_stock: read(rows.failed_row),
    });
    log_info!(
        "DDR SELECTION: legacy end banners ready (CLEARED kind {}, FAILED kind {})",
        rows.cleared_kind,
        rows.failed_kind
    );
}

/// Scene callback (game thread, before `createNextSequence`).
pub fn on_scene_change(next: i32) {
    IN_GAMEPLAY.store(
        next == crate::types::scenes::scene::GAMEPLAY,
        Ordering::Release,
    );
}

/// A session exists (the detour must watch every update).
pub fn active() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

/// A3's banner (root or overlay) is on screen — lock-free, also read from
/// libafp's display pass by the sound route.
pub fn live() -> bool {
    LIVE.load(Ordering::Acquire)
}

/// The legacy banner package + art for this song end, or `None` (World's).
fn resolve(outcome: Outcome) -> Option<(u8, Art, &'static CStr)> {
    let skin = super::armed_skin();
    if skin == 0 {
        return None;
    }
    let art = logic::art(skin, outcome, super::armed_mcode())?;
    let package = *PACKAGES.get(skin as usize - 1)?;
    Some((skin, art, package))
}

fn root_name(art: &Art) -> &'static CStr {
    if art.root == "shutter_failed" {
        ROOT_FAILED
    } else {
        ROOT_CLEAR
    }
}

/// Pre-original (every update while watched). Hosts a legacy banner when
/// this update is about to read a banner row, and runs the safety net.
/// `Some(kind)` = that kind's row was patched (restore it post-original).
///
/// # Safety
/// `actor` is the ShutterActor inside its own update (game thread).
pub unsafe fn before_update(actor: *mut u8, pre: &shutter::Snapshot) -> Option<i32> {
    if active() {
        safety_net(actor, pre);
    }
    if pre.state != super::panel_logic::ST_IDLE {
        return None;
    }
    let rows = ROWS.get()?;
    let outcome = logic::outcome_of(
        pre.pending_kind,
        rows.rows.cleared_kind,
        rows.rows.failed_kind,
    )?;
    if !IN_GAMEPLAY.load(Ordering::Acquire) {
        return None;
    }
    let (skin, art, package) = resolve(outcome)?;
    let pkg_name = package.to_str().unwrap_or("");
    if active() {
        if warn_once(W_BUSY) {
            log_warn!(
                "DDR SELECTION: a {} banner was requested while the previous legacy banner is still live -- World's banner",
                outcome.name()
            );
        }
        return None;
    }
    // The named-package path's done callback int3s on a missing package.
    if !super::package_helper::probe_arc(BM2D_DIR, package) {
        if warn_once(W_PACKAGE) {
            log_warn!("DDR SELECTION: {pkg_name} not found -- World's end banners stay");
        }
        return None;
    }
    // World's release has no refcount: it must be the package's only owner.
    if bm2d_package::held_by_tickets(pkg_name) {
        if warn_once(W_HELD) {
            log_warn!(
                "DDR SELECTION: {pkg_name} is still held by the stage panel -- World's {} banner",
                outcome.name()
            );
        }
        return None;
    }
    if bm2d_package::lookup_unowned(package).is_some() {
        if warn_once(W_RESIDENT) {
            log_warn!(
                "DDR SELECTION: {pkg_name} is already resident (another owner) -- World's {} banner",
                outcome.name()
            );
        }
        return None;
    }
    if !patch_row(outcome, Some((package, root_name(&art)))) {
        return None;
    }
    *lock(&SESSION) = Some(Session {
        skin,
        outcome,
        kind: pre.pending_kind,
        art,
        package,
        machine: Banner::new(),
        overlay: None,
        legacy_root: false,
        root_mc: None,
        root_target: None,
    });
    ACTIVE.store(true, Ordering::Release);
    log_info!(
        "DDR SELECTION: legacy {} banner hosted (skin {}: {} / {} + {}{})",
        outcome.name(),
        skin,
        pkg_name,
        art.root,
        art.overlay.clip(),
        if art.overlay == logic::Overlay::PrayForAll {
            ", PRAY FOR ALL (Tohoku EVOLVED)"
        } else if art.pray_fallback {
            " -- Tohoku EVOLVED, but this era has no PRAY FOR ALL art"
        } else {
            ""
        }
    );
    Some(pre.pending_kind)
}

/// Post-original of the update that read our row: restore it and say what
/// World did with it.
///
/// # Safety
/// As [`before_update`].
pub unsafe fn after_row_read(actor: *mut u8, kind: i32) {
    let Some(rows) = ROWS.get() else {
        return;
    };
    let outcome = if kind == rows.rows.cleared_kind {
        Outcome::Cleared
    } else {
        Outcome::Failed
    };
    patch_row(outcome, None);
    let post = shutter::snapshot_of(actor).ok();
    log_info!(
        "DDR SELECTION: World's kind-art loader read the {} row (SE / voice blanked) -- shutter now state {:?}, pending kind {:?}; row restored",
        outcome.name(),
        post.map(|p| p.state),
        post.map(|p| p.pending_kind)
    );
}

/// A banner row: `Some((package, root))` = the legacy row, `None` = stock.
/// Checked patch of the first [`PATCHED_FIELDS`] pointers.
fn patch_row(outcome: Outcome, legacy: Option<(&'static CStr, &'static CStr)>) -> bool {
    let Some(rows) = ROWS.get() else {
        return false;
    };
    let (row, stock) = match outcome {
        Outcome::Cleared => (rows.rows.cleared_row, rows.cleared_stock),
        Outcome::Failed => (rows.rows.failed_row, rows.failed_stock),
    };
    // Field 3 (SE out) is the row's own "" (gated at derivation).
    let empty = stock[3];
    let ours = |(pkg, root): (&CStr, &CStr)| -> [usize; PATCHED_FIELDS] {
        [
            pkg.as_ptr() as usize,
            root.as_ptr() as usize,
            empty,
            empty,
            empty,
        ]
    };
    let bytes = |v: [usize; PATCHED_FIELDS]| -> Vec<u8> {
        v.iter().flat_map(|p| p.to_le_bytes()).collect()
    };
    let (from, to, what) = match legacy {
        Some(l) => (bytes(stock), bytes(ours(l)), "apply"),
        None => {
            // Restore whatever the legacy pointers were (read back: the
            // package differs per skin).
            let cur = unsafe {
                let p = row as *const usize;
                let mut v = [0usize; PATCHED_FIELDS];
                for (i, x) in v.iter_mut().enumerate() {
                    *x = *p.add(i);
                }
                v
            };
            if cur == stock {
                return true;
            }
            (bytes(cur), bytes(stock), "restore")
        }
    };
    match unsafe { memory::apply_checked_patch(row as *mut u8, &from, &to) } {
        Ok(()) => true,
        Err(e) => {
            if warn_once(W_ROW) {
                log_warn!(
                    "DDR SELECTION: {} row patch ({}) failed: {:?}",
                    outcome.name(),
                    what,
                    e
                );
            }
            false
        }
    }
}

/// Pre-original: World releases the package in the update where the root
/// reaches its release frame — a still-live overlay must go first.
unsafe fn safety_net(actor: *mut u8, pre: &shutter::Snapshot) {
    let mut guard = lock(&SESSION);
    let Some(s) = guard.as_mut() else {
        return;
    };
    if s.overlay.is_none() {
        return;
    }
    let root_frame =
        shutter::kind_clip(actor, s.kind).and_then(|c| bm2d_api::mc_current_frame(c.mc));
    if !logic::must_destroy_before_release(
        pre.state,
        pre.active_kind == s.kind,
        root_frame,
        s.root_target,
    ) {
        return;
    }
    let frame = s
        .overlay
        .as_ref()
        .and_then(|o| bm2d_api::mc_current_frame(o.mc));
    destroy_overlay(s);
    s.machine.overlay_destroyed();
    if warn_once(W_SAFETY) {
        log_warn!(
            "DDR SELECTION: legacy banner overlay destroyed before World's release (safety net: root frame {:?} / {:?}, overlay frame {:?})",
            root_frame,
            s.root_target,
            frame
        );
    }
}

/// Post-original (every update while a session exists).
///
/// # Safety
/// As [`before_update`].
pub unsafe fn after_update(
    actor: *mut u8,
    pre: Option<shutter::Snapshot>,
    post: Option<shutter::Snapshot>,
) {
    let mut guard = lock(&SESSION);
    let Some(s) = guard.as_mut() else {
        return;
    };
    let Some(post) = post else {
        return;
    };
    let frame = Frame {
        pre_state: pre.map_or(post.state, |p| p.state),
        post_state: post.state,
        pending_is_ours: post.pending_kind == s.kind,
        active_is_ours: post.active_kind == s.kind,
        overlay: s.overlay.as_ref().and_then(|o| {
            bm2d_api::mc_current_frame(o.mc).map(|frame| OverlayView { frame, end: o.end })
        }),
    };
    for action in s.machine.advance(&frame) {
        perform(s, actor, action);
    }
    let root_shown =
        s.legacy_root && frame.active_is_ours && post.state >= super::panel_logic::ST_IN;
    LIVE.store(s.overlay.is_some() || root_shown, Ordering::Release);
    if s.machine.phase() == logic::Phase::Done {
        *guard = None;
        ACTIVE.store(false, Ordering::Release);
        LIVE.store(false, Ordering::Release);
    }
}

unsafe fn perform(s: &mut Session, actor: *mut u8, action: Action) {
    match action {
        Action::CreateOverlay => {
            if !create_overlay(s, actor) {
                s.machine.overlay_failed();
            }
        }
        Action::StartOverlay => {
            if let Some(o) = &s.overlay {
                bm2d_api::mc_op_str(o.mc, MC_OP_GOTO_LABEL_PLAY, c"in");
                bm2d_api::layer_play(&o.layer, 1.0);
                bm2d_api::layer_set_visible(&o.layer, true);
                log_info!(
                    "DDR SELECTION: legacy {} banner in (skin {}, {} over {})",
                    s.outcome.name(),
                    s.skin,
                    s.art.overlay.clip(),
                    s.art.root
                );
            }
        }
        Action::OverlayOut => {
            if let Some(o) = &s.overlay {
                bm2d_api::mc_op_str(o.mc, MC_OP_GOTO_LABEL_PLAY, c"out");
                log_info!(
                    "DDR SELECTION: legacy {} banner out (World opened it)",
                    s.outcome.name()
                );
            }
        }
        Action::DestroyOverlay => {
            let frame = s
                .overlay
                .as_ref()
                .and_then(|o| bm2d_api::mc_current_frame(o.mc));
            if destroy_overlay(s) {
                log_info!(
                    "DDR SELECTION: legacy banner overlay {} destroyed (frame {:?})",
                    s.art.overlay.clip(),
                    frame
                );
            }
        }
        Action::Finished => {
            log_info!(
                "DDR SELECTION: legacy {} banner released by World",
                s.outcome.name()
            );
        }
    }
}

/// World's pending clip exists: create our overlay from World's copy of the
/// era package, parked invisible at frame 0.
unsafe fn create_overlay(s: &mut Session, actor: *mut u8) -> bool {
    let name = s.package.to_str().unwrap_or("?");
    let Some(root) = shutter::kind_clip(actor, s.kind) else {
        if warn_once(W_OVERLAY) {
            log_warn!(
                "DDR SELECTION: no {} banner clip in World's slot -- no overlay",
                s.outcome.name()
            );
        }
        return false;
    };
    // World loaded the package under our row's name: only then is the root
    // A3's (in modes with their own kind tables World never read our row).
    let Some(pkg) = bm2d_package::lookup_unowned(s.package) else {
        if warn_once(W_OVERLAY) {
            log_warn!(
                "DDR SELECTION: {name} is not in the package registry at the swap -- World's {} banner, no overlay",
                s.outcome.name()
            );
        }
        return false;
    };
    s.legacy_root = true;
    s.root_mc = Some(root.mc);
    s.root_target = Some(logic::release_target(
        bm2d_api::mc_frame_by_label(root.mc, c"out_end"),
        bm2d_api::mc_frame_by_label(root.mc, c"end"),
    ))
    .filter(|&t| t > 0);
    let Some(layer) =
        bm2d_api::create_layer_from_package(pkg.afpu_package_id, s.art.overlay.clip())
    else {
        if warn_once(W_OVERLAY) {
            log_warn!(
                "DDR SELECTION: could not create {} from {name} -- A3's root without its overlay",
                s.art.overlay.clip()
            );
        }
        return false;
    };
    bm2d_api::layer_set_attribute(&layer, ATTR_DISPLAY_SETUP, ATTR_DISPLAY_SETUP);
    bm2d_api::layer_set_group(&layer, OVERLAY_GROUP);
    bm2d_api::layer_set_priority(
        &layer,
        super::panel_logic::clayer_priority(logic::OVERLAY_PRIORITY),
    );
    bm2d_api::layer_play(&layer, 0.0);
    bm2d_api::layer_set_visible(&layer, false);
    let Some(mc) = bm2d_api::layer_find_child(layer.id(), "/") else {
        let _ = bm2d_api::destroy_layer(layer);
        return false;
    };
    let end = bm2d_api::mc_frame_by_label(mc, c"end").filter(|&f| f > 0);
    log_info!(
        "DDR SELECTION: legacy banner overlay {} created (layer 0x{:08X}, end {:?}; root {} layer 0x{:08X}, release frame {:?})",
        s.art.overlay.clip(),
        layer.id(),
        end,
        s.art.root,
        root.layer,
        s.root_target
    );
    s.overlay = Some(Overlay { layer, mc, end });
    LIVE.store(true, Ordering::Release);
    true
}

fn destroy_overlay(s: &mut Session) -> bool {
    let Some(o) = s.overlay.take() else {
        return false;
    };
    let id = o.layer.id();
    let ok = bm2d_api::destroy_layer(o.layer);
    if !ok {
        log_warn!(
            "DDR SELECTION: destroying banner overlay layer 0x{:08X} failed",
            id
        );
    }
    ok
}
