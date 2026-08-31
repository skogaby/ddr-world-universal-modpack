//! Results judgement graph — the S-Marvelous series + legend entry
//! (design §4.8, plan Step 8).
//!
//! The GraphTab's vslot-6 ingest aggregates the record's per-note streams
//! into per-second `vector<double>` series on the tab (judge chart at
//! `tab+0x538+k*0x20`: [0] filler, [1] miss, [2] good, [3] great,
//! [4] perfect, [5] marvelous+O.K., [6] all-marvelous shimmer), and the
//! vslot-7 rebuild (`graph_tab_rebuild`) clears + rebuilds every chart and
//! legend text EVERY FRAME. Three cooperating detours, zero position math
//! (2026-08-30 RE — `progress.md` Step 8 entry):
//!
//! 1. **Rebuild detour (pre-original, one-shot per tab)**: build our
//!    per-second S-Marv vector from the stage record (mirror bucketing —
//!    [`records::smarv_per_second`]), SUBTRACT it from the marvelous
//!    series (+0x5D8) / shimmer (+0x5F8) per second (whichever holds the
//!    second's content, clamped), and register the tab.
//! 2. **Chart-append detour** (`graph_chart_append` — the single-color
//!    series append the rebuild calls once per series): after the FILLER
//!    series (`vec == tab+0x538`) of a registered tab on the judge page,
//!    append our series — the callable's lambda vftable is captured live
//!    from the incoming argument BEFORE the original consumes it (same
//!    `(uint,double,double)` family, so a `{vft, violet}` clone of it is
//!    a valid color functor). Our series therefore LEADS the
//!    judge-colored series (S-Marv first — maintainer directive
//!    2026-08-30), and re-injection every frame is automatic (charts are
//!    rebuilt per frame).
//! 3. **Legend detour** (`graph_legend_text`): when the stock white
//!    "■MARVELOUS" legend line (rgba 0xF0F0F0FF, judge page, registered
//!    tab) arrives, first call the original with our own "■MARVELOUS" in
//!    VIOLET (no "S-" prefix — matches the shipped art language), then
//!    pass the stock call through — the caller's stack context (rect
//!    block + running x-cursor) does all layout.
//!
//! Re-entry safety: our own injected calls fail the gates (different
//! vec / different rgba), and the registry lock is NEVER held across a
//! call into game code (same-thread `Mutex` re-entry deadlocks).
//! Registry keyed by tab pointer, cleared on every scene change (tab
//! allocations recycle).
//!
//! Fail-open: unresolved signatures ⇒ no detours (stock graph); record
//! surprises ⇒ that tab stays stock with one latched WARN.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use retour::GenericDetour;

use crate::core::memory;
use crate::core::msvc::{MsvcString, MsvcVec};
use crate::core::signatures::SignatureStore;
use crate::services::stage_records;
use crate::{log_info, log_warn};

use super::{records, results_score, state};

// ── GraphTab field offsets (RE 2026-08-30) ───────────────────────────

const TAB_RECORD_SIDE: usize = 0x148;
const TAB_STAGE: usize = 0x14C;
const TAB_PAGE: usize = 0x138; // 0 = judge graph
const TAB_HAS_DATA: usize = 0x1C4;
/// Judge-series vector<double> groups (0x20 stride).
const SERIES_FILLER: usize = 0x538;
const SERIES_MARVELOUS: usize = 0x5D8;
const SERIES_SHIMMER: usize = 0x5F8;

/// Art-matched deep violet (the combo tint pair's deep member), RGBA.
const VIOLET_RGBA: u32 = 0xB05C_E0FF;
/// "■MARVELOUS" — SJIS ■ (81 A1) + ASCII, 11 bytes (SSO-safe). The
/// S-Marv legend entry deliberately reads "MARVELOUS" in violet (no "S-"
/// prefix) to match the rest of the shipped art language (maintainer
/// directive 2026-08-30), and is injected BEFORE the stock white entry
/// (first judge item).
const LEGEND_TEXT: &[u8] = b"\x81\xA1MARVELOUS";
/// The stock "■MARVELOUS" legend line's color — the injection anchor.
const MARVELOUS_LEGEND_RGBA: u32 = 0xF0F0_F0FF;

// ── Game ABIs ────────────────────────────────────────────────────────

/// GraphTab rebuild — vslot 7, `fn(this)`.
type RebuildFn = unsafe extern "C" fn(*mut u8) -> u64;
/// Chart series append: (chart, &vector<double>, &callable).
type ChartAppendFn = unsafe extern "C" fn(*mut u8, *mut MsvcVec<f64>, *mut ColorCallable) -> u64;
/// Legend line: (&ctx {rect*, cursor*, tab*}, &string, rgba).
type LegendFn = unsafe extern "C" fn(*mut u8, *const MsvcString, u32) -> u64;

