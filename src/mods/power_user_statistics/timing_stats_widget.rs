//! Timing Stats Widget — per-player text widget during gameplay showing
//! either the DETAILED set (EX loss, Current, Max, Abs Mean, Mean ms-error
//! and live calories) or the STREAMLINED set (Δ, Max Δ, EX loss and the
//! per-grade tallies), in one of three LAYOUTS: the original side column
//! (one field per line beside each playfield) or a single line along the
//! bottom or the top edge of the screen. The blocks stay on screen through
//! the stage results (0-idx 29 → 30) so the breakdown can be read without
//! gameplay pressure; they hide on any other scene.
//!
//! Sign convention for the signed readouts (Current, Δ, Max Δ, μ): POSITIVE
//! = FAST (early), NEGATIVE = SLOW (late) — the game's own results-graph
//! convention. The live judge delta the feed captures is the inverse, so the
//! composer negates at the display boundary; see `readout`.
//!
//! Layout (scale + a mirrored horizontal/vertical offset + alignment) is
//! cabinet-wide: seeded from the `power_user_statistics` config section at
//! enable and live-editable from the overlay menu's POWER USER STATISTICS
//! rows on the GLOBAL SETTINGS tab, which persist the whole section back.
//! The offsets are PER LAYOUT — each layout keeps its own pair, and the
//! offset rows always edit the ACTIVE layout's (they are re-registered in
//! place with the new layout's values and labels whenever the layout row
//! changes; `register_*_row` replaces by key, so their menu position never
//! moves). Alignment is the SIDE COLUMN's alone: the BOTTOM LINE and TOP
//! LINE bake in OUTER EDGE (a single line has nothing to align about — the
//! anchor already is its edge), so its row is removed while either is
//! active and re-added (still the group's last row) on the way back. Edits
//! re-lay out any already-created widgets immediately (render thread).
//!
//! The BOTTOM LINE layout occupies the band the stock CREDIT / PASELI /
//! ONLINE readouts draw in, so while its widgets are on screen (GAMEPLAY
//! entry through the results carry-over) the stock text is hidden through
//! `services::bottom_text` under this mod's own contributor bit — the
//! operator's `hide-bottom-text` toggle is independent. The hide follows the
//! widget PHASE, not per-side visibility (a calibration song, whose widget
//! never shows, still hides the stock line — harmless), and is re-evaluated
//! on every scene change and layout edit. The TOP LINE shares no band with
//! the stock text, so it never hides it (`Layout::hides_bottom_text`).

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::core::deferred_work::PendingPump;
use crate::mods::config;
use crate::services::bottom_text::{self, HideReason};
use crate::services::{custom_options, scene_manager, widget_renderer};
use crate::types::scenes::scene;
use crate::widgets::text_widget::{TextAlignment, TextWidget};
use crate::{log_info, log_warn};

use super::readout::{self, AlignmentRule, BlockAlignment, Content, Layout, LineAlign, Snapshot};
use super::{calorie_feed, data_feed};

// Layout row ranges. Scale is a percentage of `readout::STOCK_SCALE`; the
// per-layout offset ranges live in `readout::Geometry`.
pub const SCALE_MIN: i32 = 50;
pub const SCALE_MAX: i32 = 150;
pub const SCALE_DEFAULT: i32 = 100;
/// Offset rows: fine step 5 px, coarse step 50 px (the ranges are ~700 px
/// wide — 25 px coarse steps took too long to traverse).
const OFFSET_STEP_FINE: i32 = 5;
const OFFSET_STEP_COARSE: i32 = 50;

