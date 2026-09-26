//! S-Marvelous Judgement (`s-marvelous`, default ON) — a display-only judgement grade above
//! Marvelous: a stock Marvelous whose timing delta is within `window_ms` (default ±12 ms,
//! clamped 1..=16 so it stays a strict subset of stock Marvelous' ±17 ms) is shown as
//! S-Marvelous. The engine's grade space is never touched — to score / EX / gauge / combo /
//! save / ghost an S-Marvelous IS a Marvelous — so nothing here taints through `score_guard`.
//!
//! ## Classification
//!
//! The delta only exists inside `judge_submit`, so classification rides the shared tap in
//! `power_user_statistics::data_feed` (pre-original; the grade dispatch is synchronous, so
//! every display re-drive runs post-original from the same detour). This module owns the
//! policy: [`state`] is the per-side counters plus the "combo has no loose Marvelous" bit;
//! both sides arm at entry to a play scene (GAMEPLAY and the attract demo, whose autoplay
//! runs the same actor chain) with the live window latched per song, disarm on leaving it,
//! and reset on every `song_reset` (quick restart, training loops/scrubs).
//! `state::combo_is_all_smarv` requires the side to be armed — a side the mod never
//! classified must not read as all-S-Marvelous.
//!
//! ## Display surfaces (each optional, fail-open to the stock look)
//!
//! - [`flash`] — re-drives the NoteResultActor's own `dance_judge` clip to the synthesized
//!   `in_smarvelous` label, and fans out to the other judge-event surfaces. [`afp_patches`]
//!   registers the `dance_judge` AP2 patch; [`assets`] stages its word art (both glow mutes,
//!   S-Marv copy and stock Marvelous word) and every other asset below.
//! - [`receptor`] + pure [`receptor_color`] — violet receptor burst: the game's own
//!   `JudgeEffectRenderer::push` with type 7, recoloured through `playfield_styling`'s
//!   refcounted `render_sprite_final` fill hook (`fill_acquire_smarvelous`). The
//!   `dance_effect` bomb stays stock.
//! - [`combo`] — S-Marvelous digits + tint, a POST subscriber on `services::combo_hooks`.
//! - [`splash`] — S-MFC full-combo splash (`fullcombo_actor_on_message` detour).
//! - [`fast_slow`] — one-byte gate patch so a loose Marvelous shows FAST/SLOW; the flash
//!   re-hides it on S-Marvelous (the highest tier is exempt).
//! - Results: [`results_score`] (7-row score tab, exclusive MARVELOUS, Marvelous FAST/SLOW
//!   share), [`results_graph`] (violet judge series, gradient transplant, timing-page
//!   Marvelous bands, legend), [`results_emblem`] (S-MFC stage emblem + total badge). All
//!   recompute from the stage record's per-note streams via [`records`] with the side's
//!   last-armed window, fail-closed to stock counts.
//! - Lamps: [`lamp`] (per-side S-MFC set) + pure [`lamp_codec`] + [`lamp_badge`] (violet
//!   lamp re-bind on the wheel card, side-info table and difficulty picker).
//!
//! ## Server upload
//!
//! [`upload_hook`] registers a `/data` node producer with
//! `custom_options_persistence::register_data_node_producer`: on per-stage saves of a side
//! armed from song start (not course mode) it emits `/data/s_marv` with the S-Marvelous-aware
//! duplicates computed by pure [`upload`]; stock bytes are never touched. The backend's
//! `smarv_scores` load field, plus our own S-MFC emissions, feed [`lamp`].
//!
//! ## Cross-mod seams
//!
//! DDR SELECTION legacy skins ([`legacy`], pure names in [`targets`]): with both mods enabled,
//! every skin whose art exists under `data_mods/ddr_selection/s_marvelous/N/` is staged like
//! World — its `dance_judge000N` word and `dance_fullcombo000N` S-MFC splash, and on skins
//! 4–5 an all-S-Marvelous combo sheet. The patch fns pick the target by the song
//! (`ddr_selection::legacy_package` + `armed_skin`); the flash and splash re-drives fire only
//! when THIS song's template was patched; DDR SELECTION's A3 combo write asks
//! [`legacy_combo_smarv`] (a legacy combo never reaches World's refresh). A skin without art
//! keeps A3's presentation. DDR SELECTION's `enable` calls [`on_ddr_selection_enabled`].
//! `is_enabled()` lets Power User Statistics show its S-Marv tally.
//!
//! ## Degradation, assets, config
//!
//! Only `judge_submit` is required; without the tap the mod is inert. Every surface's
//! signatures are optional and a missing one leaves only that surface stock.
//! Art sources live in `data_mods/s_marvelous/`; the staged `*_ifs/` output (atlas clones,
//! results sheets) is generated there at enable — never commit it. The results sheets are
//! stock-name replacements LayeredFS serves passively, so they are purged at init (a
//! config-disabled boot) and at disable.
//!
//! Config section `s_marvelous` (`window_ms`, `judgement_color`, `receptor_flash`) is
//! seeded at enable and live-edited by three overlay GLOBAL SETTINGS rows (window applies
//! next song, colour when `dance_judge` next loads, receptor flash on the next hit);
//! `persist_section` rewrites the whole section. The retired `marvelous_shimmer` key is ignored.
//!
//! RE: `docs/s_marvelous_judgement_research.md`. Host tests (the actual AP2 recipes on
//! real templates, plus the pure modules): `scripts/validate_s_marvelous.sh`.