/// The rebuild's stack shape for the color functor: a 0x20-byte MSVC
/// `std::function` with inline storage — `{impl vftable, rgba, pad,
/// impl_ptr → self}`. The append clones the impl through vft slot 0 and
/// CONSUMES the object (impl_ptr nulled), so a fresh stack instance per
/// call is exactly what the stock caller does too.
#[repr(C)]
struct ColorCallable {
    vft: usize,
    rgba: u32,
    _pad: u32,
    _pad2: u64,
    impl_ptr: *mut ColorCallable,
}

// ── State ────────────────────────────────────────────────────────────

static REBUILD_DETOUR: once_cell::sync::OnceCell<GenericDetour<RebuildFn>> =
    once_cell::sync::OnceCell::new();
static APPEND_DETOUR: once_cell::sync::OnceCell<GenericDetour<ChartAppendFn>> =
    once_cell::sync::OnceCell::new();
static LEGEND_DETOUR: once_cell::sync::OnceCell<GenericDetour<LegendFn>> =
    once_cell::sync::OnceCell::new();

/// Mod enabled (deactivate flips; detours stay installed but inert).
static ACTIVE: AtomicBool = AtomicBool::new(false);

struct TabState {
    /// Our per-second S-Marv counts (padded to the game series length).
    smarv: Vec<f64>,
    /// Whether there is anything to draw (all-zero vectors skip
    /// injection so we never add an empty series/legend line).
    any: bool,
}

static TABS: Mutex<Option<HashMap<usize, TabState>>> = Mutex::new(None);

static WARN_RECORD: AtomicBool = AtomicBool::new(false);
static WARN_LEFTOVER: AtomicBool = AtomicBool::new(false);
static FIRST_INJECT_LOGGED: AtomicBool = AtomicBool::new(false);

fn warn_once(latch: &AtomicBool, msg: &str) {
    if !latch.swap(true, Ordering::Relaxed) {
        log_warn!("{}", msg);
    }
}

// ── Install / lifecycle ──────────────────────────────────────────────

/// Resolve + install the three detours. All-or-nothing; fail-open.
pub fn install(signatures: &SignatureStore) -> bool {
    let (Some(rebuild), Some(append), Some(legend)) = (
        signatures.get_address("graph_tab_rebuild"),
        signatures.get_address("graph_chart_append"),
        signatures.get_address("graph_legend_text"),
    ) else {
        log_warn!("SMarvelous: graph-tab signatures unresolved -- graph stays stock");
        return false;
    };

    unsafe {
        let rebuild_fn: RebuildFn = std::mem::transmute(rebuild);
        let append_fn: ChartAppendFn = std::mem::transmute(append);
        let legend_fn: LegendFn = std::mem::transmute(legend);
        let (Ok(d1), Ok(d2), Ok(d3)) = (
            GenericDetour::new(rebuild_fn, rebuild_hook),
            GenericDetour::new(append_fn, append_hook),
            GenericDetour::new(legend_fn, legend_hook),
        ) else {
            log_warn!("SMarvelous: graph detour creation failed -- graph stays stock");
            return false;
        };
        if d1.enable().is_err() || d2.enable().is_err() || d3.enable().is_err() {
            log_warn!("SMarvelous: graph detour enable failed -- graph stays stock");
            return false;
        }
        let _ = REBUILD_DETOUR.set(d1);
        let _ = APPEND_DETOUR.set(d2);
        let _ = LEGEND_DETOUR.set(d3);
    }
    log_info!("SMarvelous: judgement-graph detours installed");
    true
}

pub fn activate() {
    ACTIVE.store(true, Ordering::Release);
}

pub fn deactivate() {
    ACTIVE.store(false, Ordering::Release);
    clear_tabs();
}

/// Scene changed — tab objects die and their allocations recycle; drop
/// every registration. Called from the mod's scene callback.
pub fn on_scene_change() {
    clear_tabs();
}

fn clear_tabs() {
    if let Ok(mut guard) = TABS.lock() {
        if let Some(map) = guard.as_mut() {
            map.clear();
        }
    }
}

// ── Rebuild detour: per-tab one-shot subtraction + registration ──────

unsafe extern "C" fn rebuild_hook(tab: *mut u8) -> u64 {
    if ACTIVE.load(Ordering::Acquire) && !tab.is_null() {
        if let Err(e) = std::panic::catch_unwind(|| prepare_tab(tab)) {
            let _ = e;
        }
    }
    match REBUILD_DETOUR.get() {
        Some(d) => d.call(tab),
        None => 0,
    }
}