/// Overlay row keys (GLOBAL SETTINGS, grouped under this mod's header;
/// registration order = display order: scale, layout, content, then the
/// layout-scoped offset rows and — SIDE COLUMN only — the alignment row).
const SCALE_ROW_KEY: &str = "pus_widget_scale";
const LAYOUT_ROW_KEY: &str = "pus_widget_layout";
const CONTENT_ROW_KEY: &str = "pus_widget_content";
const OFFSET_X_ROW_KEY: &str = "pus_widget_offset_x";
const OFFSET_Y_ROW_KEY: &str = "pus_widget_offset_y";
const ALIGN_ROW_KEY: &str = "pus_widget_alignment";
const ALL_ROW_KEYS: [&str; 6] = [
    SCALE_ROW_KEY,
    LAYOUT_ROW_KEY,
    CONTENT_ROW_KEY,
    OFFSET_X_ROW_KEY,
    OFFSET_Y_ROW_KEY,
    ALIGN_ROW_KEY,
];
const OWNING_MOD_ID: &str = "power-user-statistics";

/// Live layout values (already clamped). Written by config seeding and the
/// overlay rows; read on every (re)layout.
static LIVE_SCALE_PERCENT: AtomicI32 = AtomicI32::new(SCALE_DEFAULT);
/// `Layout` index (see `Layout::from_index`).
static LIVE_LAYOUT: AtomicI32 = AtomicI32::new(Layout::DEFAULT as i32);
/// `Content` index (see `Content::from_index`).
static LIVE_CONTENT: AtomicI32 = AtomicI32::new(Content::DEFAULT as i32);
/// Per-LAYOUT offsets, indexed by `Layout::index()`.
static LIVE_OFFSET_X: [AtomicI32; Layout::ALL.len()] =
    [AtomicI32::new(0), AtomicI32::new(0), AtomicI32::new(0)];
static LIVE_OFFSET_Y: [AtomicI32; Layout::ALL.len()] =
    [AtomicI32::new(0), AtomicI32::new(0), AtomicI32::new(0)];
/// The operator's `BlockAlignment` index (see `BlockAlignment::from_index`).
/// Only the SIDE COLUMN's alignment is configurable — the BOTTOM / TOP
/// LINE's is fixed by its `AlignmentRule` — so one slot suffices; every read
/// goes through `readout::resolve_alignment`, which ignores it for fixed
/// layouts.
static LIVE_ALIGNMENT: AtomicI32 = AtomicI32::new(BlockAlignment::Center as i32);

fn live_layout() -> Layout {
    Layout::from_index(LIVE_LAYOUT.load(Ordering::Relaxed))
}

fn live_content() -> Content {
    Content::from_index(LIVE_CONTENT.load(Ordering::Relaxed))
}

fn live_offset_x(layout: Layout) -> i32 {
    LIVE_OFFSET_X[layout.index()].load(Ordering::Relaxed)
}

fn live_offset_y(layout: Layout) -> i32 {
    LIVE_OFFSET_Y[layout.index()].load(Ordering::Relaxed)
}

/// The operator's configured alignment (the SIDE COLUMN's).
fn configured_alignment() -> BlockAlignment {
    BlockAlignment::from_index(LIVE_ALIGNMENT.load(Ordering::Relaxed))
}

/// The alignment `layout` actually renders with.
fn live_alignment(layout: Layout) -> BlockAlignment {
    readout::resolve_alignment(layout, configured_alignment())
}

fn text_alignment(align: LineAlign) -> TextAlignment {
    match align {
        LineAlign::Left => TextAlignment::Left,
        LineAlign::Center => TextAlignment::Center,
        LineAlign::Right => TextAlignment::Right,
    }
}

struct TimingStatsState {
    p1: Option<TextWidget>,
    p2: Option<TextWidget>,
    /// The widget PHASE: true from GAMEPLAY entry through the results
    /// carry-over, false elsewhere. Per-side show/hide rides on top of it.
    visible: bool,
    /// Whether the widgets were raised to the top of the widget render
    /// list this song (once, at the first show — see `raise_above_hud`).
    raised: bool,
    raise_pump: PendingPump,
}

static STATE: OnceLock<Arc<Mutex<TimingStatsState>>> = OnceLock::new();

fn state() -> &'static Arc<Mutex<TimingStatsState>> {
    STATE.get_or_init(|| {
        Arc::new(Mutex::new(TimingStatsState {
            p1: None,
            p2: None,
            visible: false,
            raised: false,
            raise_pump: PendingPump::new(),
        }))
    })
}

