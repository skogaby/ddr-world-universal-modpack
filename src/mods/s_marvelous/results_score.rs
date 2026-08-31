//! Results score-tab S-MARVELOUS row + exclusive MARVELOUS (design §4.7,
//! plan Step 7).
//!
//! ONE template (`body_tab_detail_result`) serves both judgement-count tabs
//! ("Simple results" = kind 1 plays `loop_guest`@f130, "Details" = kind 6
//! plays `loop_registered`@f18 — package dump 2026-08-30). The row labels
//! are a single stacked sheet texture (`scre_tab_detail_judge`, 108×118,
//! six words at 19px pitch) baked into the template; the six count rows are
//! named instances `marvelous_num_usr`..`miss_num_usr` at ty 43..138.
//!
//! Mechanism (maintainer-approved Option 1, 2026-08-30):
//!
//! 1. **Sheet swap** — the two label sheets are replaced with 7-row art
//!    (same canvas, 16px pitch, violet S-MARVELOUS on top) via plain
//!    stock-name texture replacement (`assets::stage_results`). Because
//!    LayeredFS serves those passively from disk, staging is enable-gated
//!    and PURGED at init/disable — a disabled mod must never leave a 7-row
//!    sheet under 6-row row positions.
//! 2. **Row repositioning** — an afp_patcher patch translate-splices the
//!    six stock rows down to the 16px grid ([`ROW_MOVES`], f0 placements +
//!    f127 guest-move updates, root + sprite-130 dual timeline; no new
//!    objects/shapes/labels, so none of the engine invariants are in play).
//! 3. **The S-MARV number** — a post-original detour on the PlaydataTab
//!    populate (`playdata_tab_update`, vslot 7; the heavy populate is
//!    gated on the dirty byte tab+0x151, consumed at its start — reading
//!    it pre-call detects exactly the populate frames) calls the game's
//!    OWN row-write helper (`playdata_row_write`) so the widget joins the
//!    tab's widget vector: the game lays it out every frame and destroys
//!    it with the tab. Anchor = the EXISTING `marvelous_num_usr` instance
//!    with `offset_y = -16` (one row above) — the anchor's own guest-move
//!    and fade update records then apply to our row for free.
//! 4. **Exclusive MARVELOUS** — the stock marvelous widget's glyphs are
//!    rewritten to `stock − smarv` via `spritelayer_set_names`, so the
//!    seven rows sum to the stock total.
//!
//! Counts are recomputed from the stage record's per-note grade/ms streams
//! ([`super::records`]) with the window the side was last armed with —
//! correct for every stage of a multi-stage session and independent of the
//! live gameplay counters. The record resolution replicates the populate
//! fn's own branch (course gate global ⇒ course record, else per-stage
//! array), so S-Marvelous rows show in EVERY mode the tab shows counts in
//! (normal, course/Dan — maintainer directive 2026-08-30).
//!
//! Fail-open everywhere: unresolved signatures ⇒ no detour, no staging, no
//! patch (stock tab); record-stream surprises ⇒ latched WARN + stock counts
//! for that tab instance.
//!
//! PANIC SAFETY: the detour callback wraps all post-original work in
//! `catch_unwind`; no unwrap/indexing on the hook path.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};
use std::sync::{Mutex, Once};

use retour::GenericDetour;

use crate::core::ap2::Ap2Doc;
use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::services::{afp_patcher, stage_records};
use crate::{log_info, log_warn};

use super::assets::{self, StagedResultPatch, RESULT_TEMPLATE};
use super::{records, state};

// ── Row repositioning table ──────────────────────────────────────────
//
// (depth, stock ty_raw, dy_raw) in raw fixed-point /20 units. New grid =
// 16px pitch from ty 43 (S-MARV on top at the stock marvelous position):
// 43,59,75,91,107,123,139. Each row matches exactly 4 records (f0 + f127
// guest update, in root + the sprite-130 copy).
//
// NOTE: `scripts/validate_s_marvelous.sh` Leg F extracts this table from
// this file mechanically — keep one `(depth, ty, dy),` tuple per line.
pub const ROW_MOVES: [(u16, i32, i32); 6] = [
    (23, 860, 320),  // marvelous_num_usr  ty 43 -> 59
    (22, 1240, 260), // perfect_num_usr    ty 62 -> 75
    (21, 1620, 200), // great_num_usr      ty 81 -> 91
    (20, 2000, 140), // good_num_usr       ty 100 -> 107
    (19, 2380, 80),  // ok_num_usr         ty 119 -> 123
    (18, 2760, 20),  // miss_num_usr       ty 138 -> 139
];
pub const ROW_MOVES_EXPECTED_EACH: usize = 4;

