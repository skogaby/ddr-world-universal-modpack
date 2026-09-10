//! Results judgement graph — the S-Marvelous series + legend entry
//! (design §4.8, plan Step 8), the shimmer-gradient transplant (2026-09-10)
//! and, on the TIMING page, the Marvelous FAST/SLOW series + legend
//! entries (2026-09-10 follow-up).
//!
//! The GraphTab's vslot-6 ingest aggregates the record's per-note streams
//! into per-second `vector<double>` series on the tab (judge chart at
//! `tab+0x538+k*0x20`: [0] filler, [1] miss, [2] good, [3] great,
//! [4] perfect, [5] marvelous+O.K., [6] all-marvelous shimmer; timing
//! charts at `tab+0x378+k*0x20`: [1..=4] FAST miss/good/great/perfect,
//! [5] grade-0/6 (NEVER drawn), [6..=9] SLOW perfect/great/good/miss), and
//! the vslot-7 rebuild (`graph_tab_rebuild`) clears + rebuilds every chart
//! and legend text EVERY FRAME. Five cooperating detours, zero absolute
//! position math (2026-08-30 RE — `progress.md` Step 8 entry; timing page
//! + gradient RE 2026-09-10):
//!
//! **The stock shimmer gradient.** The ingest's post-pass moves a second's
//! marvelous count 5 → 6 when EVERY other judge series is empty there
//! (filler/miss/good/great/perfect ≤ 0) — series 6 is the "100 % top tier"
//! flag, drawn through the TWO-COLOUR append (`graph_chart_append_2c`)
//! with a functor whose per-cell inner colour function returns `v > 0.5 ?
//! c0 : c1` at the quad's four corners (cyan `A9FEEC` bottom, pink
//! `DEA7EF` top ⇒ the pearlescent vertical gradient). Series 5 (mixed
//! seconds) is flat near-white alternating per second (`F0F0F0`/`ECE9EC`
//! by bucket parity). With the mod on, S-Marvelous is the top tier, so the
//! gradient moves with it: pure-S-Marvelous seconds draw violet → light
//! violet, mixed seconds flat violet, and Marvelous never shimmers.
//!
//! 1. **Rebuild detour (pre-original, one-shot per tab)**: build our
//!    per-second VIOLET vector from the stage record (mirror bucketing —
//!    [`records::violet_per_second`]: S-Marvelous hits PLUS freeze O.K.s,
//!    since the stock ingest folds grade 6 into the marvelous+O.K. series
//!    as the highest tier it knows, and with the mod on that tier is
//!    violet — 2026-09-03), FOLD the shimmer series (+0x5F8) back into the
//!    marvelous series (+0x5D8) so no Marvelous cell draws the gradient,
//!    SUBTRACT the violet vector from the marvelous series per second
//!    (clamped), then SPLIT the violet vector into PURE seconds (nothing
//!    left in any other judge series — [`records::split_pure_seconds`],
//!    the stock post-pass condition on the new top tier) and MIXED ones,
//!    build the LOOSE-Marvelous FAST/SLOW vectors for the timing page
//!    ([`records::marvelous_fast_slow_per_second`] — stock never draws
//!    grade 0 there because Marvelous was the exempt top tier; with the
//!    mod on that tier is S-Marvelous), and register the tab. Also resets
//!    the per-frame legend state machine.
//! 2. **Chart-append detour** (`graph_chart_append` — the single-color
//!    series append the rebuild calls once per series): after the FILLER
//!    series (`vec == tab+0x538`) of a registered tab on the judge page,
//!    append our MIXED violet series (flat) — the callable's lambda
//!    vftable is captured live from the incoming argument BEFORE the
//!    original consumes it (same `uint(double,double)` family, so a
//!    `{vft, violet}` clone of it is a valid color functor). Our series
//!    therefore LEADS the judge-colored series (S-Marv first — maintainer
//!    directive 2026-08-30). On the TIMING page, after the FAST PERFECT
//!    append (`vec == tab+0x3F8`) / SLOW PERFECT append (`vec ==
//!    tab+0x438`) our Marvelous FAST/SLOW series are appended LAST — the
//!    bar renderer stacks the last-appended series at the axis, so
//!    Marvelous sits between the two PERFECT bands exactly where the best
//!    tier belongs. The timing lambdas are HSV-shifting functors
//!    (`cdfd0(rgba@+0xC, 0, 0, value_shift)` with the shift baked into the
//!    lambda's code), so we capture the GREAT (shift 0 = identity)
//!    lambda's vftable from the `tab+0x3D8` / `tab+0x458` appends of the
//!    same frame and carry our rgba at +0xC. Re-injection every frame is
//!    automatic (charts are rebuilt per frame).
//! 3. **Two-colour append detour** (`graph_chart_append_2c`): after the
//!    stock SHIMMER append (`vec == tab+0x5F8`, now an all-zero series) of
//!    a registered judge-page tab, append our PURE violet series through
//!    the same function with a `{vft17, light-violet, violet}` clone of
//!    the incoming shimmer functor (vftable captured live pre-original) —
//!    the renderer builds the stock inner `v > 0.5 ? c0 : c1` functor per
//!    cell, so our cells get exactly the stock gradient in our colours.
//!    Optional half: when the signature is missing the pure seconds ride
//!    the flat MIXED series instead (nothing is lost, only the gradient).
//! 4. **Legend detour** (`graph_legend_text`): judge page — when the
//!    stock white "■MARVELOUS" legend line (rgba 0xF0F0F0FF, registered
//!    tab) arrives, first call the original with our own "■MARVELOUS" in
//!    VIOLET (no "S-" prefix — matches the shipped art language), then
//!    pass the stock call through. Timing page — the stock single legend
//!    row (NOTES/SEC + 8 entries) already spans the panel, so the FAST
//!    group is lifted onto a second line ABOVE the stock one (mirroring
//!    the chart: FAST bars up, SLOW bars down) and one "■MARVELOUS" is
//!    appended to each group next to its PERFECT: on "■FAST MISS" save
//!    the cursor and lift the legend rect's y by one line; pass FAST
//!    MISS/GOOD/GREAT/PERFECT through; after the FAST PERFECT inject the
//!    light-green MARVELOUS, drop the rect back, rewind the cursor to the
//!    saved start and inject the goldenrod MARVELOUS as the first item of
//!    the lower (SLOW) line; the stock PERFECT/GREAT/GOOD/SLOW MISS then
//!    follow it. The caller's stack context (rect block + running
//!    x-cursor) still does all layout — the mod only nudges y/cursor.
//! 5. **Axis detour** (`graph_timing_axis_max`, post-original): the
//!    timing charts' ±y range is the tallest stock FAST/SLOW stack rounded
//!    up to even; ours can be taller with the Marvelous band, and the bar
//!    renderer does NOT clip — so fold our series into the same max
//!    (`max(stock, 2*ceil(ours/2))`). Without this detour the timing
//!    injection is disabled outright (fail-open to the stock timing
//!    graph). The timing page has NO stock gradient (all eight series are
//!    single-colour appends), so nothing to transplant there.
//!
//! Re-entry safety: our own injected calls fail the gates (different
//! vec / different rgba) and the legend path additionally carries an
//! `INJECTING` flag; the registry lock is NEVER held across a call into
//! game code (same-thread `Mutex` re-entry deadlocks). Registry keyed by
//! tab pointer, cleared on every scene change (tab allocations recycle).
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
const TAB_PAGE: usize = 0x138;
const TAB_PAGE_JUDGE: i32 = 0;
const TAB_PAGE_TIMING: i32 = 1;
const TAB_HAS_DATA: usize = 0x1C4;
/// Judge-series vector<double> groups (0x20 stride): [0] filler (unjudged),
/// [1] miss, [2] good, [3] great, [4] perfect, [5] marvelous+O.K. (mixed
/// seconds), [6] all-marvelous shimmer (pure seconds, gradient).
const SERIES_FILLER: usize = 0x538;
const SERIES_MISS: usize = 0x558;
const SERIES_GOOD: usize = 0x578;
const SERIES_GREAT: usize = 0x598;
const SERIES_PERFECT: usize = 0x5B8;
const SERIES_MARVELOUS: usize = 0x5D8;
const SERIES_SHIMMER: usize = 0x5F8;
/// The judge series a second must be EMPTY in (after our fold/subtraction)
/// to count as pure top tier — the stock post-pass's test set, with the
/// marvelous series standing in for the loose Marvelous the mod leaves
/// there.
const JUDGE_OTHER_SERIES: [usize; 6] = [
    SERIES_FILLER,
    SERIES_MISS,
    SERIES_GOOD,
    SERIES_GREAT,
    SERIES_PERFECT,
    SERIES_MARVELOUS,
];
/// Timing-series vector<double> groups (0x20 stride from +0x378; RE
/// 2026-09-10). The rebuild appends the FAST chart as miss→good→great→
/// perfect and the SLOW chart as miss→good→great→perfect too (+0x498,
/// +0x478, +0x458, +0x438), so PERFECT is last = nearest the axis on both.
const SERIES_FAST_GREAT: usize = 0x3D8;
const SERIES_FAST_PERFECT: usize = 0x3F8;
const SERIES_SLOW_PERFECT: usize = 0x438;
const SERIES_SLOW_GREAT: usize = 0x458;
/// The 8 drawn timing series the stock axis-max scans (FAST then SLOW).
const TIMING_FAST_SERIES: [usize; 4] = [0x398, 0x3B8, 0x3D8, 0x3F8];
const TIMING_SLOW_SERIES: [usize; 4] = [0x438, 0x458, 0x478, 0x498];