/// One-shot per tab: compute our per-second vector, subtract it from the
/// game's marvelous/shimmer series, register the tab for injection.
fn prepare_tab(tab: *mut u8) {
    unsafe {
        if memory::read_u8(tab.add(TAB_HAS_DATA)) == 0 {
            return; // no ingest data (also: never inject on this tab)
        }
        {
            let mut guard = match TABS.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            let map = guard.get_or_insert_with(HashMap::new);
            if map.contains_key(&(tab as usize)) {
                return; // already prepared
            }
            // Reserve the slot immediately (any failure below leaves the
            // tab registered as "nothing to draw" — stock look, no
            // retries every frame).
            map.insert(
                tab as usize,
                TabState {
                    smarv: Vec::new(),
                    any: false,
                },
            );
        }

        let side = memory::read_i32(tab.add(TAB_RECORD_SIDE));
        let stage = memory::read_i32(tab.add(TAB_STAGE));
        if !(0..=1).contains(&side) {
            return;
        }
        let window = state::last_armed_window(side as usize);
        if window <= 0 {
            return;
        }
        if stage_records::side_entered(side as usize) == Some(false) {
            return;
        }
        let record = if results_score::course_active() {
            stage_records::course_record(side as usize)
        } else if stage >= 0 {
            stage_records::stage_record(side as usize, stage as usize)
        } else {
            None
        };
        let Some(record) = record else {
            warn_once(
                &WARN_RECORD,
                "SMarvelous: graph record unavailable -- graph stays stock",
            );
            return;
        };
        if memory::read_i32(record) == -1 {
            return; // virgin record
        }
        let (Some((grades, errors)), Some(notes)) = (
            records::read_streams(record),
            records::read_note_refs(record),
        ) else {
            warn_once(
                &WARN_RECORD,
                "SMarvelous: graph record streams unreadable -- graph stays stock",
            );
            return;
        };
        let Some(mut smarv) = records::smarv_per_second(&notes, &grades, &errors, window) else {
            warn_once(
                &WARN_RECORD,
                "SMarvelous: graph bucketing failed -- graph stays stock",
            );
            return;
        };

        // Match the game's series length (our vector can be shorter when
        // the song's tail seconds hold no S-Marv; never longer — same
        // bucketing — but clamp defensively).
        let Some((marv_ptr, marv_len)) = series_slice(tab, SERIES_MARVELOUS) else {
            return;
        };
        let Some((shim_ptr, shim_len)) = series_slice(tab, SERIES_SHIMMER) else {
            return;
        };
        smarv.truncate(marv_len);
        smarv.resize(marv_len, 0.0);

        // Subtract per second from whichever series holds the second's
        // marvelous content (the ingest's shimmer post-pass moves pure
        // seconds' counts 5 → 6).
        let mut leftover = 0.0f64;
        let mut any = false;
        for (s, &c) in smarv.iter().enumerate() {
            if c <= 0.0 {
                continue;
            }
            any = true;
            let mut rem = c;
            let m = marv_ptr.add(s);
            let taken = rem.min(*m);
            *m -= taken;
            rem -= taken;
            if rem > 0.0 && s < shim_len {
                let sh = shim_ptr.add(s);
                let taken = rem.min(*sh);
                *sh -= taken;
                rem -= taken;
            }
            leftover += rem;
        }
        if leftover > 0.0 {
            warn_once(
                &WARN_LEFTOVER,
                "SMarvelous: graph subtraction leftover (series mismatch?) -- clamped",
            );
        }

        if let Ok(mut guard) = TABS.lock() {
            if let Some(st) = guard.as_mut().and_then(|m| m.get_mut(&(tab as usize))) {
                st.smarv = smarv;
                st.any = any;
            }
        }
        if any && !FIRST_INJECT_LOGGED.swap(true, Ordering::Relaxed) {
            log_info!(
                "SMarvelous: graph series prepared (side {}, {} buckets)",
                side,
                marv_len
            );
        }
    }
}

/// Mutable view of a judge-series `vector<double>` on the tab.
unsafe fn series_slice(tab: *mut u8, offset: usize) -> Option<(*mut f64, usize)> {
    let begin = memory::read_ptr(tab.add(offset)) as *mut f64;
    let end = memory::read_ptr(tab.add(offset + 8)) as usize;
    if begin.is_null() || end < begin as usize {
        return None;
    }
    let len = (end - begin as usize) / 8;
    if len > 4096 {
        return None;
    }
    Some((begin, len))
}

// ── Chart-append detour: series injection ────────────────────────────