pub mod afp_patches;
pub mod assets;
pub mod combo;
pub mod fast_slow;
pub mod flash;
pub mod lamp;
pub mod lamp_badge;
pub mod lamp_codec;
pub mod legacy;
pub mod receptor;
pub mod receptor_color;
pub mod records;
pub mod results_emblem;
pub mod results_graph;
pub mod results_score;
pub mod splash;
pub mod state;
pub mod targets;
pub mod upload;
pub mod upload_hook;

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

/// Whether the S-Marvelous mod is currently enabled (set for the whole
/// enable..disable span). Sibling mods key display decisions on it — the
/// Power User Statistics streamlined readout shows its S-Marv tally (and
/// an exclusive Marv count) only while this is true. The `state::*`
/// counters are the per-song data; this is the "is the tier a thing on
/// this cabinet right now" flag.
pub(crate) fn is_enabled() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

/// DDR SELECTION just enabled (its `enable`, after S-Marvelous' at boot):
/// stage the legacy skins' S-Marvelous art if this mod is enabled too.
pub fn on_ddr_selection_enabled() {
    legacy::stage_if_ready(live_color());
}

/// DDR SELECTION's A3 combo write, per combo step on a legacy combo actor
/// (game thread): whether side `side`'s combo should use skin `skin`'s
/// S-Marvelous sheet — the mod enabled, that sheet staged, and the side's
/// combo all S-Marvelous so far (`state::combo_is_all_smarv`, which also
/// requires the side armed). The caller still requires worst grade
/// Marvelous. Atomics only.
pub fn legacy_combo_smarv(skin: u8, side: i32) -> bool {
    is_enabled()
        && combo::legacy_sheet_staged(skin)
        && (0..=1).contains(&side)
        && state::combo_is_all_smarv(side as usize)
}

/// The LIVE S-Marvelous window (ms). Seeded from `s_marvelous.window_ms` at
/// enable; the overlay menu's scalar row writes it. Read at each
/// GAMEPLAY-entry arm, so an edit applies NEXT song (per-song latch, design
/// D26) — a mid-song edit never changes the armed window.
static LIVE_WINDOW_MS: AtomicI32 = AtomicI32::new(state::DEFAULT_WINDOW_MS);

/// Overlay menu row keys (GLOBAL SETTINGS, grouped under this mod's header;
/// registration order = display order: window, colour, then receptor flash).
const WINDOW_ROW_KEY: &str = "smarv_window_ms";
const COLOR_ROW_KEY: &str = "smarv_judgement_color";
const RECEPTOR_ROW_KEY: &str = "smarv_receptor_flash";

/// The LIVE "Judgement Color" choice as a `JudgementColor::index()`. Seeded
/// from `s_marvelous.judgement_color` at enable; the overlay row writes it
/// and re-stages the word art on the spot (the game picks the new bytes up
/// when it next loads the dance_judge package — normally next song).
static LIVE_COLOR_IDX: AtomicI32 = AtomicI32::new(0);

/// The LIVE "Receptor Flash Color" choice as a `ReceptorFlash::index()`.
/// Seeded from `s_marvelous.receptor_flash` at enable; the overlay row
/// writes it and hands it to the receptor module, which reads it per
/// S-Marv event — applies to the very next hit.
static LIVE_RECEPTOR_IDX: AtomicI32 = AtomicI32::new(0);

fn live_color() -> assets::JudgementColor {
    assets::JudgementColor::from_index(LIVE_COLOR_IDX.load(Ordering::Relaxed))
        .unwrap_or(assets::JudgementColor::DEFAULT)
}

fn live_receptor_flash() -> receptor_color::ReceptorFlash {
    receptor_color::ReceptorFlash::from_index(LIVE_RECEPTOR_IDX.load(Ordering::Relaxed))
        .unwrap_or(receptor_color::ReceptorFlash::DEFAULT)
}