/// The one transform the patch fn, the staging dry-run, and the offline
/// harness all share (the deploy-#3 lesson: dev legs must exercise the
/// DLL's actual code path).
pub fn apply_row_moves(doc: &mut Ap2Doc) -> Option<()> {
    doc.shift_row_translates(&ROW_MOVES, ROW_MOVES_EXPECTED_EACH)
}

// ── Tab / widget field offsets (display-side RE §1–2) ────────────────

const TAB_WRAPPER: usize = 0x110;
const TAB_RECORD_SIDE: usize = 0x148;
const TAB_STAGE: usize = 0x14C;
const TAB_DIRTY: usize = 0x151;
const TAB_WIDGETS_BEGIN: usize = 0x158;
const TAB_WIDGETS_END: usize = 0x160;
/// SpriteLayer fields (music_wheel_song_length research §3).
const SL_ANCHOR_NAME: usize = 0x68; // MSVC string: buf/ptr +0x68, len +0x78, cap +0x80
const SL_OFFSET_Y: usize = 0xC8;

/// Our row sits one 16px pitch above its anchor (the stock marvelous row).
const SMARV_OFFSET_Y: f64 = -16.0;
/// The anchor instance our row rides (17 bytes — heap-form MSVC string).
const ANCHOR_NAME: &[u8] = b"marvelous_num_usr";

// ── Game function ABIs ───────────────────────────────────────────────

/// PlaydataTab populate/update — vslot 7, `fn(this)`.
type TabUpdateFn = unsafe extern "C" fn(*mut u8) -> u64;
/// Row-write helper: (ctx {wrapper,tab}, out shared_ptr pair, anchor-name
/// string, text string) -> out. Creates a SpriteLayer number widget and
/// pushes it into tab+0x158 (the game then owns layout + destruction).
type RowWriteFn = unsafe extern "C" fn(
    *mut RowCtx,
    *mut SharedPtrPair,
    *const MsvcString,
    *const MsvcString,
) -> *mut SharedPtrPair;
/// `sequence::SpriteLayer::SetBitmaps(this, names)` — copy-assigns.
type SetNamesFn = unsafe extern "C" fn(*mut u8, *const MsvcVec) -> *mut u8;

/// The ctx pair the row-write helper reads: `[0]` = parent wrapper
/// (`*(tab+0x110)`), `[1]` = the tab (for the widget-vector push).
#[repr(C)]
struct RowCtx {
    wrapper: *mut u8,
    tab: *mut u8,
}

/// MSVC `std::shared_ptr` — {object, control block}.
#[repr(C)]
struct SharedPtrPair {
    obj: *mut u8,
    ctrl: *mut u8,
}

/// MSVC `std::string` as the game's `vector<string>` stores it (0x28
/// stride: 16-byte SSO buf/heap-ptr union, size, capacity, 8 bytes
/// TRAILING PAD — the pad is load-bearing: `set_names` walks the source
/// vector at 0x28 stride, so a 0x20-stride array reads as ZERO elements;
/// music_wheel's cabinet-proven GameString layout, re-confirmed by Step-7
/// deploy #1's blank MARVELOUS row). The game only READS strings we pass
/// by const ref, so the heap form may point at our own static bytes.
#[repr(C)]
struct MsvcString {
    buf: [u8; 16],
    len: u64,
    cap: u64,
    _pad: u64,
}

impl MsvcString {
    /// SSO form — `s` must be ≤ 15 bytes (oversized clamps to empty rather
    /// than panicking on the hook path; the music_wheel 2026-08-16 lesson).
    fn sso(s: &str) -> MsvcString {
        let bytes = s.as_bytes();
        let n = if bytes.len() <= 15 { bytes.len() } else { 0 };
        let mut buf = [0u8; 16];
        buf[..n].copy_from_slice(&bytes[..n]);
        MsvcString {
            buf,
            len: n as u64,
            cap: 0xF,
            _pad: 0,
        }
    }

