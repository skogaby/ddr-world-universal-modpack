//! S-Marvelous Judgement — a discrete presentation-layer judgement grade
//! above Marvelous: a stock Marvelous whose timing delta is within the
//! configured window (default ±12 ms, stock Marvelous is ±17 ms) is shown
//! as S-Marvelous. The engine's internal grade space is never touched — to
//! score/EX/gauge/combo/save/ghost an S-Marvelous IS a Marvelous.
//!
//! Classification rides the shared `judge_submit` detour
//! (`power_user_statistics::data_feed`, tap block — the delta only exists
//! there); this module owns the policy: per-song arm/disarm latching, the
//! per-side counters (`state`), and — in later plan steps — the display
//! surfaces.
//!
//! Design: `.agents/planning/2026-08-29-s-marvelous-judgement/design/
//! detailed-design.md` (Approved 2026-08-29).

pub mod afp_patches;
pub mod assets;
pub mod combo;
pub mod fast_slow;
pub mod flash;
pub mod records;
pub mod results_emblem;
pub mod results_graph;
pub mod results_score;
pub mod splash;
pub mod state;

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crate::mods::config;
use crate::mods::mod_trait::{Mod, ModContext};
use crate::mods::power_user_statistics::data_feed;
use crate::services::{scene_manager, song_reset};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

/// Gates the scene/song-reset callback bodies: snapshot dispatch means a
/// removed callback can fire one last time after `disable`.
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// The LIVE S-Marvelous window (ms). Seeded from `s_marvelous.window_ms` at
/// enable; the overlay menu's scalar row writes it. Read at each
/// GAMEPLAY-entry arm, so an edit applies NEXT song (per-song latch, design
/// D26) — a mid-song edit never changes the armed window.
static LIVE_WINDOW_MS: AtomicI32 = AtomicI32::new(state::DEFAULT_WINDOW_MS);

/// Overlay menu row key (GLOBAL SETTINGS, grouped under this mod's header).
const WINDOW_ROW_KEY: &str = "smarv_window_ms";

pub struct SMarvelousMod {
    data_feed_installed: bool,
    combo_installed: bool,
    splash_installed: bool,
    results_installed: bool,
    graph_installed: bool,
    emblem_installed: bool,
    fast_slow_installed: bool,
    scene_cb_id: Option<usize>,
    reset_cb_id: Option<usize>,
}

impl SMarvelousMod {
    pub fn new() -> Self {
        Self {
            data_feed_installed: false,
            combo_installed: false,
            splash_installed: false,
            results_installed: false,
            graph_installed: false,
            emblem_installed: false,
            fast_slow_installed: false,
            scene_cb_id: None,
            reset_cb_id: None,
        }
    }
}

/// Read + clamp the operator window from `s_marvelous.window_ms`.
fn configured_window() -> i32 {
    let raw = config::get()
        .and_then(|c| c.s_marvelous.as_ref())
        .and_then(|s| s.window_ms)
        .unwrap_or(state::DEFAULT_WINDOW_MS);
    let clamped = state::clamp_window(raw);
    if clamped != raw {
        log_info!(
            "SMarvelous: window_ms {} out of range -- clamped to {}",
            raw,
            clamped
        );
    }
    clamped
}

/// Register (or idempotently re-register — `register_scalar_row` replaces by
/// key) the overlay menu's window row, seeded with `initial`. Renders on the
/// GLOBAL SETTINGS tab under this mod's auto-generated section header
/// (S-MARVELOUS JUDGEMENT). Edits write the live atomic (armed next song)
/// and persist the `s_marvelous` config section.
fn register_overlay_row(initial: i32) {
    use crate::mods::mod_menu::{self, ScalarRowSpec};
    mod_menu::register_scalar_row(ScalarRowSpec {
        key: WINDOW_ROW_KEY.to_string(),
        label: "S-Marvelous Window".to_string(),
        hint: "S-Marvelous window (ms, stock Marvelous is 17). Applies next song.".to_string(),
        parent_row_key: Some("s-marvelous".to_string()),
        min: state::MIN_WINDOW_MS,
        max: state::MAX_WINDOW_MS,
        step_fine: 1,
        step_coarse: 4,
        initial,
        on_change: std::sync::Arc::new(|v| {
            let clamped = state::clamp_window(v);
            LIVE_WINDOW_MS.store(clamped, Ordering::Relaxed);
            config::save_json_key("s_marvelous", serde_json::json!({ "window_ms": clamped }));
            log_info!(
                "SMarvelous: window set to {} ms (applies next song)",
                clamped
            );
        }),
    });
}