/// Resolved (x, y, scale, alignment) for a side from the live layout
/// values. Side 0 is P1 (inward = +x), side 1 is P2 (inward = -x).
fn layout_for_side(side: usize) -> (f32, f32, f32, TextAlignment) {
    let layout = live_layout();
    let scale = readout::STOCK_SCALE * LIVE_SCALE_PERCENT.load(Ordering::Relaxed) as f32 / 100.0;
    let (x, y) = readout::anchor(layout, side, live_offset_x(layout), live_offset_y(layout));
    let align = text_alignment(live_alignment(layout).line_align(side));
    (x, y, scale, align)
}

fn apply_layout(w: &TextWidget, side: usize) {
    let (x, y, scale, align) = layout_for_side(side);
    w.set_position(x, y);
    w.set_scale(scale, scale);
    w.set_alignment(align);
}

/// S-Marv tally for the composer: `Some` only while the S-Marvelous mod is
/// enabled and the side is classified (the field is omitted otherwise and
/// Marv counts every grade-0 step — also for a side the S-Marvelous mod
/// excludes, the Multiplayer Bot's Target Score replay).
fn smarv_for(side: usize) -> Option<u32> {
    if crate::mods::s_marvelous::is_enabled() && !crate::mods::s_marvelous::state::is_excluded(side)
    {
        Some(crate::mods::s_marvelous::state::smarv_count(side))
    } else {
        None
    }
}

/// Copy one side's live statistics out of the feed. `None` when the
/// buffer lock is contended (judge-hook hot path — never block).
fn snapshot(side: usize) -> Option<Snapshot> {
    let bufs = data_feed::buffers();
    let b = bufs.get(side)?.try_lock().ok()?;
    Some(Snapshot {
        current_ms: b.current,
        max_abs_ms: b.max_abs,
        max_signed_ms: b.max_signed,
        sum_abs: b.sum_abs,
        sum: b.sum,
        count: b.count,
        ex_loss: b.ex_loss,
        grade_counts: b.grade_counts,
        smarv: smarv_for(side),
        // Live calories burned this song. The game's accumulator
        // (`actor+0x94`, via the calorie tick hook) counts small-calories
        // (cal); the game's own display shows kcal, so divide by 1000.
        kcal: calorie_feed::latest(side) as f32 / 1000.0,
    })
}

/// The all-zero placeholder a freshly created widget shows (matches the
/// live content set, incl. the S-Marv field when the tier is on).
fn placeholder_text(side: usize) -> String {
    let snap = Snapshot {
        smarv: smarv_for(side).map(|_| 0),
        ..Snapshot::default()
    };
    readout::compose(live_content(), live_layout(), &snap)
}

/// Re-apply the live layout to any already-created widgets and recompose
/// their text for the live content set (render thread). The text refresh is
/// best-effort — a contended buffer leaves the previous text until the next
/// judgement rewrites it.
fn relayout_widgets() {
    let st = state().clone();
    widget_renderer::run_on_render_thread(move || {
        let Ok(s) = st.lock() else { return };
        let content = live_content();
        let layout = live_layout();
        for (side, widget) in [(0usize, s.p1.as_ref()), (1usize, s.p2.as_ref())] {
            let Some(w) = widget else { continue };
            apply_layout(w, side);
            if let Some(snap) = snapshot(side) {
                w.set_text(&readout::compose(content, layout, &snap));
            }
        }
    });
}

fn create_player_widget(side: usize) -> Option<TextWidget> {
    let w = widget_renderer::create_text_widget()?;
    apply_layout(&w, side);
    w.set_color(1.0, 1.0, 1.0, 1.0);
    w.set_system_outline();
    w.set_text(&placeholder_text(side));
    w.hide();
    Some(w)
}