    /// Heap form referencing caller-owned storage (for names > 15 bytes).
    /// `bytes` must outlive every use (ours are `'static`).
    fn heap_ref(bytes: &'static [u8]) -> MsvcString {
        let mut buf = [0u8; 16];
        buf[..8].copy_from_slice(&(bytes.as_ptr() as u64).to_le_bytes());
        MsvcString {
            buf,
            len: bytes.len() as u64,
            // Any value > 15 selects the heap-pointer interpretation; 31
            // mirrors MSVC's minimum heap capacity.
            cap: 31,
            _pad: 0,
        }
    }
}

/// MSVC `std::vector<std::string>` header — passed by const pointer as the
/// set-names SOURCE (copy-assigned; backing storage stays ours).
#[repr(C)]
struct MsvcVec {
    begin: *const MsvcString,
    end: *const MsvcString,
    cap_end: *const MsvcString,
}

// ── State ────────────────────────────────────────────────────────────

static DETOUR: once_cell::sync::OnceCell<GenericDetour<TabUpdateFn>> =
    once_cell::sync::OnceCell::new();
static ROW_WRITE: AtomicUsize = AtomicUsize::new(0);
static SET_NAMES: AtomicUsize = AtomicUsize::new(0);
static COURSE_GATE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());

/// Sheets staged + patch registered + mod enabled (deactivate clears).
static ASSETS_READY: AtomicBool = AtomicBool::new(false);
/// The patch fn produced a patched template this session (latched — a
/// template already loaded stays patched across disable, like dance_judge).
static PATCH_APPLIED: AtomicBool = AtomicBool::new(false);

static STAGED: Mutex<Option<StagedResultPatch>> = Mutex::new(None);
static REGISTER_ONCE: Once = Once::new();

static WARN_RECORD: AtomicBool = AtomicBool::new(false);
static WARN_STOCK_WIDGET: AtomicBool = AtomicBool::new(false);
static WARN_VARIANT: AtomicBool = AtomicBool::new(false);
static WARN_TRANSFORM: AtomicBool = AtomicBool::new(false);

fn warn_once(latch: &AtomicBool, msg: &str) {
    if !latch.swap(true, Ordering::Relaxed) {
        log_warn!("{}", msg);
    }
}

// ── Install (mod init) ───────────────────────────────────────────────

/// Resolve the results-surface signatures and install the populate detour.
/// All-or-nothing: without any piece the results tab stays fully stock
/// (and `activate()` must not stage the sheets — the caller gates on the
/// returned bool).
pub fn install(signatures: &SignatureStore) -> bool {
    let (Some(populate), Some(row_write), Some(set_names), Some(course_gate)) = (
        signatures.get_address("playdata_tab_update"),
        signatures.get_address("playdata_row_write"),
        signatures.get_address("spritelayer_set_names"),
        signatures.get_address("results_course_gate_global"),
    ) else {
        log_warn!("SMarvelous: results-tab signatures unresolved -- results tab stays stock");
        return false;
    };
    ROW_WRITE.store(row_write as usize, Ordering::Release);
    SET_NAMES.store(set_names as usize, Ordering::Release);
    COURSE_GATE.store(course_gate as *mut u8, Ordering::Release);

    let target: TabUpdateFn = unsafe { std::mem::transmute(populate) };
    match unsafe { GenericDetour::new(target, tab_update_hook) } {
        Ok(detour) => {
            if unsafe { detour.enable() }.is_err() {
                log_warn!("SMarvelous: results detour enable failed -- results tab stays stock");
                return false;
            }
            let _ = DETOUR.set(detour);
            log_info!("SMarvelous: results score-tab detour installed");
            true
        }
        Err(e) => {
            log_warn!(
                "SMarvelous: results detour failed: {:?} -- results tab stays stock",
                e
            );
            false
        }
    }
}

// ── Activate / deactivate (mod enable/disable) ───────────────────────

/// Stage the sheets + register the template patch. Called from enable
/// (only when [`install`] succeeded).
pub fn activate() {
    let mut staged = match STAGED.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    if staged.is_none() {
        *staged = assets::stage_results(|doc| {
            let mut scratch = doc.clone();
            apply_row_moves(&mut scratch).is_some() && scratch.serialize().is_some()
        });
        if staged.is_none() {
            return; // WARN already emitted; nothing staged (fail-open)
        }
        REGISTER_ONCE.call_once(|| {
            afp_patcher::register_patch(RESULT_TEMPLATE, Box::new(patch_detail_result));
        });
    } else {
        // Re-enable: the sheets were purged at disable — restage them.
        if !assets::restage_result_sheets() {
            return;
        }
    }
    ASSETS_READY.store(true, Ordering::Release);
}