/// Art-matched deep violet (the combo tint pair's deep member), RGBA.
const VIOLET_RGBA: u32 = 0xB05C_E0FF;
/// The light end of the pure-S-Marvelous gradient (violet tinted ~45 %
/// toward white). Stock's shimmer puts its lighter colour (cyan) at the
/// BOTTOM of the bar and the darker (pink) at the top; we mirror that:
/// violet top → light violet bottom. Tunable.
const VIOLET_LIGHT_RGBA: u32 = 0xD4A5_EEFF;
/// Timing-page Marvelous bands (maintainer choice 2026-09-10): light
/// green for FAST (early), goldenrod for SLOW (late). Applied through the
/// identity (shift-0) GREAT lambda, so these render as-is.
const FAST_MARV_RGBA: u32 = 0x90EE_90FF;
const SLOW_MARV_RGBA: u32 = 0xDAA5_20FF;
/// "■MARVELOUS" — SJIS ■ (81 A1) + ASCII, 11 bytes (SSO-safe). The
/// S-Marv legend entry deliberately reads "MARVELOUS" in violet (no "S-"
/// prefix) to match the rest of the shipped art language (maintainer
/// directive 2026-08-30), and is injected BEFORE the stock white entry
/// (first judge item). The timing page reuses the same word for the two
/// Marvelous FAST/SLOW bands.
const LEGEND_TEXT: &[u8] = b"\x81\xA1MARVELOUS";
/// The stock "■MARVELOUS" legend line's color — the injection anchor.
const MARVELOUS_LEGEND_RGBA: u32 = 0xF0F0_F0FF;
/// Stock timing-page legend strings (SJIS ■ prefix), in call order:
/// NOTES/SEC, FAST MISS, GOOD, GREAT, PERFECT, PERFECT, GREAT, GOOD,
/// SLOW MISS. The state machine keys on these, not on the HSV-shifted
/// rgba values.
const LEGEND_FAST_MISS: &[u8] = b"\x81\xA1FAST MISS";
const LEGEND_GOOD: &[u8] = b"\x81\xA1GOOD";
const LEGEND_GREAT: &[u8] = b"\x81\xA1GREAT";
const LEGEND_PERFECT: &[u8] = b"\x81\xA1PERFECT";
/// Vertical pitch of the lifted FAST legend line, in LOGICAL canvas px
/// (the 1280×720 design canvas — never output px). The legend text renders
/// at scale 0.6 (~5 px glyph band) bottom-aligned at `rect.y − 1`; the row
/// above (Timing average / Variation) ends ~19 px higher, so 11 px leaves
/// ~6 px between the lines and ~8 px to that row. Tunable.
const LEGEND_LINE_HEIGHT: f64 = 11.0;