/// Write the whole `s_marvelous` section from the live values —
/// `save_json_key` REPLACES the section, so every row edit must emit every
/// DLL-written key or the others silently reset on the next boot.
fn persist_section() {
    config::save_json_key(
        "s_marvelous",
        serde_json::json!({
            "window_ms": LIVE_WINDOW_MS.load(Ordering::Relaxed),
            "judgement_color": live_color().key(),
            "receptor_flash": live_receptor_flash().key(),
        }),
    );
}

pub struct SMarvelousMod {
    data_feed_installed: bool,
    combo_installed: bool,
    splash_installed: bool,
    results_installed: bool,
    graph_installed: bool,
    emblem_installed: bool,
    fast_slow_installed: bool,
    lamp_badge_installed: bool,
    receptor_available: bool,
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
            lamp_badge_installed: false,
            receptor_available: false,
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

/// Read the operator/persisted colour from `s_marvelous.judgement_color`
/// (unknown key ⇒ one INFO + default).
fn configured_color() -> assets::JudgementColor {
    use assets::JudgementColor;
    let raw = config::get()
        .and_then(|c| c.s_marvelous.as_ref())
        .and_then(|s| s.judgement_color.clone());
    match raw {
        None => JudgementColor::DEFAULT,
        Some(k) => JudgementColor::from_key(&k).unwrap_or_else(|| {
            log_info!(
                "SMarvelous: judgement_color '{}' unknown -- using {}",
                k,
                JudgementColor::DEFAULT.key()
            );
            JudgementColor::DEFAULT
        }),
    }
}

/// Read the operator/persisted receptor flash choice from
/// `s_marvelous.receptor_flash` (unknown key ⇒ one INFO + default).
fn configured_receptor_flash() -> receptor_color::ReceptorFlash {
    use receptor_color::ReceptorFlash;
    let raw = config::get()
        .and_then(|c| c.s_marvelous.as_ref())
        .and_then(|s| s.receptor_flash.clone());
    match raw {
        None => ReceptorFlash::DEFAULT,
        Some(k) => ReceptorFlash::from_key(&k).unwrap_or_else(|| {
            log_info!(
                "SMarvelous: receptor_flash '{}' unknown -- using {}",
                k,
                ReceptorFlash::DEFAULT.key()
            );
            ReceptorFlash::DEFAULT
        }),
    }
}

/// One INFO for installs whose config still carries the RETIRED
/// `s_marvelous.marvelous_shimmer` key (the stock Marvelous word's pulse is
/// now always muted; the ON/OFF row is gone). Never reinterpreted; the
/// next `persist_section` drops it from the file.
fn note_retired_keys() {
    let present = config::get()
        .and_then(|c| c.s_marvelous.as_ref())
        .and_then(|s| s.marvelous_shimmer);
    if let Some(v) = present {
        log_info!(
            "SMarvelous: config key s_marvelous.marvelous_shimmer ({}) is retired and ignored -- the stock Marvelous shimmer is always muted now",
            v
        );
    }
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
            persist_section();
            log_info!(
                "SMarvelous: window set to {} ms (applies next song)",
                clamped
            );
        }),
    });
}

/// The "Judgement Color" enum row (ALL PURPLE / PURPLE SHADOW), directly
/// under the window row. Edits update the live choice, persist the section
/// and re-stage the word art immediately — the additive `marvelous_ef`
/// glow is muted on BOTH the S-Marv copy and the stock Marvelous word
/// regardless of the choice (`assets::word_clone_opts`), so every word
/// renders static.
fn register_color_row(initial: assets::JudgementColor) {
    use crate::mods::mod_menu::{self, EnumRowSpec};
    use assets::JudgementColor;
    mod_menu::register_enum_row(EnumRowSpec {
        key: COLOR_ROW_KEY.to_string(),
        label: "Judgement Color".to_string(),
        hint: "S-Marvelous flash art: all-violet word, or white letters with a violet shadow. Applies next song."
            .to_string(),
        parent_row_key: Some("s-marvelous".to_string()),
        values: JudgementColor::ALL.iter().map(|c| c.index()).collect(),
        labels: JudgementColor::ALL.iter().map(|c| c.label().to_string()).collect(),
        initial_value: initial.index(),
        on_change: std::sync::Arc::new(|v| {
            let Some(color) = JudgementColor::from_index(v) else {
                return;
            };
            LIVE_COLOR_IDX.store(color.index(), Ordering::Relaxed);
            persist_section();
            if !afp_patches::set_judgement_color(color) {
                log_info!(
                    "SMarvelous: judgement color {} saved (word art not staged this session -- applies next launch)",
                    color.key()
                );
            }
        }),
    });
}