/// Disable: patch fn goes inert (subsequent template loads stream stock)
/// and the stock-name sheet replacements are unstaged so the NEXT session
/// serves stock art. Textures/templates already mounted this session stay
/// as they are — consistent (7-row sheet ⇔ patched rows).
pub fn deactivate() {
    ASSETS_READY.store(false, Ordering::Release);
    assets::purge_results();
}

// ── The template patch ───────────────────────────────────────────────

fn patch_detail_result(afp: &[u8], _bsi: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    if !ASSETS_READY.load(Ordering::Acquire) {
        return None; // disabled/unstaged — stock, no warn (normal state)
    }
    let guard = STAGED.lock().ok()?;
    let staged = guard.as_ref()?;

    // v1 skin gate (house pattern): unknown variants stream stock — and
    // the sheets are unstaged so the 7-row art doesn't sit over unmoved
    // rows on the next mount.
    if afp != staged.stock_bytes.as_slice() {
        warn_once(
            &WARN_VARIANT,
            "SMarvelous: body_tab_detail_result variant differs from the staged template -- streaming stock (sheets unstaged)",
        );
        assets::purge_results();
        return None;
    }

    let run = || -> Option<Vec<u8>> {
        let mut doc = Ap2Doc::parse(afp)?;
        apply_row_moves(&mut doc)?;
        doc.serialize()
    };
    match run() {
        Some(out) => {
            PATCH_APPLIED.store(true, Ordering::Release);
            log_info!(
                "SMarvelous: body_tab_detail_result patched ({} -> {} bytes, {} rows to 16px pitch)",
                afp.len(),
                out.len(),
                ROW_MOVES.len()
            );
            Some((out, vec![0u8; 2]))
        }
        None => {
            warn_once(
                &WARN_TRANSFORM,
                "SMarvelous: body_tab_detail_result transform failed at stream time -- streaming stock (sheets unstaged)",
            );
            assets::purge_results();
            None
        }
    }
}

// ── The populate detour ──────────────────────────────────────────────

unsafe extern "C" fn tab_update_hook(tab: *mut u8) -> u64 {
    // The populate consumes the dirty byte at its start — read it BEFORE
    // the original to know whether THIS call populated the rows.
    let populated = !tab.is_null()
        && memory::read_u8(tab.add(TAB_DIRTY)) != 0
        && !memory::read_ptr(tab.add(TAB_WRAPPER)).is_null();

    let ret = match DETOUR.get() {
        Some(d) => d.call(tab),
        None => 0,
    };

    if populated && ASSETS_READY.load(Ordering::Acquire) && PATCH_APPLIED.load(Ordering::Acquire) {
        if let Err(e) = std::panic::catch_unwind(|| populate_smarv_row(tab)) {
            let _ = e;
        }
    }
    ret
}

/// Post-populate: compute the exclusive counts from the stage record,
/// rewrite the stock marvelous widget, and add/refresh our S-MARV row.
fn populate_smarv_row(tab: *mut u8) {
    unsafe {
        let side = memory::read_i32(tab.add(TAB_RECORD_SIDE));
        let stage = memory::read_i32(tab.add(TAB_STAGE));
        if !(0..=1).contains(&side) {
            return;
        }
        let side = side as usize;

        // The window the side played with (sticky across the GAMEPLAY-exit
        // disarm). 0 = the mod wasn't armed for this song — stock counts.
        let window = state::last_armed_window(side);
        if window <= 0 {
            return;
        }
        // Non-entered sides never played — silent bail, not a WARN (the
        // populate can run for both tabs regardless).
        if stage_records::side_entered(side) == Some(false) {
            return;
        }

        // Resolve the SAME record the populate just displayed (its own
        // course-gate branch, replicated).
        let record = if course_active() {
            stage_records::course_record(side)
        } else if stage >= 0 {
            stage_records::stage_record(side, stage as usize)
        } else {
            None
        };
        let Some(record) = record else {
            warn_once(
                &WARN_RECORD,
                "SMarvelous: results record unavailable -- tab stays stock",
            );
            return;
        };
        // Virgin record (mcode == -1, the save marshal's skip key) — the
        // tab has nothing real to show; bail silently.
        if memory::read_i32(record) == -1 {
            return;
        }

        let Some(smarv) = records::smarv_count_from_record(record, window) else {
            warn_once(
                &WARN_RECORD,
                "SMarvelous: record streams unreadable/mismatched -- tab stays stock",
            );
            return;
        };
        let Some(marv) = records::marv_count_from_record(record) else {
            warn_once(
                &WARN_RECORD,
                "SMarvelous: record marvelous counter unreadable -- tab stays stock",
            );
            return;
        };
        let exclusive = marv.saturating_sub(smarv);

        // 1) Exclusive MARVELOUS on the stock widget (BEFORE creating our
        //    row — the scan matches by anchor name, and ours shares it).
        if !rewrite_stock_marvelous(tab, exclusive) {
            warn_once(
                &WARN_STOCK_WIDGET,
                "SMarvelous: stock marvelous widget not found -- MARVELOUS stays inclusive",
            );
            // Still add our row: an inclusive MARVELOUS + S-MARV subset is
            // wrong-but-honest; refusing the row entirely would leave the
            // 7-row sheet with a blank slot.
        }

        // 2) Our S-MARV row: reuse an existing one (idempotent across any
        //    re-populate) or create it through the game's own helper.
        let digits = format_count(smarv);
        if let Some(existing) = find_our_row(tab) {
            set_widget_names_digits(existing, &digits);
        } else if !create_smarv_row(tab, &digits) {
            return;
        }

        log_info!(
            "SMarvelous: results row live (side {}, stage {}, smarv {}, marv_excl {})",
            side,
            stage,
            smarv,
            exclusive
        );
    }
}