/// Read + clamp one integer layout value from the config section, logging
/// when the operator's value was out of range.
fn configured_i32(
    name: &str,
    pick: impl Fn(&config::PowerUserStatisticsConfig) -> Option<i32>,
    default: i32,
    min: i32,
    max: i32,
) -> i32 {
    let raw = config::get()
        .and_then(|c| c.power_user_statistics.as_ref())
        .and_then(pick)
        .unwrap_or(default);
    let clamped = raw.clamp(min, max);
    if clamped != raw {
        log_info!(
            "timing_stats_widget: {} {} out of range -- clamped to {}",
            name,
            raw,
            clamped
        );
    }
    clamped
}

/// Read one keyed (string enum) value from the config section; unknown
/// keys warn once and fall back to `default`.
fn configured_key<T: Copy>(
    name: &str,
    pick: impl Fn(&config::PowerUserStatisticsConfig) -> Option<&String>,
    parse: impl Fn(&str) -> Option<T>,
    default: T,
    default_key: &str,
) -> T {
    let raw = config::get()
        .and_then(|c| c.power_user_statistics.as_ref())
        .and_then(|s| pick(s).cloned());
    match raw {
        None => default,
        Some(k) => parse(&k).unwrap_or_else(|| {
            log_warn!(
                "timing_stats_widget: unknown {} {:?} -- using {}",
                name,
                k,
                default_key
            );
            default
        }),
    }
}

/// Write the whole `power_user_statistics` section from the live values —
/// `save_json_key` REPLACES the section, so every row edit must emit every
/// key or the others silently reset on the next boot.
fn persist_section() {
    config::save_json_key(
        "power_user_statistics",
        serde_json::json!({
            "widget_scale_percent": LIVE_SCALE_PERCENT.load(Ordering::Relaxed),
            "widget_layout": live_layout().key(),
            "widget_content": live_content().key(),
            "widget_offset_x": live_offset_x(Layout::Vertical),
            "widget_offset_y": live_offset_y(Layout::Vertical),
            "widget_alignment": configured_alignment().key(),
            "horizontal_offset_x": live_offset_x(Layout::Horizontal),
            "horizontal_offset_y": live_offset_y(Layout::Horizontal),
            "top_line_offset_x": live_offset_x(Layout::TopLine),
            "top_line_offset_y": live_offset_y(Layout::TopLine),
        }),
    );
}

/// Shared row-edit tail: clamp, store, persist, re-lay out live widgets.
fn on_layout_row_change(slot: &'static AtomicI32, min: i32, max: i32, what: &str, v: i32) {
    let clamped = v.clamp(min, max);
    slot.store(clamped, Ordering::Relaxed);
    persist_section();
    relayout_widgets();
    log_info!("timing_stats_widget: {} set to {}", what, clamped);
}

/// Ask `bottom_text` to hide the stock bottom readouts iff the live layout
/// sits in their band (the BOTTOM LINE) and its widgets are in their
/// on-screen phase. Idempotent; silent when the service is unavailable (the
/// mod then simply draws over the stock text — one WARN at enable covers
/// it).
fn sync_bottom_text_hide(widgets_shown: bool) {
    if !bottom_text::is_available() {
        return;
    }
    let want = widgets_shown && live_layout().hides_bottom_text();
    if bottom_text::is_hidden_by(HideReason::PowerUserStatistics) != want {
        bottom_text::set_hidden(HideReason::PowerUserStatistics, want);
    }
}