// ── Game ABIs ────────────────────────────────────────────────────────

/// GraphTab rebuild — vslot 7, `fn(this)`.
type RebuildFn = unsafe extern "C" fn(*mut u8) -> u64;
/// Chart series append: (chart, &vector<double>, &callable). The
/// single-colour append (`graph_chart_append`) and the two-colour append
/// (`graph_chart_append_2c`) share this ABI; only the callable's functor
/// TYPE differs (see [`ColorCallable`]).
type ChartAppendFn = unsafe extern "C" fn(*mut u8, *mut MsvcVec<f64>, *mut ColorCallable) -> u64;
/// Legend line: (&ctx {rect*, cursor*, tab*}, &string, rgba).
type LegendFn = unsafe extern "C" fn(*mut u8, *const MsvcString, u32) -> u64;
/// Timing axis half-max: (&{tab*, judge_axis_max: i32}) -> i32.
type AxisMaxFn = unsafe extern "C" fn(*mut u8) -> i32;

/// The rebuild's stack shape for the color functor: a 0x20-byte MSVC
/// `std::function` with inline storage — `{impl vftable, 8 bytes of
/// captures, pad, impl_ptr → self}`. The append clones the impl through
/// vft slot 0 (copies the whole 8-byte capture word) and CONSUMES the
/// object (impl_ptr nulled), so a fresh stack instance per call is exactly
/// what the stock caller does too. Capture layout differs per lambda
/// family: the JUDGE single-colour lambdas return the rgba stored at +0x8
/// (`word_lo`); the TIMING lambdas run `cdfd0(rgba@+0xC, 0, 0, shift)` —
/// rgba at `word_hi`, `word_lo` unused (stale stack in the stock caller);
/// the two-colour SHIMMER lambda (`lambda17`, outer type
/// `function<uint(double,double)>(int,int)`) carries `c0 @+0x8` (drawn at
/// the BOTTOM corners, `v > 0.5`) and `c1 @+0xC` (TOP corners).
#[repr(C)]
struct ColorCallable {
    vft: usize,
    word_lo: u32,
    word_hi: u32,
    _pad2: u64,
    impl_ptr: *mut ColorCallable,
}

impl ColorCallable {
    /// Judge-page family: rgba at +0x8.
    fn judge(vft: usize, rgba: u32) -> Self {
        ColorCallable {
            vft,
            word_lo: rgba,
            word_hi: 0,
            _pad2: 0,
            impl_ptr: std::ptr::null_mut(),
        }
    }

    /// Timing-page family: rgba at +0xC.
    fn timing(vft: usize, rgba: u32) -> Self {
        ColorCallable {
            vft,
            word_lo: 0,
            word_hi: rgba,
            _pad2: 0,
            impl_ptr: std::ptr::null_mut(),
        }
    }

    /// Two-colour shimmer family: `bottom` at +0x8 (`v > 0.5`), `top` at
    /// +0xC — the inner `lambda47` functor the outer builds per cell.
    fn gradient(vft: usize, top: u32, bottom: u32) -> Self {
        ColorCallable {
            vft,
            word_lo: bottom,
            word_hi: top,
            _pad2: 0,
            impl_ptr: std::ptr::null_mut(),
        }
    }
}

// ── State ────────────────────────────────────────────────────────────

static REBUILD_DETOUR: once_cell::sync::OnceCell<GenericDetour<RebuildFn>> =
    once_cell::sync::OnceCell::new();
static APPEND_DETOUR: once_cell::sync::OnceCell<GenericDetour<ChartAppendFn>> =
    once_cell::sync::OnceCell::new();
static APPEND_2C_DETOUR: once_cell::sync::OnceCell<GenericDetour<ChartAppendFn>> =
    once_cell::sync::OnceCell::new();
static LEGEND_DETOUR: once_cell::sync::OnceCell<GenericDetour<LegendFn>> =
    once_cell::sync::OnceCell::new();
static AXIS_DETOUR: once_cell::sync::OnceCell<GenericDetour<AxisMaxFn>> =
    once_cell::sync::OnceCell::new();

/// Mod enabled (deactivate flips; detours stay installed but inert).
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// The timing-page half is only armed when the axis detour installed —
/// injecting taller stacks without the axis fix would draw past the chart.
static TIMING_AVAILABLE: AtomicBool = AtomicBool::new(false);
/// The pure-second GRADIENT half is only armed when the two-colour append
/// detour installed; otherwise pure seconds ride the flat series.
static GRADIENT_AVAILABLE: AtomicBool = AtomicBool::new(false);
/// Set while THIS module calls the legend helper itself, so the re-entrant
/// hook invocation passes straight through (single render thread).
static INJECTING: AtomicBool = AtomicBool::new(false);