/// Scenes whose judge dispatches the mod classifies: real gameplay AND the
/// attract demo (its autoplay routes through the same GamePlayActor /
/// judge_submit / NoteResultActor chain — with the mod enabled the demo
/// shows S-Marvelous exactly like a credit does; 2026-09-01 directive). An
/// unarmed play scene is what produced the attract "hodgepodge": stock
/// white word (no re-drive) under a violet combo (see
/// `state::combo_is_all_smarv`).
fn is_play_scene(scene_id: i32) -> bool {
    scene_id == scene::GAMEPLAY || scene_id == scene::ATTRACT_DEMO
}

/// Per-song report (Step 1's cabinet demo) for every side that saw a
/// Marvelous-grade event, then disarm both sides.
fn report_and_disarm() {
    for side in 0..2usize {
        let marv = state::marv_total(side);
        let smarv = state::smarv_count(side);
        if marv > 0 || smarv > 0 {
            log_info!(
                "SMarvelous: song end side={} smarv={} marv_total={} window={}",
                side,
                smarv,
                marv,
                state::last_armed_window(side)
            );
        }
    }
    state::disarm_all();
}

/// Arm both sides for a play scene. Per-song latch: mid-song config/toggle/
/// overlay-row changes apply next song (design D26) — the live window is
/// read here, at play-scene entry, and nowhere else.
fn arm_for_play_scene() {
    let window = LIVE_WINDOW_MS.load(Ordering::Relaxed);
    state::reset_song_state();
    state::arm(0, window);
    state::arm(1, window);
    flash::reset_latches();
    splash::reset_latches();
}

impl Mod for SMarvelousMod {
    fn id(&self) -> &str {
        "s-marvelous"
    }
    fn name(&self) -> &str {
        "S-Marvelous Judgement"
    }
    fn description(&self) -> &str {
        "Discrete S-Marvelous judgement for Marvelous steps inside a tighter window (display-only)"
    }
    fn required_signatures(&self) -> &[&str] {
        &["judge_submit"]
    }
    fn is_active(&self) -> bool {
        self.data_feed_installed
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        // The results sheets are STOCK-NAME texture replacements that
        // LayeredFS serves passively from disk — purge any staged copies
        // from a previous session up front. init runs even when the mod is
        // config-disabled, so a disabled boot always reverts to stock art;
        // enable() restages (Step 7).
        assets::purge_results();

        self.data_feed_installed = data_feed::install(ctx.signatures);
        if !self.data_feed_installed {
            log_warn!("SMarvelous: judge_submit tap unavailable -- mod inert");
        }
        // Shared clip capture (flash re-drive needs the side-bound
        // dance_judge clip even with overlay-element-styling disabled).
        // Best-effort: without it the flash degrades to stock (one WARN at
        // first S-Marv event), classification unaffected.
        if self.data_feed_installed
            && !crate::mods::overlay_element_styling::ensure_capture_installed(ctx.signatures)
        {
            log_warn!("SMarvelous: shared clip capture unavailable -- flash will show stock");
        }
        // NoteResultActor RTTI vtable: lets the flash drive the actor's OWN
        // stored dance_judge wrapper (stock-identical target) instead of the
        // captured pool wrapper. Best-effort — without it the flash falls
        // back to the captured clip.
        flash::set_note_result_vtable(ctx.signatures.get_address("note_result_actor_vtable"));
        // Combo digit override (Step 5): detour installs once; the override
        // itself is gated on ACTIVE state + staged assets, so a disabled
        // mod costs one branch per combo refresh. Best-effort.
        if self.data_feed_installed {
            self.combo_installed = combo::install(ctx.signatures);
            self.splash_installed = splash::install(ctx.signatures);
            // Results score tab (Step 7): populate detour + the game's
            // row-write helper. Best-effort — without it the results tab
            // stays fully stock (no sheets, no patch, no row).
            self.results_installed = results_score::install(ctx.signatures);
            // Judgement graph (Step 8): rebuild/append/legend detours.
            // Best-effort — without them the graph stays stock.
            self.graph_installed = results_graph::install(ctx.signatures);
            // FC emblems (Step 9): results-build + total-results detours.
            // Best-effort per surface — without them the emblems stay
            // stock (violet stage emblem and/or total badge).
            self.emblem_installed = results_emblem::install(ctx.signatures);
            // Marvelous FAST/SLOW gate (2026-09-01): one-byte patch site.
            // Best-effort — without it Marvelous never shows FAST/SLOW
            // (stock).
            self.fast_slow_installed = fast_slow::install(ctx.signatures);
        }
        true
    }