unsafe extern "C" fn append_hook(
    chart: *mut u8,
    vec: *mut MsvcVec<f64>,
    callable: *mut ColorCallable,
) -> u64 {
    // Capture the color functor's impl vftable BEFORE the original runs —
    // the append CONSUMES the callable (impl ptr nulled on return).
    let vft = if ACTIVE.load(Ordering::Acquire) && !callable.is_null() {
        let impl_ptr = callable.read_unaligned().impl_ptr;
        if impl_ptr.is_null() {
            0
        } else {
            memory::read_ptr(impl_ptr as *const u8) as usize
        }
    } else {
        0
    };
    let ret = match APPEND_DETOUR.get() {
        Some(d) => d.call(chart, vec, callable),
        None => 0,
    };
    // POST-original on the FILLER series: our violet series is appended
    // right after it, BEFORE the shimmer/marvelous pair — the S-Marv tier
    // leads the judge stack (maintainer directive 2026-08-30: S-Marv
    // first).
    if vft != 0 {
        if let Err(e) = std::panic::catch_unwind(|| maybe_inject_series(chart, vec, vft)) {
            let _ = e;
        }
    }
    ret
}

/// If this append was a registered tab's FILLER series on the judge page,
/// append our S-Marv series next so it leads the judge-colored series
/// (the functor vftable was live-captured from the incoming argument —
/// same `(uint,double,double)` lambda family, so a `{vft, violet}` clone
/// is a valid color functor). Our injected call re-enters the hook but
/// fails the vec-identity gate (its source is our stack view).
fn maybe_inject_series(chart: *mut u8, vec: *mut MsvcVec<f64>, vft: usize) {
    unsafe {
        if chart.is_null() || vec.is_null() {
            return;
        }
        // vec == tab + SERIES_FILLER for a registered tab?
        let tab = (vec as usize).wrapping_sub(SERIES_FILLER) as *mut u8;
        // Snapshot under the lock; NEVER call game code while holding it.
        let (begin, len) = {
            let guard = match TABS.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            let Some(st) = guard.as_ref().and_then(|m| m.get(&(tab as usize))) else {
                return;
            };
            if !st.any {
                return;
            }
            (st.smarv.as_ptr(), st.smarv.len())
        };
        if memory::read_i32(tab.add(TAB_PAGE)) != 0 {
            return; // judge page only
        }

        let mut our_vec = MsvcVec::<f64> {
            begin,
            end: begin.add(len),
            cap_end: begin.add(len),
        };
        let mut our_callable = ColorCallable {
            vft,
            rgba: VIOLET_RGBA,
            _pad: 0,
            _pad2: 0,
            impl_ptr: std::ptr::null_mut(),
        };
        our_callable.impl_ptr = &mut our_callable;
        if let Some(d) = APPEND_DETOUR.get() {
            d.call(chart, &mut our_vec, &mut our_callable);
        }
    }
}

// ── Legend detour: "■S-MARVELOUS" line ───────────────────────────────

unsafe extern "C" fn legend_hook(ctx: *mut u8, text: *const MsvcString, rgba: u32) -> u64 {
    // PRE-original on the stock white "■MARVELOUS" line: our violet entry
    // goes in first, so S-Marv leads the judge legend (maintainer
    // directive 2026-08-30).
    if ACTIVE.load(Ordering::Acquire) && rgba == MARVELOUS_LEGEND_RGBA {
        if let Err(e) = std::panic::catch_unwind(|| maybe_inject_legend(ctx)) {
            let _ = e;
        }
    }
    match LEGEND_DETOUR.get() {
        Some(d) => d.call(ctx, text, rgba),
        None => 0,
    }
}

/// Before the stock "■MARVELOUS" line of a registered judge-page tab,
/// append ours — the caller's stack ctx (rect + running cursor) is live,
/// so the original does all the layout. Our own injected call re-enters
/// the hook with the violet rgba and passes straight through.
fn maybe_inject_legend(ctx: *mut u8) {
    unsafe {
        if ctx.is_null() {
            return;
        }
        // ctx = {rect block*, cursor*, tab*} (RE §4 of the rebuild dump).
        let tab = memory::read_ptr(ctx.add(0x10)) as *mut u8;
        if tab.is_null() {
            return;
        }
        let draw = {
            let guard = match TABS.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            guard
                .as_ref()
                .and_then(|m| m.get(&(tab as usize)))
                .is_some_and(|st| st.any)
        };
        if !draw || memory::read_i32(tab.add(TAB_PAGE)) != 0 {
            return;
        }
        let text = MsvcString::sso_bytes(LEGEND_TEXT);
        if let Some(d) = LEGEND_DETOUR.get() {
            d.call(ctx, &text, VIOLET_RGBA);
        }
    }
}