/// Where the timing-page legend rewrite is within the current frame's
/// legend call sequence (reset by the rebuild pre-hook every frame).
#[derive(Clone, Copy, PartialEq)]
enum LegendPhase {
    /// Waiting for "■FAST MISS".
    Idle,
    /// Inside the FAST group: the rect is lifted by one line; remember the
    /// lower line's cursor start and the stock rect y to put back.
    InFast { cursor_start: i32, rect_y: f64 },
    /// Both MARVELOUS entries injected (or aborted) — pass everything through.
    Done,
}

struct TabState {
    /// Our per-second violet counts for MIXED seconds (S-Marv + freeze
    /// O.K. in seconds that also hold something else), drawn FLAT after
    /// the filler append. When the gradient half is unavailable this holds
    /// EVERY violet second. Padded to the game series length.
    smarv: Vec<f64>,
    /// Our per-second violet counts for PURE seconds (nothing but S-Marv /
    /// O.K. in the second — the stock shimmer condition on the new top
    /// tier), drawn with the violet → light-violet GRADIENT after the
    /// stock shimmer append. Empty when the gradient half is unavailable.
    smarv_pure: Vec<f64>,
    /// Whether there is anything to draw on the judge page (all-zero
    /// vectors skip injection so we never add an empty series/legend line).
    any: bool,
    /// Whether `smarv_pure` holds any second (skips the gradient append).
    any_pure: bool,
    /// Per-second LOOSE-Marvelous FAST / SLOW counts for the timing page
    /// (padded to the game's timing series length).
    fast: Vec<f64>,
    slow: Vec<f64>,
    /// Timing-page injection armed for this tab: the vectors resolved
    /// against the game's timing series. Unlike the judge page's `any`
    /// gate this is NOT content-dependent — the two-line legend and both
    /// MARVELOUS entries appear on every applicable tab (zero-height bands
    /// draw nothing) so the layout never jumps between songs.
    timing_on: bool,
    /// Identity-shift lambda vftables captured this frame from the FAST /
    /// SLOW GREAT appends (0 = not seen yet this frame).
    fast_vft: usize,
    slow_vft: usize,
    legend: LegendPhase,
}

static TABS: Mutex<Option<HashMap<usize, TabState>>> = Mutex::new(None);

static WARN_RECORD: AtomicBool = AtomicBool::new(false);
static WARN_LEFTOVER: AtomicBool = AtomicBool::new(false);
static WARN_LEGEND_SEQ: AtomicBool = AtomicBool::new(false);
static FIRST_INJECT_LOGGED: AtomicBool = AtomicBool::new(false);
static FIRST_TIMING_LOGGED: AtomicBool = AtomicBool::new(false);

fn warn_once(latch: &AtomicBool, msg: &str) {
    if !latch.swap(true, Ordering::Relaxed) {
        log_warn!("{}", msg);
    }
}

// ── Install / lifecycle ──────────────────────────────────────────────

/// Resolve + install the detours. The three judge-page detours are
/// all-or-nothing; the two-colour append detour (pure-second gradient) and
/// the timing axis detour (timing-page half) are each optional. Fail-open.
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

    // Pure-second gradient (optional half). The two-colour append must be
    // a DIFFERENT function from the single-colour one (a shared match would
    // mean two detours on one address).
    match signatures.get_address("graph_chart_append_2c") {
        Some(append_2c) if append_2c != append => unsafe {
            let append_2c_fn: ChartAppendFn = std::mem::transmute(append_2c);
            match GenericDetour::new(append_2c_fn, append_2c_hook) {
                Ok(d) if d.enable().is_ok() => {
                    let _ = APPEND_2C_DETOUR.set(d);
                    GRADIENT_AVAILABLE.store(true, Ordering::Release);
                    log_info!("SMarvelous: judgement-graph gradient detour installed");
                }
                _ => log_warn!(
                    "SMarvelous: two-colour append detour install failed -- pure seconds draw flat"
                ),
            }
        },
        Some(_) => log_warn!(
            "SMarvelous: graph_chart_append_2c resolved onto graph_chart_append -- pure seconds draw flat"
        ),
        None => log_warn!(
            "SMarvelous: graph_chart_append_2c unresolved -- pure S-Marvelous seconds draw flat"
        ),
    }

    // Timing page (optional half).
    match signatures.get_address("graph_timing_axis_max") {
        Some(axis) => unsafe {
            let axis_fn: AxisMaxFn = std::mem::transmute(axis);
            match GenericDetour::new(axis_fn, axis_max_hook) {
                Ok(d4) if d4.enable().is_ok() => {
                    let _ = AXIS_DETOUR.set(d4);
                    TIMING_AVAILABLE.store(true, Ordering::Release);
                    log_info!("SMarvelous: timing-graph axis detour installed");
                }
                _ => log_warn!(
                    "SMarvelous: timing-axis detour install failed -- timing graph stays stock"
                ),
            }
        },
        None => {
            log_warn!("SMarvelous: graph_timing_axis_max unresolved -- timing graph stays stock")
        }
    }
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
        if let Err(e) = std::panic::catch_unwind(|| {
            prepare_tab(tab);
            reset_frame_state(tab);
        }) {
            let _ = e;
        }
    }
    match REBUILD_DETOUR.get() {
        Some(d) => d.call(tab),
        None => 0,
    }
}

/// Per-frame reset of the state the timing-page detours carry across the
/// rebuild's legend/append call sequence.
fn reset_frame_state(tab: *mut u8) {
    if let Ok(mut guard) = TABS.lock() {
        if let Some(st) = guard.as_mut().and_then(|m| m.get_mut(&(tab as usize))) {
            st.legend = LegendPhase::Idle;
            st.fast_vft = 0;
            st.slow_vft = 0;
        }
    }
}