/// Register (or re-register in place) the rows that edit the ACTIVE
/// layout's offsets — and, when the layout's alignment is configurable, its
/// alignment. Called at enable and from the layout row's `on_change`: the
/// row store replaces by key, so the rows keep their menu position while
/// their labels, ranges and displayed values switch to the new layout's.
/// The callbacks read the live layout at edit time, so a stale callback can
/// never write the wrong layout's slot.
fn register_layout_scoped_rows() {
    use crate::mods::mod_menu::{self, EnumRowSpec, ScalarRowSpec};
    let layout = live_layout();
    let g = layout.geometry();
    let prefix = layout.row_prefix();
    mod_menu::register_scalar_row(ScalarRowSpec {
        key: OFFSET_X_ROW_KEY.to_string(),
        label: format!("{} H-Offset (px)", prefix),
        hint: format!(
            "{} horizontal shift, mirrored for P1/P2: + moves both inward, - toward the edges. Each layout keeps its own.",
            prefix
        ),
        parent_row_key: Some(OWNING_MOD_ID.to_string()),
        min: g.offset_x_min,
        max: g.offset_x_max,
        step_fine: OFFSET_STEP_FINE,
        step_coarse: OFFSET_STEP_COARSE,
        initial: live_offset_x(layout),
        on_change: Arc::new(|v| {
            let l = live_layout();
            let g = l.geometry();
            on_layout_row_change(
                &LIVE_OFFSET_X[l.index()],
                g.offset_x_min,
                g.offset_x_max,
                "h-offset px",
                v,
            )
        }),
    });
    mod_menu::register_scalar_row(ScalarRowSpec {
        key: OFFSET_Y_ROW_KEY.to_string(),
        label: format!("{} V-Offset (px)", prefix),
        hint: format!(
            "{} vertical shift for both sides: + moves down, - moves up. Each layout keeps its own.",
            prefix
        ),
        parent_row_key: Some(OWNING_MOD_ID.to_string()),
        min: g.offset_y_min,
        max: g.offset_y_max,
        step_fine: OFFSET_STEP_FINE,
        step_coarse: OFFSET_STEP_COARSE,
        initial: live_offset_y(layout),
        on_change: Arc::new(|v| {
            let l = live_layout();
            let g = l.geometry();
            on_layout_row_change(
                &LIVE_OFFSET_Y[l.index()],
                g.offset_y_min,
                g.offset_y_max,
                "v-offset px",
                v,
            )
        }),
    });
    match g.alignment {
        // A multi-line block gets the alignment row (re-registered in place
        // when coming back from a fixed layout — it is the group's LAST row,
        // so appending it to the store puts it exactly where it was).
        AlignmentRule::Configurable(_) => {
            mod_menu::register_enum_row(EnumRowSpec {
                key: ALIGN_ROW_KEY.to_string(),
                label: format!("{} Alignment", prefix),
                hint: format!(
                    "{} line alignment, mirrored: CENTERED, OUTER EDGE (flush to the screen edges) or INNER EDGE (toward centre).",
                    prefix
                ),
                parent_row_key: Some(OWNING_MOD_ID.to_string()),
                values: vec![0, 1, 2],
                labels: vec![
                    "CENTERED".to_string(),
                    "OUTER EDGE".to_string(),
                    "INNER EDGE".to_string(),
                ],
                initial_value: configured_alignment().index(),
                on_change: Arc::new(|v| {
                    let a = BlockAlignment::from_index(v);
                    LIVE_ALIGNMENT.store(a.index(), Ordering::Relaxed);
                    persist_section();
                    relayout_widgets();
                    log_info!("timing_stats_widget: alignment set to {}", a.key());
                }),
            });
        }
        // A single line has nothing to align about — no row while this
        // layout is active.
        AlignmentRule::Fixed(_) => mod_menu::remove_rows_for(&[ALIGN_ROW_KEY]),
    }
}

fn on_layout_change(v: i32) {
    let layout = Layout::from_index(v);
    LIVE_LAYOUT.store(layout as i32, Ordering::Relaxed);
    // Re-seed the layout-scoped rows with this layout's values BEFORE the
    // menu's own post-callback rebuild renders them.
    register_layout_scoped_rows();
    persist_section();
    relayout_widgets();
    // A live switch mid-song must (un)hide the stock bottom text on the spot.
    let shown = state().lock().map(|s| s.visible).unwrap_or(false);
    sync_bottom_text_hide(shown);
    log_info!("timing_stats_widget: layout set to {}", layout.key());
}

fn on_content_change(v: i32) {
    let content = Content::from_index(v);
    LIVE_CONTENT.store(content as i32, Ordering::Relaxed);
    persist_section();
    relayout_widgets();
    log_info!("timing_stats_widget: content set to {}", content.key());
}