/// The populate fn's own record-selection gate: `*(**global + 0x70) != 0`
/// ⇒ the tab displays the course record. Shared with `results_graph` (the
/// GraphTab reads the same branch).
pub(super) fn course_active() -> bool {
    let global = COURSE_GATE.load(Ordering::Acquire);
    if global.is_null() {
        return false;
    }
    unsafe {
        let p1 = memory::read_ptr(global);
        if p1.is_null() {
            return false;
        }
        let p2 = memory::read_ptr(p1);
        if p2.is_null() {
            return false;
        }
        memory::read_u64(p2.add(0x70)) != 0
    }
}

/// Decimal digits for a count (display-clamped like the stock rows' i32).
fn format_count(n: u32) -> String {
    format!("{}", n.min(99_999))
}

// ── Widget-vector helpers ────────────────────────────────────────────

/// Walk the tab's widget vector (`tab+0x158..0x160`, 0x10-stride
/// shared_ptr pairs), yielding widget object pointers.
unsafe fn widget_vector(tab: *mut u8) -> impl Iterator<Item = *mut u8> {
    let begin = memory::read_ptr(tab.add(TAB_WIDGETS_BEGIN)) as usize;
    let end = memory::read_ptr(tab.add(TAB_WIDGETS_END)) as usize;
    let count = if begin != 0 && end > begin {
        ((end - begin) / 0x10).min(64)
    } else {
        0
    };
    (0..count).filter_map(move |i| {
        let obj = memory::read_ptr((begin + i * 0x10) as *const u8) as *mut u8;
        if obj.is_null() {
            None
        } else {
            Some(obj)
        }
    })
}

/// Read a widget's anchor-name string (+0x68 MSVC string) and compare.
unsafe fn widget_anchor_is(widget: *mut u8, name: &[u8]) -> bool {
    let len = memory::read_u64(widget.add(SL_ANCHOR_NAME + 0x10)) as usize;
    if len != name.len() || len > 0x100 {
        return false;
    }
    let cap = memory::read_u64(widget.add(SL_ANCHOR_NAME + 0x18)) as usize;
    let data = if cap > 15 {
        memory::read_ptr(widget.add(SL_ANCHOR_NAME)) as *const u8
    } else {
        widget.add(SL_ANCHOR_NAME) as *const u8
    };
    if data.is_null() {
        return false;
    }
    std::slice::from_raw_parts(data, len) == name
}

/// Our row's discriminator vs the stock marvelous widget on the same
/// anchor: ours carries the -16 y offset.
unsafe fn widget_is_ours(widget: *mut u8) -> bool {
    (memory::read_u64(widget.add(SL_OFFSET_Y)) as u64) == SMARV_OFFSET_Y.to_bits()
}

unsafe fn find_our_row(tab: *mut u8) -> Option<*mut u8> {
    widget_vector(tab).find(|&w| widget_anchor_is(w, ANCHOR_NAME) && widget_is_ours(w))
}