/// One-shot per tab: compute our per-second vectors, subtract the violet
/// one from the game's marvelous/shimmer series, register the tab for
/// injection.
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
                    smarv_pure: Vec::new(),
                    any: false,
                    any_pure: false,
                    fast: Vec::new(),
                    slow: Vec::new(),
                    timing_on: false,
                    fast_vft: 0,
                    slow_vft: 0,
                    legend: LegendPhase::Idle,
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
        let Some(mut smarv) = records::violet_per_second(&notes, &grades, &errors, window) else {
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

        // 1. FOLD the stock all-marvelous shimmer series back into the
        //    marvelous series: with S-Marvelous on, Marvelous is no longer
        //    the top tier, so no Marvelous cell draws the "100 % top tier"
        //    gradient (the ingest's post-pass had moved pure seconds'
        //    counts 5 → 6). The shimmer append still runs on the now-empty
        //    series and draws nothing.
        for s in 0..marv_len.min(shim_len) {
            let sh = shim_ptr.add(s);
            if *sh > 0.0 {
                *marv_ptr.add(s) += *sh;
                *sh = 0.0;
            }
        }

        // 2. SUBTRACT our violet counts from the marvelous series per
        //    second (every S-Marv/O.K. slot is in it by construction after
        //    the fold — clamp defensively, WARN on leftover).
        let mut leftover = 0.0f64;
        let mut any = false;
        for (s, &c) in smarv.iter().enumerate() {
            if c <= 0.0 {
                continue;
            }
            any = true;
            let m = marv_ptr.add(s);
            let taken = c.min(*m);
            *m -= taken;
            leftover += c - taken;
        }
        if leftover > 0.0 {
            warn_once(
                &WARN_LEFTOVER,
                "SMarvelous: graph subtraction leftover (series mismatch?) -- clamped",
            );
        }

        // 3. SPLIT the violet vector: seconds where NOTHING else remains
        //    in the judge stack (filler/miss/good/great/perfect AND the
        //    loose Marvelous left in the marvelous series) are PURE top
        //    tier and take the stock shimmer gradient; the rest draw flat.
        //    Without the two-colour detour everything stays flat.
        let (smarv, smarv_pure) = if GRADIENT_AVAILABLE.load(Ordering::Acquire) {
            let others: Vec<&[f64]> = JUDGE_OTHER_SERIES
                .iter()
                .filter_map(|&off| series_slice(tab, off))
                .map(|(p, len)| std::slice::from_raw_parts(p as *const f64, len))
                .collect();
            if others.len() == JUDGE_OTHER_SERIES.len() {
                let (pure, mixed) = records::split_pure_seconds(&smarv, &others);
                (mixed, pure)
            } else {
                warn_once(
                    &WARN_RECORD,
                    "SMarvelous: judge series unreadable -- pure seconds draw flat",
                );
                (smarv, Vec::new())
            }
        } else {
            (smarv, Vec::new())
        };

        // Timing page: the loose-Marvelous FAST/SLOW bands. Nothing to
        // subtract — stock never draws grade 0 on this page. Padded to the
        // timing series length (all 10 are resized together by the ingest).
        // Armed regardless of content (see `TabState::timing_on`).
        let (fast, slow, timing_on) = if TIMING_AVAILABLE.load(Ordering::Acquire) {
            match (
                records::marvelous_fast_slow_per_second(&notes, &grades, &errors, window),
                series_slice(tab, TIMING_FAST_SERIES[0]),
            ) {
                (Some((mut fast, mut slow)), Some((_, timing_len))) => {
                    fast.truncate(timing_len);
                    fast.resize(timing_len, 0.0);
                    slow.truncate(timing_len);
                    slow.resize(timing_len, 0.0);
                    (fast, slow, true)
                }
                _ => {
                    warn_once(
                        &WARN_RECORD,
                        "SMarvelous: timing-graph bucketing failed -- timing graph stays stock",
                    );
                    (Vec::new(), Vec::new(), false)
                }
            }
        } else {
            (Vec::new(), Vec::new(), false)
        };

        let pure_seconds = smarv_pure.iter().filter(|&&c| c > 0.0).count();
        if let Ok(mut guard) = TABS.lock() {
            if let Some(st) = guard.as_mut().and_then(|m| m.get_mut(&(tab as usize))) {
                st.smarv = smarv;
                st.smarv_pure = smarv_pure;
                st.any = any;
                st.any_pure = pure_seconds > 0;
                st.fast = fast;
                st.slow = slow;
                st.timing_on = timing_on;
            }
        }
        if any && !FIRST_INJECT_LOGGED.swap(true, Ordering::Relaxed) {
            log_info!(
                "SMarvelous: graph series prepared (side {}, {} buckets, {} pure S-Marv second(s) -> gradient)",
                side,
                marv_len,
                pure_seconds
            );
        }
        if timing_on && !FIRST_TIMING_LOGGED.swap(true, Ordering::Relaxed) {
            log_info!(
                "SMarvelous: timing-graph Marvelous series prepared (side {})",
                side
            );
        }
    }
}

/// Mutable view of a per-second `vector<double>` series on the tab.
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

/// Which stock append (of a registered tab) just ran — decides what we do
/// post-original.
#[derive(Clone, Copy)]
enum AppendRole {
    /// Judge page filler (`tab+0x538`, single-colour append): inject the
    /// MIXED (flat) violet series next.
    JudgeFiller,
    /// Judge page all-marvelous shimmer (`tab+0x5F8`, TWO-colour append —
    /// an all-zero series after our fold): inject the PURE violet series
    /// with the gradient functor next.
    JudgeShimmer,
    /// Timing FAST GREAT (`tab+0x3D8`): remember the identity lambda vft.
    FastGreat,
    /// Timing FAST PERFECT (`tab+0x3F8`): inject the Marvelous FAST band.
    FastPerfect,
    /// Timing SLOW GREAT (`tab+0x458`): remember the identity lambda vft.
    SlowGreat,
    /// Timing SLOW PERFECT (`tab+0x438`): inject the Marvelous SLOW band.
    SlowPerfect,
}