    fn enable(&mut self) {
        if !self.data_feed_installed {
            return;
        }
        let window = configured_window();
        LIVE_WINDOW_MS.store(window, Ordering::Relaxed);
        register_overlay_row(window);
        ACTIVE.store(true, Ordering::Release);

        if scene_manager::is_available() {
            let id = scene_manager::on_scene_change(Box::new(move |prev, next| {
                if !ACTIVE.load(Ordering::Acquire) {
                    return;
                }
                // The graph registry keys on tab POINTERS — allocations
                // recycle across scenes, so every transition drops it
                // (Step 8).
                results_graph::on_scene_change();
                if is_play_scene(prev) && !is_play_scene(next) {
                    report_and_disarm();
                }
                if is_play_scene(next) {
                    arm_for_play_scene();
                }
            }));
            self.scene_cb_id = Some(id);
        } else {
            log_warn!("SMarvelous: scene_manager unavailable -- arming disabled");
        }

        // In-place song resets (quick restart's instant path, training
        // scrubs/loops) never leave scene 28 — clear the counters there too.
        if song_reset::is_available() {
            self.reset_cb_id = Some(song_reset::on_song_reset(|_t_ms| {
                if ACTIVE.load(Ordering::Acquire) {
                    state::reset_song_state();
                }
            }));
        }

        // Gameplay-flash synthesis chain (Step 4): stage the dance_judge
        // assets (atlas clone + rewritten geo + MD5 mapping) and register
        // the AP2 patch. Best-effort — a staging failure WARNs and leaves
        // the patch unstaged (stock template streams); classification and
        // logging above keep working regardless.
        afp_patches::activate();

        // Combo digit textures (Step 5): FRESH atlas entries + per-image
        // PNGs. Best-effort — failure leaves the combo override dormant
        // (stock digits/tint).
        if self.combo_installed {
            combo::set_assets_ready(assets::stage_combo_digits());
        }

        // S-MFC splash (Step 6): stage the four dance_fullcombo template
        // patches + art. Best-effort — failure leaves the stock splash.
        if self.splash_installed {
            splash::activate();
        }

        // Results score tab (Step 7): stage the 7-row label sheets +
        // register the row-repositioning patch. Best-effort — failure
        // leaves the stock tab (sheets are purged on any refusal so art
        // and row positions always move together).
        if self.results_installed {
            results_score::activate();
        }

        // Judgement graph (Step 8): pure detour work, no assets.
        if self.graph_installed {
            results_graph::activate();
        }

        // FC emblems (Step 9): stage the result_root patch + violet word
        // region + total-results badge texture. Best-effort — failure
        // leaves stock emblems.
        if self.emblem_installed {
            results_emblem::activate();
        }

        // Marvelous FAST/SLOW indicator: flip the NoteResultActor gate so
        // Marvelous steps show FAST/SLOW during gameplay (the results-tab
        // FAST/SLOW totals gain the Marvelous share via results_score).
        if self.fast_slow_installed {
            fast_slow::activate();
        }

        log_info!("SMarvelous: enabled (window {} ms)", window);
    }

    fn disable(&mut self) {
        ACTIVE.store(false, Ordering::Release);
        crate::mods::mod_menu::remove_rows_for(&[WINDOW_ROW_KEY]);
        afp_patches::deactivate();
        combo::set_assets_ready(false);
        splash::deactivate();
        results_score::deactivate();
        results_graph::deactivate();
        results_emblem::deactivate();
        fast_slow::deactivate();
        state::disarm_all();
        state::reset_song_state();
        if let Some(id) = self.scene_cb_id.take() {
            scene_manager::remove_callback(id);
        }
        if let Some(id) = self.reset_cb_id.take() {
            song_reset::remove_callback(id);
        }
        log_info!("SMarvelous: disabled");
    }
}
