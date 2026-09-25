//! Restored A3 `%04d` package-name append — the full replacement of World's
//! `LayoutActor` per-package helper (`layout_package_helper`).
//!
//! World's `LayoutActor::onInitialize` calls the helper once per gameplay
//! package with `(this, side /*0,1; 2 = shared*/, base, skin, shared)`. The
//! helper builds a record value `{std::string name; …; int skin @+0x28}`,
//! probes the arc, and — when `!shared || skin != 0` — inserts
//! `records[side][base] = value` and pushes `name` on the load list. A3 built
//! `name = base + "%04d"(skin)` here; World formats the suffix into a dead
//! buffer and never appends it.
//!
//! The detour (game thread, `LayoutActor::onInitialize`, not a hot path):
//!
//! * disarmed, or a [`policy::Decision::Stock`] package ⇒ the original with
//!   **skin 0** — exactly World's behaviour (shared packages then come from
//!   the stage loader, the record skin stays 0 so World's surviving skin
//!   branches keep their stock path) even though `GameWork+0xA8` holds the
//!   armed skin;
//! * a `Legacy` package ⇒ A3's append, verbatim: probe `<arc_base>000N`
//!   through the game's own arc probe (LayeredFS-aware, `_v3`/`_v0`/…), then
//!   insert `records[side][base] = {"<arc_base>000N", N}` and push the name —
//!   the `LayoutActor` loads it and releases it at finalize like any stock
//!   package. A probe miss falls back to stock (never to `<base>0000`).
//!
//! Both game callees COPY their inputs (checked on every supported build),
//! so the record value is a mod-owned `std::string` view on the stack.

use std::ffi::{c_char, CStr};
use std::ptr::addr_of;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;

use retour::GenericDetour;

use crate::core::hooks;
use crate::core::memory;
use crate::core::msvc::MsvcString;
use crate::core::signatures::DdrSelectionSites;
use crate::{log_info, log_warn};

use super::policy::{self, Decision};

type HelperFn = unsafe extern "C" fn(*mut u8, i32, *const c_char, i32, u8);
type ProbeFn = unsafe extern "C" fn(*const c_char, *const c_char) -> u8;
type InsertFn = unsafe extern "C" fn(*mut u8, *const c_char, *const RecordValue);
type PushFn = unsafe extern "C" fn(*mut u8, *const MsvcString);

/// The helper's record value: `std::string` (0x20 + 8 pad) then `int skin`
/// at +0x28 — the layout the insert callee copies (`value+0x28`).
#[repr(C)]
struct RecordValue {
    name: MsvcString,
    skin: i32,
    _pad: i32,
}

const _: () = assert!(std::mem::offset_of!(RecordValue, skin) == 0x28);

struct Callees {
    probe: ProbeFn,
    insert: InsertFn,
    push: PushFn,
    bm2d_dir: *const c_char,
    records_shared_off: usize,
    records_side_off: usize,
    records_side_stride: usize,
    load_list_off: usize,
}

unsafe impl Send for Callees {}
unsafe impl Sync for Callees {}

static CALLEES: OnceLock<Callees> = OnceLock::new();
static mut HOOK: Option<GenericDetour<HelperFn>> = None;

/// One INFO per (package, skin) per arm — reset by [`reset_arm_logs`].
static LOGGED_LEGACY: AtomicU32 = AtomicU32::new(0);
static LOGGED_MISS: AtomicU32 = AtomicU32::new(0);

pub fn reset_arm_logs() {
    LOGGED_LEGACY.store(0, Ordering::Relaxed);
    LOGGED_MISS.store(0, Ordering::Relaxed);
}

/// Install the detour (once per session; it stays installed and passes
/// through while disarmed).
pub fn install(sites: &DdrSelectionSites) -> bool {
    if unsafe { (*addr_of!(HOOK)).is_some() } {
        return true;
    }
    let callees = unsafe {
        Callees {
            probe: std::mem::transmute::<*const u8, ProbeFn>(sites.probe),
            insert: std::mem::transmute::<*const u8, InsertFn>(sites.record_insert),
            push: std::mem::transmute::<*const u8, PushFn>(sites.load_list_push),
            bm2d_dir: sites.bm2d_dir as *const c_char,
            records_shared_off: sites.records_shared_off,
            records_side_off: sites.records_side_off,
            records_side_stride: sites.records_side_stride,
            load_list_off: sites.load_list_off,
        }
    };
    let _ = CALLEES.set(callees);
    let target: HelperFn = unsafe { std::mem::transmute(sites.package_helper) };
    match unsafe { hooks::install_enabled(std::ptr::addr_of_mut!(HOOK), target, helper_hook) } {
        Ok(()) => true,
        Err(e) => {
            log_warn!(
                "DDR SELECTION: package-helper detour failed: {} -- mod inactive",
                e
            );
            false
        }
    }
}

unsafe extern "C" fn helper_hook(
    this: *mut u8,
    side: i32,
    base: *const c_char,
    skin: i32,
    shared: u8,
) {
    let Some(hook) = (*addr_of!(HOOK)).as_ref() else {
        return;
    };
    let armed = super::armed_skin();
    if armed == 0 || base.is_null() || this.is_null() {
        hook.call(this, side, base, skin, shared);
        if !base.is_null() {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                after_stock(this, base, 0)
            }));
        }
        return;
    }
    let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        register_legacy(this, side, base, armed)
    }))
    .unwrap_or(false);
    if !handled {
        // World's own behaviour for this package (skin 0: no suffix, record
        // skin 0, shared packages left to the stage loader).
        hook.call(this, side, base, 0, shared);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            after_stock(this, base, armed)
        }));
    }
}