/// Roles reachable through the SINGLE-colour append.
const APPEND_ROLES: [(usize, AppendRole); 5] = [
    (SERIES_FILLER, AppendRole::JudgeFiller),
    (SERIES_FAST_GREAT, AppendRole::FastGreat),
    (SERIES_FAST_PERFECT, AppendRole::FastPerfect),
    (SERIES_SLOW_GREAT, AppendRole::SlowGreat),
    (SERIES_SLOW_PERFECT, AppendRole::SlowPerfect),
];
/// Roles reachable through the TWO-colour append.
const APPEND_2C_ROLES: [(usize, AppendRole); 1] = [(SERIES_SHIMMER, AppendRole::JudgeShimmer)];

/// Read the colour functor's impl vftable BEFORE the original runs — the
/// appends CONSUME the callable (impl ptr nulled on return). 0 when inert.
unsafe fn capture_callable_vft(callable: *mut ColorCallable) -> usize {
    if !ACTIVE.load(Ordering::Acquire) || callable.is_null() {
        return 0;
    }
    let impl_ptr = callable.read_unaligned().impl_ptr;
    if impl_ptr.is_null() {
        0
    } else {
        memory::read_ptr(impl_ptr as *const u8) as usize
    }
}

unsafe extern "C" fn append_hook(
    chart: *mut u8,
    vec: *mut MsvcVec<f64>,
    callable: *mut ColorCallable,
) -> u64 {
    let vft = capture_callable_vft(callable);
    let ret = match APPEND_DETOUR.get() {
        Some(d) => d.call(chart, vec, callable),
        None => 0,
    };
    // POST-original: judge FILLER ⇒ our (flat) violet series is appended
    // right after it, BEFORE the shimmer/marvelous pair — the S-Marv tier
    // leads the judge stack (maintainer directive 2026-08-30: S-Marv
    // first). Timing PERFECT ⇒ our Marvelous band is appended LAST = at
    // the axis.
    if vft != 0 {
        if let Err(e) =
            std::panic::catch_unwind(|| maybe_inject_series(chart, vec, vft, &APPEND_ROLES))
        {
            let _ = e;
        }
    }
    ret
}

unsafe extern "C" fn append_2c_hook(
    chart: *mut u8,
    vec: *mut MsvcVec<f64>,
    callable: *mut ColorCallable,
) -> u64 {
    let vft = capture_callable_vft(callable);
    let ret = match APPEND_2C_DETOUR.get() {
        Some(d) => d.call(chart, vec, callable),
        None => 0,
    };
    // POST-original: judge SHIMMER ⇒ our PURE violet series rides the same
    // two-colour path with a `{vft17, light, violet}` clone of the stock
    // shimmer functor — the stock gradient in our colours.
    if vft != 0 {
        if let Err(e) =
            std::panic::catch_unwind(|| maybe_inject_series(chart, vec, vft, &APPEND_2C_ROLES))
        {
            let _ = e;
        }
    }
    ret
}

/// Resolve which registered tab + stock series this append was for (the
/// vec argument is `tab + series_offset` for the game's own appends; our
/// injected stack vecs never resolve to a registered tab).
fn classify_append(
    vec: *mut MsvcVec<f64>,
    roles: &[(usize, AppendRole)],
) -> Option<(*mut u8, AppendRole)> {
    let guard = TABS.lock().ok()?;
    let map = guard.as_ref()?;
    for &(offset, role) in roles {
        let tab = (vec as usize).wrapping_sub(offset);
        if map.contains_key(&tab) {
            return Some((tab as *mut u8, role));
        }
    }
    None
}