/// The "Receptor Flash Color" enum row (PURPLE / WHITE), third in the
/// section. PURPLE = the violet `JudgeEffectRenderer` burst on every S-Marv
/// hit (the 2026-09-12 look); WHITE = no burst, so the receptor shows
/// exactly the stock Marvelous feedback (the white `dance_effect` bomb —
/// which is stock in both modes). Edits persist the section and hand the
/// choice to the receptor module, which reads it per event — the very next
/// hit follows it.
fn register_receptor_row(initial: receptor_color::ReceptorFlash) {
    use crate::mods::mod_menu::{self, EnumRowSpec};
    use receptor_color::ReceptorFlash;
    mod_menu::register_enum_row(EnumRowSpec {
        key: RECEPTOR_ROW_KEY.to_string(),
        label: "Receptor Flash Color".to_string(),
        hint: "S-Marvelous receptor flash: violet burst, or white (identical to Marvelous). Applies immediately."
            .to_string(),
        parent_row_key: Some("s-marvelous".to_string()),
        values: ReceptorFlash::ALL.iter().map(|m| m.index()).collect(),
        labels: ReceptorFlash::ALL.iter().map(|m| m.label().to_string()).collect(),
        initial_value: initial.index(),
        on_change: std::sync::Arc::new(|v| {
            let Some(mode) = ReceptorFlash::from_index(v) else {
                return;
            };
            LIVE_RECEPTOR_IDX.store(mode.index(), Ordering::Relaxed);
            persist_section();
            receptor::set_flash_mode(mode);
            log_info!("SMarvelous: receptor flash color {}", mode.label());
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
    receptor::reset_for_song();
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
            // Song-select S-MFC lamp (server-upload Step 8): card-refresh
            // detour. Best-effort — without it S-MFC charts keep the stock
            // MFC lamp; the upload/echo-back data path is unaffected.
            self.lamp_badge_installed = lamp_badge::install(ctx.signatures);
            // Violet receptor burst (2026-09-12): the game's own
            // JudgeEffectRenderer pusher + the derived GamePlayActor
            // renderer offset + the shared fill hook. Best-effort — without
            // it S-Marv keeps the stock (white, burst-less) Marvelous
            // receptor.
            self.receptor_available = receptor::init(ctx.signatures);
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
        let color = configured_color();
        LIVE_COLOR_IDX.store(color.index(), Ordering::Relaxed);
        register_color_row(color);
        let receptor_flash = configured_receptor_flash();
        LIVE_RECEPTOR_IDX.store(receptor_flash.index(), Ordering::Relaxed);
        register_receptor_row(receptor_flash);
        receptor::set_flash_mode(receptor_flash);
        note_retired_keys();
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
        afp_patches::activate(color);

        // Violet receptor burst: arm the type-7 push + acquire the shared
        // fill hook for the recolour (both or neither). The dance_effect
        // bomb stays stock white — S-Marv's bomb IS the Marvelous bomb. The
        // "Receptor Flash Color" row gates the push per event on top of
        // this (WHITE ⇒ armed but silent).
        if self.receptor_available && !receptor::activate() {
            log_warn!(
                "SMarvelous: receptor burst not armed -- S-Marv shows the stock Marvelous receptor"
            );
        }

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

        // DDR SELECTION legacy skins: stage their word / splash / combo art
        // too when DDR SELECTION is already enabled (a live enable after it;
        // at boot DDR SELECTION enables later and calls
        // `on_ddr_selection_enabled`).
        legacy::stage_if_ready(color);

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

        // Server-side awareness (server-upload design): the `/data/s_marv`
        // node on per-stage saves (pure recompute from the stage record via
        // the persistence service's subtree-producer registry) + the S-MFC
        // lamp set fed by the backend's `smarv_scores` load field and by our
        // own S-MFC emissions. Both fail-open; neither touches a stock byte.
        upload_hook::activate();
        lamp::activate();
        if self.lamp_badge_installed {
            lamp_badge::activate();
        }

        log_info!(
            "SMarvelous: enabled (window {} ms, judgement color {}, receptor flash {})",
            window,
            color.key(),
            receptor_flash.key()
        );
    }

    fn disable(&mut self) {
        ACTIVE.store(false, Ordering::Release);
        crate::mods::mod_menu::remove_rows_for(&[WINDOW_ROW_KEY, COLOR_ROW_KEY, RECEPTOR_ROW_KEY]);
        afp_patches::deactivate();
        receptor::deactivate();
        combo::set_assets_ready(false);
        splash::deactivate();
        results_score::deactivate();
        results_graph::deactivate();
        results_emblem::deactivate();
        fast_slow::deactivate();
        upload_hook::deactivate();
        lamp_badge::deactivate();
        lamp::deactivate();
        state::disarm_all();
        state::clear_song_armed();
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