/// A package went through World's own path (`armed` = the armed skin, 0 when
/// disarmed): World's stage-frame / gauge export names back for a stock
/// `dance_stage` / `dance_gauge`; on an
/// armed song, the legacy layout root pushed onto the load list next to
/// World's `dance_common` (no record — World's builder keeps World's root;
/// `markers.rs` reads the legacy one after it).
unsafe fn after_stock(this: *mut u8, base: *const c_char, armed: u8) {
    let Ok(base_str) = CStr::from_ptr(base).to_str() else {
        return;
    };
    match base_str {
        "dance_stage" => super::stage_frame::restore(),
        "dance_gauge" => super::gauge::restore(),
        "dance_common" if armed != 0 && !this.is_null() => {
            let Some(root) = super::markers::on_common_request(this, armed) else {
                return;
            };
            if push_extra(this, root) {
                log_info!(
                    "DDR SELECTION: legacy layout root {} queued on the LayoutActor load list (skin {})",
                    root,
                    armed
                );
            }
        }
        _ => {}
    }
}

/// Push `name` onto the `LayoutActor` load list without a record (the actor
/// loads it with its own packages and releases it at finalize).
unsafe fn push_extra(this: *mut u8, name: &str) -> bool {
    let Some(c) = CALLEES.get() else {
        return false;
    };
    if !memory::is_readable(this, c.load_list_off + 0x18) {
        return false;
    }
    let mut bytes = name.as_bytes().to_vec();
    bytes.push(0);
    let view = string_view(&bytes[..bytes.len() - 1]);
    (c.push)(this.add(c.load_list_off), &view);
    true
}

/// A3's append for one package. `true` = registered (the original must NOT
/// run); `false` = stock.
unsafe fn register_legacy(this: *mut u8, side: i32, base: *const c_char, skin: u8) -> bool {
    let Some(c) = CALLEES.get() else {
        return false;
    };
    let Ok(base_str) = CStr::from_ptr(base).to_str() else {
        return false;
    };
    let Decision::Legacy { arc_base, skin } = policy::decide(base_str, skin, super::adapters())
    else {
        return false;
    };
    if !(0..=2).contains(&side) {
        return false;
    }
    let bit = policy::package_index(base_str)
        .map(|i| 1u32 << i)
        .unwrap_or(0);
    if !memory::is_readable(this, c.load_list_off + 0x18) {
        return false;
    }

    let name = policy::legacy_name(arc_base, skin);
    // Positions from the legacy layout root (danger 3–5 at `danger_gauge`):
    // never without the root, or the element lands at (0,0).
    if policy::adapter_for(base_str, skin) == Some(policy::Adapter::Markers)
        && !super::markers::root_available(skin)
    {
        if LOGGED_MISS.fetch_or(bit, Ordering::Relaxed) & bit == 0 {
            log_info!(
                "DDR SELECTION: no legacy layout root for skin {} -- {} stays stock",
                skin,
                base_str
            );
        }
        return false;
    }
    if (c.probe)(c.bm2d_dir, name.as_ptr() as *const c_char) == 0 {
        if LOGGED_MISS.fetch_or(bit, Ordering::Relaxed) & bit == 0 {
            log_info!(
                "DDR SELECTION: no {} arc for skin {} -- {} stays stock",
                name.trim_end_matches('\0'),
                skin,
                base_str
            );
        }
        return false;
    }

    // World's StageFrameActor asks the package for export `dance_stage` —
    // A3's names must be patched in first, or the package stays stock.
    if base_str == "dance_stage" && !super::stage_frame::apply(skin) {
        return false;
    }
    // Same for the gauge actors' `dance_gauge` export (A3: `00_dance_gauge`).
    if base_str == "dance_gauge" && !super::gauge::apply() {
        return false;
    }

    let bytes = &name.as_bytes()[..name.len() - 1];
    let value = RecordValue {
        name: string_view(bytes),
        skin: skin as i32,
        _pad: 0,
    };
    let map = if side == 2 {
        this.add(c.records_shared_off)
    } else {
        this.add(c.records_side_off + side as usize * c.records_side_stride)
    };
    (c.insert)(map, base, &value);
    (c.push)(this.add(c.load_list_off), &value.name);

    super::note_legacy(bit);
    if LOGGED_LEGACY.fetch_or(bit, Ordering::Relaxed) & bit == 0 {
        log_info!(
            "DDR SELECTION: {} -> {} (skin {}, side {})",
            base_str,
            name.trim_end_matches('\0'),
            skin,
            side
        );
    }
    true
}

/// Whether `data/arc/<dir>/<name>{_v3,_v0,_lite,}.arc` exists, through the
/// game's own arc probe (LayeredFS-aware). `false` before [`install`].
pub fn probe_arc(dir: &CStr, name: &CStr) -> bool {
    match CALLEES.get() {
        Some(c) => unsafe { (c.probe)(dir.as_ptr(), name.as_ptr()) != 0 },
        None => false,
    }
}

/// An MSVC `std::string` view of `bytes` (which the caller keeps alive and
/// NUL-terminated at `bytes.len()` for the duration of the game calls). The
/// callees only copy from it.
fn string_view(bytes: &[u8]) -> MsvcString {
    if bytes.len() <= 15 {
        return MsvcString::sso_bytes(bytes);
    }
    let mut buf = [0u8; 16];
    buf[..8].copy_from_slice(&(bytes.as_ptr() as u64).to_le_bytes());
    MsvcString {
        buf,
        len: bytes.len() as u64,
        cap: (bytes.len() as u64).max(31),
        _pad: 0,
    }
}