/// If this append was one of a registered tab's gated series, act on it:
/// judge FILLER ⇒ append our MIXED S-Marv series flat (the functor vftable
/// was live-captured from the incoming argument — same `uint(double,
/// double)` lambda family, so a `{vft, violet}` clone is a valid color
/// functor); judge SHIMMER ⇒ append our PURE S-Marv series through the
/// two-colour append with a `{vft17, light, violet}` clone of the stock
/// shimmer functor (gradient); timing GREAT ⇒ remember its identity-shift
/// vftable; timing PERFECT ⇒ append the Marvelous FAST/SLOW band with that
/// vftable. Our injected call re-enters the hook but fails the vec-identity
/// gate (its source is our stack view).
fn maybe_inject_series(
    chart: *mut u8,
    vec: *mut MsvcVec<f64>,
    vft: usize,
    roles: &[(usize, AppendRole)],
) {
    unsafe {
        if chart.is_null() || vec.is_null() {
            return;
        }
        let Some((tab, role)) = classify_append(vec, roles) else {
            return;
        };
        let page = memory::read_i32(tab.add(TAB_PAGE));
        // Snapshot under the lock; NEVER call game code while holding it.
        let (begin, len, rgba, use_vft) = {
            let mut guard = match TABS.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            let Some(st) = guard.as_mut().and_then(|m| m.get_mut(&(tab as usize))) else {
                return;
            };
            match role {
                AppendRole::JudgeFiller => {
                    if !st.any || page != TAB_PAGE_JUDGE {
                        return;
                    }
                    (st.smarv.as_ptr(), st.smarv.len(), VIOLET_RGBA, vft)
                }
                AppendRole::JudgeShimmer => {
                    if !st.any || !st.any_pure || page != TAB_PAGE_JUDGE {
                        return;
                    }
                    (
                        st.smarv_pure.as_ptr(),
                        st.smarv_pure.len(),
                        VIOLET_RGBA,
                        vft,
                    )
                }
                AppendRole::FastGreat => {
                    st.fast_vft = vft;
                    return;
                }
                AppendRole::SlowGreat => {
                    st.slow_vft = vft;
                    return;
                }
                AppendRole::FastPerfect => {
                    if !st.timing_on || page != TAB_PAGE_TIMING || st.fast_vft == 0 {
                        return;
                    }
                    (st.fast.as_ptr(), st.fast.len(), FAST_MARV_RGBA, st.fast_vft)
                }
                AppendRole::SlowPerfect => {
                    if !st.timing_on || page != TAB_PAGE_TIMING || st.slow_vft == 0 {
                        return;
                    }
                    (st.slow.as_ptr(), st.slow.len(), SLOW_MARV_RGBA, st.slow_vft)
                }
            }
        };

        let mut our_vec = MsvcVec::<f64> {
            begin,
            end: begin.add(len),
            cap_end: begin.add(len),
        };
        let mut our_callable = match role {
            AppendRole::JudgeFiller => ColorCallable::judge(use_vft, rgba),
            AppendRole::JudgeShimmer => {
                ColorCallable::gradient(use_vft, VIOLET_RGBA, VIOLET_LIGHT_RGBA)
            }
            _ => ColorCallable::timing(use_vft, rgba),
        };
        our_callable.impl_ptr = &mut our_callable;
        let detour = match role {
            AppendRole::JudgeShimmer => APPEND_2C_DETOUR.get(),
            _ => APPEND_DETOUR.get(),
        };
        if let Some(d) = detour {
            d.call(chart, &mut our_vec, &mut our_callable);
        }
    }
}

// ── Axis detour: fold our timing bands into the ±y range ─────────────

unsafe extern "C" fn axis_max_hook(ctx: *mut u8) -> i32 {
    let stock = match AXIS_DETOUR.get() {
        Some(d) => d.call(ctx),
        None => 0,
    };
    if !ACTIVE.load(Ordering::Acquire) || ctx.is_null() {
        return stock;
    }
    match std::panic::catch_unwind(|| timing_axis_max(ctx, stock)) {
        Ok(v) => v,
        Err(_) => stock,
    }
}

/// `max(stock, 2*ceil(ours/2))` where `ours` is the tallest per-second
/// FAST or SLOW stack INCLUDING our Marvelous band — the stock helper's
/// rounding (`ceil(x*0.5)*2`, cabinet constants 0.5/2.0), applied to a
/// max that is ≥ the stock one, so the overall max is monotone-correct.
/// Reads the tab's series under the registry lock (memory reads only —
/// no game calls).
fn timing_axis_max(ctx: *mut u8, stock: i32) -> i32 {
    unsafe {
        let tab = memory::read_ptr(ctx) as *mut u8;
        if tab.is_null() {
            return stock;
        }
        let guard = match TABS.lock() {
            Ok(g) => g,
            Err(_) => return stock,
        };
        let Some(st) = guard.as_ref().and_then(|m| m.get(&(tab as usize))) else {
            return stock;
        };
        if !st.timing_on {
            return stock;
        }
        let fast_series = TIMING_FAST_SERIES.map(|off| series_slice(tab, off));
        let slow_series = TIMING_SLOW_SERIES.map(|off| series_slice(tab, off));
        let stack = |series: &[Option<(*mut f64, usize)>], ours: &[f64], s: usize| -> f64 {
            let mut total = ours.get(s).copied().unwrap_or(0.0);
            for sl in series.iter().flatten() {
                if s < sl.1 {
                    total += *sl.0.add(s);
                }
            }
            total
        };
        let len = st.fast.len().max(st.slow.len());
        let mut m = 0.0f64;
        for s in 0..len {
            m = m
                .max(stack(&fast_series, &st.fast, s))
                .max(stack(&slow_series, &st.slow, s));
        }
        if !m.is_finite() {
            return stock;
        }
        let ours = ((m * 0.5).ceil() * 2.0) as i32;
        stock.max(ours)
    }
}

// ── Legend detour: "■MARVELOUS" lines ────────────────────────────────

unsafe extern "C" fn legend_hook(ctx: *mut u8, text: *const MsvcString, rgba: u32) -> u64 {
    if INJECTING.load(Ordering::Acquire) || !ACTIVE.load(Ordering::Acquire) || ctx.is_null() {
        return match LEGEND_DETOUR.get() {
            Some(d) => d.call(ctx, text, rgba),
            None => 0,
        };
    }
    // PRE-original on the stock white "■MARVELOUS" line (judge page): our
    // violet entry goes in first, so S-Marv leads the judge legend
    // (maintainer directive 2026-08-30).
    if rgba == MARVELOUS_LEGEND_RGBA {
        if let Err(e) = std::panic::catch_unwind(|| maybe_inject_legend(ctx)) {
            let _ = e;
        }
    }
    // Timing page: the two-line relayout state machine (pre-original side
    // effects on the ctx: rect lift + cursor rewind).
    let post = match std::panic::catch_unwind(|| timing_legend_pre(ctx, text)) {
        Ok(p) => p,
        Err(_) => TimingPost::None,
    };
    let ret = match LEGEND_DETOUR.get() {
        Some(d) => d.call(ctx, text, rgba),
        None => 0,
    };
    if !matches!(post, TimingPost::None) {
        if let Err(e) = std::panic::catch_unwind(|| timing_legend_post(ctx, post)) {
            let _ = e;
        }
    }
    ret
}