fn register_overlay_rows() {
    use crate::mods::mod_menu::{self, EnumRowSpec, ScalarRowSpec};
    mod_menu::register_scalar_row(ScalarRowSpec {
        key: SCALE_ROW_KEY.to_string(),
        label: "Stats Widget Scale (%)".to_string(),
        hint: "Size of the realtime gameplay statistics block, percent of stock (100).".to_string(),
        parent_row_key: Some(OWNING_MOD_ID.to_string()),
        min: SCALE_MIN,
        max: SCALE_MAX,
        step_fine: 5,
        step_coarse: 25,
        initial: LIVE_SCALE_PERCENT.load(Ordering::Relaxed),
        on_change: Arc::new(|v| {
            on_layout_row_change(&LIVE_SCALE_PERCENT, SCALE_MIN, SCALE_MAX, "scale %", v)
        }),
    });
    mod_menu::register_enum_row(EnumRowSpec {
        key: LAYOUT_ROW_KEY.to_string(),
        label: "Stats Widget Layout".to_string(),
        hint: "SIDE COLUMN: one field per line beside each playfield. BOTTOM LINE: every field on one line along the bottom edge, P1 from the left / P2 to the right (hides the stock CREDIT/PASELI text while shown). TOP LINE: the same line along the top edge."
            .to_string(),
        parent_row_key: Some(OWNING_MOD_ID.to_string()),
        values: Layout::ALL.iter().map(|l| l.index() as i32).collect(),
        labels: Layout::ALL.iter().map(|l| l.label().to_string()).collect(),
        initial_value: live_layout().index() as i32,
        on_change: Arc::new(on_layout_change),
    });
    mod_menu::register_enum_row(EnumRowSpec {
        key: CONTENT_ROW_KEY.to_string(),
        label: "Stats Widget Content".to_string(),
        hint: "DETAILED: EX, current/max/mean ms error, calories. STREAMLINED: delta, max delta, EX, then judgement counts."
            .to_string(),
        parent_row_key: Some(OWNING_MOD_ID.to_string()),
        values: vec![Content::Detailed as i32, Content::Streamlined as i32],
        labels: vec![
            Content::Detailed.label().to_string(),
            Content::Streamlined.label().to_string(),
        ],
        initial_value: live_content() as i32,
        on_change: Arc::new(on_content_change),
    });
    register_layout_scoped_rows();
}

/// Seed one layout's offset pair from its config keys.
fn seed_layout_offsets(
    layout: Layout,
    x_name: &str,
    pick_x: impl Fn(&config::PowerUserStatisticsConfig) -> Option<i32>,
    y_name: &str,
    pick_y: impl Fn(&config::PowerUserStatisticsConfig) -> Option<i32>,
) {
    let g = layout.geometry();
    let i = layout.index();
    LIVE_OFFSET_X[i].store(
        configured_i32(x_name, pick_x, 0, g.offset_x_min, g.offset_x_max),
        Ordering::Relaxed,
    );
    LIVE_OFFSET_Y[i].store(
        configured_i32(y_name, pick_y, 0, g.offset_y_min, g.offset_y_max),
        Ordering::Relaxed,
    );
}

/// Seed the operator's alignment (`widget_alignment`) — the SIDE COLUMN's,
/// the only layout whose alignment is configurable.
fn seed_alignment() {
    let default = match Layout::Vertical.geometry().alignment {
        AlignmentRule::Configurable(a) | AlignmentRule::Fixed(a) => a,
    };
    LIVE_ALIGNMENT.store(
        configured_key(
            "widget_alignment",
            |c| c.widget_alignment.as_ref(),
            BlockAlignment::from_key,
            default,
            default.key(),
        )
        .index(),
        Ordering::Relaxed,
    );
}