/// Rewrite the STOCK marvelous widget's glyphs to the exclusive count.
unsafe fn rewrite_stock_marvelous(tab: *mut u8, exclusive: u32) -> bool {
    let Some(widget) =
        widget_vector(tab).find(|&w| widget_anchor_is(w, ANCHOR_NAME) && !widget_is_ours(w))
    else {
        return false;
    };
    set_widget_names_digits(widget, &format_count(exclusive));
    true
}

/// Apply a digit string to a SpriteLayer as `scre_tab_num_<d>` glyph names
/// via the game's set-names (copy-assign; backing storage stays ours).
unsafe fn set_widget_names_digits(widget: *mut u8, digits: &str) {
    let set_names = SET_NAMES.load(Ordering::Acquire);
    if set_names == 0 || widget.is_null() {
        return;
    }
    const MAX_GLYPHS: usize = 8;
    let mut names: [MsvcString; MAX_GLYPHS] = std::array::from_fn(|_| MsvcString::sso(""));
    let mut count = 0usize;
    for ch in digits.chars().take(MAX_GLYPHS) {
        if !ch.is_ascii_digit() {
            continue;
        }
        let mut name = String::with_capacity(15);
        name.push_str("scre_tab_num_");
        name.push(ch);
        names[count] = MsvcString::sso(&name);
        count += 1;
    }
    let vec = MsvcVec {
        begin: names.as_ptr(),
        end: names.as_ptr().add(count),
        cap_end: names.as_ptr().add(MAX_GLYPHS),
    };
    let f: SetNamesFn = std::mem::transmute(set_names);
    f(widget, &vec);
}

/// Create our S-MARV row through the game's row-write helper (the widget
/// joins the tab's vector — game-owned layout and lifetime), then apply
/// the -16 offset that lifts it one row above its anchor.
unsafe fn create_smarv_row(tab: *mut u8, digits: &str) -> bool {
    let row_write = ROW_WRITE.load(Ordering::Acquire);
    if row_write == 0 {
        return false;
    }
    let wrapper = memory::read_ptr(tab.add(TAB_WRAPPER)) as *mut u8;
    if wrapper.is_null() {
        return false;
    }

    let mut ctx = RowCtx { wrapper, tab };
    let mut out = SharedPtrPair {
        obj: std::ptr::null_mut(),
        ctrl: std::ptr::null_mut(),
    };
    let name = MsvcString::heap_ref(ANCHOR_NAME);
    let text = MsvcString::sso(digits);

    let f: RowWriteFn = std::mem::transmute(row_write);
    f(&mut ctx, &mut out, &name, &text);

    let widget = out.obj;
    let ok = !widget.is_null();
    if ok {
        // One pitch above the anchor row. Every other field (alignment,
        // scale, tint) stays exactly what the helper gave the stock
        // marvelous row.
        (widget.add(SL_OFFSET_Y) as *mut f64).write_unaligned(SMARV_OFFSET_Y);
    }
    // Drop OUR strong ref — the tab's vector holds its own (the widget
    // lives and dies with the tab).
    release_shared_ptr(&mut out);
    ok
}

/// MSVC shared_ptr release (the stock populate's inlined dtor dance):
/// LOCK-dec the strong count; at zero run ctrl vft[0] (destroy managed
/// object), then LOCK-dec the weak count; at zero run ctrl vft[1] (free
/// the control block). In practice the vector's ref keeps the count > 0.
unsafe fn release_shared_ptr(pair: &mut SharedPtrPair) {
    let ctrl = pair.ctrl;
    pair.obj = std::ptr::null_mut();
    pair.ctrl = std::ptr::null_mut();
    if ctrl.is_null() {
        return;
    }
    let strong = &*(ctrl.add(0x8) as *const std::sync::atomic::AtomicI32);
    if strong.fetch_sub(1, Ordering::AcqRel) == 1 {
        let vft = memory::read_ptr(ctrl) as *const usize;
        if vft.is_null() {
            return;
        }
        let destroy: unsafe extern "C" fn(*mut u8) = std::mem::transmute(*vft);
        destroy(ctrl);
        let weak = &*(ctrl.add(0xC) as *const std::sync::atomic::AtomicI32);
        if weak.fetch_sub(1, Ordering::AcqRel) == 1 {
            let vft = memory::read_ptr(ctrl) as *const usize;
            if vft.is_null() {
                return;
            }
            let free_ctrl: unsafe extern "C" fn(*mut u8) = std::mem::transmute(*vft.add(1));
            free_ctrl(ctrl);
        }
    }
}