/// Before the stock "■MARVELOUS" line of a registered judge-page tab,
/// append ours — the caller's stack ctx (rect + running cursor) is live,
/// so the original does all the layout.
fn maybe_inject_legend(ctx: *mut u8) {
    unsafe {
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
        if !draw || memory::read_i32(tab.add(TAB_PAGE)) != TAB_PAGE_JUDGE {
            return;
        }
        call_legend(ctx, LEGEND_TEXT, VIOLET_RGBA);
    }
}

/// Call the game's legend helper with our own text/colour, flagged so the
/// re-entrant hook invocation passes straight through.
unsafe fn call_legend(ctx: *mut u8, text: &[u8], rgba: u32) {
    let text = MsvcString::sso_bytes(text);
    if let Some(d) = LEGEND_DETOUR.get() {
        INJECTING.store(true, Ordering::Release);
        d.call(ctx, &text, rgba);
        INJECTING.store(false, Ordering::Release);
    }
}

/// Whether the legend argument is exactly `expected` (SSO strings only —
/// every stock timing legend string is ≤ 15 bytes; a heap string can never
/// match).
unsafe fn legend_text_is(text: *const MsvcString, expected: &[u8]) -> bool {
    if text.is_null() {
        return false;
    }
    let s = text.read_unaligned();
    s.cap <= 15 && s.len as usize == expected.len() && &s.buf[..expected.len()] == expected
}

/// What the timing state machine owes AFTER the original legend call.
#[derive(Clone, Copy)]
enum TimingPost {
    None,
    /// The FAST PERFECT line just went out: inject the FAST MARVELOUS on
    /// the lifted line, drop the rect back to `rect_y`, rewind the cursor
    /// to `cursor_start`, inject the SLOW MARVELOUS.
    AfterFastPerfect {
        cursor_start: i32,
        rect_y: f64,
    },
}

/// Timing-page legend state machine, pre-original. `ctx` = `{rect block*
/// (double[6]: x, y, _, w, h, _), cursor*: i32, tab*}`; the legend helper
/// places each text bottom-aligned at `(rect.x + cursor + 1, rect.y − 1)`
/// and advances the cursor by the text width + 10.
fn timing_legend_pre(ctx: *mut u8, text: *const MsvcString) -> TimingPost {
    unsafe {
        let tab = memory::read_ptr(ctx.add(0x10)) as *mut u8;
        let rect = memory::read_ptr(ctx) as *mut f64;
        let cursor = memory::read_ptr(ctx.add(8)) as *mut i32;
        if tab.is_null() || rect.is_null() || cursor.is_null() {
            return TimingPost::None;
        }
        if memory::read_i32(tab.add(TAB_PAGE)) != TAB_PAGE_TIMING {
            return TimingPost::None;
        }
        let mut guard = match TABS.lock() {
            Ok(g) => g,
            Err(_) => return TimingPost::None,
        };
        let Some(st) = guard.as_mut().and_then(|m| m.get_mut(&(tab as usize))) else {
            return TimingPost::None;
        };
        if !st.timing_on {
            return TimingPost::None;
        }
        match st.legend {
            LegendPhase::Idle => {
                if legend_text_is(text, LEGEND_FAST_MISS) {
                    let rect_y = *rect.add(1);
                    if !rect_y.is_finite() {
                        st.legend = LegendPhase::Done;
                        return TimingPost::None;
                    }
                    let cursor_start = *cursor;
                    // Lift the FAST group onto its own line above.
                    *rect.add(1) = rect_y - LEGEND_LINE_HEIGHT;
                    st.legend = LegendPhase::InFast {
                        cursor_start,
                        rect_y,
                    };
                }
                TimingPost::None
            }
            LegendPhase::InFast {
                cursor_start,
                rect_y,
            } => {
                if legend_text_is(text, LEGEND_PERFECT) {
                    // Original draws FAST PERFECT on the lifted line; the
                    // post step finishes both lines.
                    st.legend = LegendPhase::Done;
                    TimingPost::AfterFastPerfect {
                        cursor_start,
                        rect_y,
                    }
                } else if legend_text_is(text, LEGEND_GOOD) || legend_text_is(text, LEGEND_GREAT) {
                    TimingPost::None
                } else {
                    // Unexpected sequence — put the rect back and give up
                    // on this frame (stock single line, no Marvelous
                    // legend; the series still draw).
                    *rect.add(1) = rect_y;
                    st.legend = LegendPhase::Done;
                    warn_once(
                        &WARN_LEGEND_SEQ,
                        "SMarvelous: timing legend sequence unexpected -- legend stays stock",
                    );
                    TimingPost::None
                }
            }
            LegendPhase::Done => TimingPost::None,
        }
    }
}

/// Timing-page legend state machine, post-original (after the stock FAST
/// PERFECT line): finish the lifted line with the FAST MARVELOUS, then
/// start the lower line with the SLOW MARVELOUS so the stock SLOW group
/// follows it. No registry lock is held across the game calls.
fn timing_legend_post(ctx: *mut u8, post: TimingPost) {
    let TimingPost::AfterFastPerfect {
        cursor_start,
        rect_y,
    } = post
    else {
        return;
    };
    unsafe {
        let rect = memory::read_ptr(ctx) as *mut f64;
        let cursor = memory::read_ptr(ctx.add(8)) as *mut i32;
        if rect.is_null() || cursor.is_null() {
            return;
        }
        // Upper (FAST) line: the rect is still lifted.
        call_legend(ctx, LEGEND_TEXT, FAST_MARV_RGBA);
        // Lower (SLOW) line: back to the stock y, restart after NOTES/SEC.
        *rect.add(1) = rect_y;
        *cursor = cursor_start;
        call_legend(ctx, LEGEND_TEXT, SLOW_MARV_RGBA);
    }
}