pub fn enable() {
    LIVE_SCALE_PERCENT.store(
        configured_i32(
            "widget_scale_percent",
            |c| c.widget_scale_percent,
            SCALE_DEFAULT,
            SCALE_MIN,
            SCALE_MAX,
        ),
        Ordering::Relaxed,
    );
    LIVE_LAYOUT.store(
        configured_key(
            "widget_layout",
            |c| c.widget_layout.as_ref(),
            Layout::from_key,
            Layout::DEFAULT,
            Layout::DEFAULT.key(),
        ) as i32,
        Ordering::Relaxed,
    );
    LIVE_CONTENT.store(
        configured_key(
            "widget_content",
            |c| c.widget_content.as_ref(),
            Content::from_key,
            Content::DEFAULT,
            Content::DEFAULT.key(),
        ) as i32,
        Ordering::Relaxed,
    );
    seed_layout_offsets(
        Layout::Vertical,
        "widget_offset_x",
        |c| c.widget_offset_x,
        "widget_offset_y",
        |c| c.widget_offset_y,
    );
    seed_layout_offsets(
        Layout::Horizontal,
        "horizontal_offset_x",
        |c| c.horizontal_offset_x,
        "horizontal_offset_y",
        |c| c.horizontal_offset_y,
    );
    seed_layout_offsets(
        Layout::TopLine,
        "top_line_offset_x",
        |c| c.top_line_offset_x,
        "top_line_offset_y",
        |c| c.top_line_offset_y,
    );
    seed_alignment();
    register_overlay_rows();
    if !bottom_text::is_available() {
        log_warn!(
            "timing_stats_widget: bottom_text service unavailable -- the BOTTOM LINE layout will draw over the stock CREDIT/PASELI text"
        );
    }
    let layout = live_layout();
    log_info!(
        "timing_stats_widget: enabled (scale {}%, layout {}, content {}, {} offset x {} y {} align {}; widgets created on first gameplay entry)",
        LIVE_SCALE_PERCENT.load(Ordering::Relaxed),
        layout.key(),
        live_content().key(),
        layout.key(),
        live_offset_x(layout),
        live_offset_y(layout),
        live_alignment(layout).key()
    );
}

fn ensure_widgets_created(s: &mut TimingStatsState) {
    if !widget_renderer::is_available() {
        return;
    }
    if s.p1.is_none() {
        s.p1 = create_player_widget(0);
    }
    if s.p2.is_none() {
        s.p2 = create_player_widget(1);
    }
}

pub fn disable() {
    crate::mods::mod_menu::remove_rows_for(&ALL_ROW_KEYS);
    let st = state().clone();
    if let Ok(mut s) = st.lock() {
        s.raise_pump.cancel();
        s.visible = false;
    }
    // Release our bottom-text contributor bit unconditionally — the
    // operator's own toggle is a separate bit.
    sync_bottom_text_hide(false);
    widget_renderer::run_on_render_thread(move || {
        let mut s = st.lock().unwrap();
        if let Some(mut w) = s.p1.take() {
            w.destroy();
        }
        if let Some(mut w) = s.p2.take() {
            w.destroy();
        }
        s.visible = false;
    });

    log_info!("timing_stats_widget: disabled");
}

/// Whether a GAMEPLAY → `next` transition keeps the stat blocks on screen.
/// The song's readout stays up through the post-song loader (0-idx 29) and
/// the stage results screen (0-idx 30) so the player can read the detailed
/// breakdown without gameplay pressure (tester feedback 2026-09). Any other
/// destination — song select (quick fail's skip-results redirect 29 → 24),
/// a quick restart (29 → 28, which re-arms via the GAMEPLAY branch), the
/// results → next-stage hop, final results — hides them.
fn carries_over_from_gameplay(prev: i32, next: i32) -> bool {
    let from_play = prev == scene::GAMEPLAY || prev == scene::STAGE_RESULT;
    let to_results = next == scene::STAGE_RESULT || next == scene::RESULTS_DETAIL;
    from_play && to_results
}

/// Called from scene_manager callback to show/hide based on gameplay state.
pub fn on_scene_change(prev: i32, next: i32) {
    let entering_gameplay = next == scene::GAMEPLAY;
    let keep_shown = carries_over_from_gameplay(prev, next);
    let st = state().clone();
    if let Ok(mut s) = st.lock() {
        s.raise_pump.cancel();
    }
    widget_renderer::run_on_render_thread(move || {
        let mut s = st.lock().unwrap();

        if entering_gameplay {
            ensure_widgets_created(&mut s);
            // Don't show yet — widgets become visible on first update_text
            // call, which only fires for players that are actually playing.
            s.visible = true;
            s.raised = false;
            if let Some(ref w) = s.p1 {
                w.hide();
            }
            if let Some(ref w) = s.p2 {
                w.hide();
            }
        } else if keep_shown {
            // Results carry-over: leave the blocks exactly as the song left
            // them (shown for sides that judged at least one step, hidden
            // otherwise). `visible` stays true so a stray late judgement
            // can't re-show a block we already hid elsewhere.
        } else if s.visible {
            s.visible = false;
            if let Some(ref w) = s.p1 {
                w.hide();
            }
            if let Some(ref w) = s.p2 {
                w.hide();
            }
        }
        // The BOTTOM LINE layout owns the stock bottom-text band for exactly
        // the widget phase (gameplay + results carry-over); other layouts
        // leave it alone.
        sync_bottom_text_hide(s.visible);
    });
}

/// Once per song, relink both stat widgets to the TAIL of the widget render
/// list so they draw above every other gameplay HUD widget — in particular
/// the training strip, whose widgets allocate lazily at gameplay entry
/// (render-list z = list order; whichever mod created its widgets later
/// won). Deferred to frame start; the first judgement is comfortably after
/// the strip's texture resolve. Skipped while the overlay menu is open: the
/// menu must stay topmost, and it re-raises itself on every open anyway.
fn raise_above_hud() {
    let st = state().clone();
    let generation = {
        // Judge-submit hot path: never block on the state lock (a render-thread
        // closure may hold it); a contended frame simply retries on the next
        // judgement — the raise only needs to land once per song.
        let Ok(mut s) = st.try_lock() else { return };
        if !s.visible || s.raised || crate::mods::mod_menu::is_open() {
            return;
        }
        let Some(generation) = s.raise_pump.request() else {
            return;
        };
        generation
    };
    widget_renderer::run_on_render_thread(move || {
        let Ok(mut s) = st.lock() else { return };
        if !s.raise_pump.begin(generation)
            || !s.visible
            || s.raised
            || scene_manager::current_scene() != scene::GAMEPLAY
            || crate::mods::mod_menu::is_open()
        {
            return;
        }
        let wrappers = [
            s.p1.as_ref().map_or(0, |w| w.render_wrapper()),
            s.p2.as_ref().map_or(0, |w| w.render_wrapper()),
        ];
        widget_renderer::bring_to_front(&wrappers);
        s.raised = true;
    });
}

/// Update widget text with current buffer values. Called from the
/// judge_submit detour (runs on the game's main/render thread).
/// Shows the widget on first call for this song (lazy show — only
/// players that are actually playing get their widget displayed).
pub fn update_text(player_side: usize) {
    let Ok(s) = state().try_lock() else { return };
    if !s.visible {
        return;
    }

    let widget = match player_side {
        0 => s.p1.as_ref(),
        1 => s.p2.as_ref(),
        _ => return,
    };
    let Some(w) = widget else { return };

    let option_on = custom_options::get_value(player_side as u8, "timing_stats").unwrap_or(0) != 0;
    if !option_on {
        return;
    }

    // Auto-calibration: the live ms-error readout leaks the exact signal
    // being calibrated. Suppression is set at GAMEPLAY entry (before the
    // first judgment), so the widget never becomes visible during a
    // calibration song. (Early-return BEFORE the show below.)
    if super::calibration_suppressed() {
        return;
    }

    // Make visible on first judgment for this player this song.
    w.show();

    // Contended buffer (the feed's own try_lock path holds it only briefly):
    // keep the previous text; the next judgement rewrites it.
    if let Some(snap) = snapshot(player_side) {
        w.set_text(&readout::compose(live_content(), live_layout(), &snap));
    }
    drop(s);
    raise_above_hud();
}
