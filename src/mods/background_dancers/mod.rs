//! Background Dancers — revives the pre-World 3D background: during every
//! gameplay song an A3 stage and A3 dancer(s) (random, or the players'
//! choice) animate behind the lane, rendered by the game's OWN model passes
//! from the character / stage / motion / camera arcs the stock World install
//! still ships but never opens. **DEFAULT OFF** (`DEFAULT_OFF_MODS`).
//!
//! World's engine kept the whole 3D pipeline (KTMDL loader, `MODEL:*` passes,
//! `agcs::scene::SceneGraph`, camera); Konami deleted only the game-side
//! scene layer. `services::scene3d` supplies that layer's engine contact
//! points (render items, the mod-owned scene node, the seqlocked
//! `frame_board`, camera slot 0, `arc_set`, the `viewport_pass` compositor);
//! this mod supplies the game logic. No rendering detours; the only code
//! patch is STAGE SCREENS' layer-select byte. RE:
//! `docs/background_dancers_research.md`, `docs/3d_model_format_research.md`.
//!
//! **Per song:** at the first entry into the song window {26, 27, 28} the
//! pick is resolved (developer pin → option rows → seeded random), its arcs
//! are loaded through the engine FileManager and parsed on one std thread,
//! and every resident model becomes a render item + scene node. The scene is
//! shown from the DancePlaySequence's step-5 edge and posed as a pure
//! function of the music count, turned into DANCE time through the chart's
//! tempo map (A3's `bpm_sync` / `stop_slow` rules). Every frame the director
//! publishes poses onto the frame board and writes the camera from A3's
//! stage-mode sequencing; the 2D background is made transparent. Window exit
//! tears the scene down and restores every override.
//!
//! **Background Movies** (for every entered side whose VIDEO SIZE shows a
//! movie): OFF (VIDEO SIZE OFF + the `MovieSuppressor::BackgroundDancers`
//! contributor of `services::movie_policy` for the window), THUMBNAIL,
//! STAGE SCREENS (default: the movie plays on stages whose materials sample
//! `offscreen1`; screen-less stages fall back to THUMBNAIL), FULLSCREEN (NO
//! STAGE: once the movie really plays, stage + shadows hide and the movie
//! camera set films the dancers) and MOVIE ONLY (NO DANCERS). A RANDOM
//! stage draw follows the song's movie state (screen stages when the movie
//! will play on screens, screen-less stages otherwise); a chosen stage is
//! never filtered. Missing dependencies degrade to THUMBNAIL with one WARN.
//!
//! **Lighting Style** STOCK (UNLIT) / SMOOTH SHADING / CEL SHADING + **Scene
//! Outlines** (INK or LAYERED inverted-hull twins, per-kind widths): applied
//! per song at item build by re-pointing each item's private material copies
//! at the `<material>_lit` / `_cel` containers that
//! `services/avs_layeredfs/shader_synthesis.rs` packs when `shader-fixes` is
//! also on (hull twins run their program 0). Not served ⇒ stock / no
//! outlines, one WARN. The shadow, `_bg` skydome parts, blended materials
//! and screen materials always stay stock.
//!
//! **Choice, previews, custom content:** two in-game `custom_options` rows
//! (`PersistMode::Local`) — BACKGROUND DANCER (per player) and BACKGROUND
//! STAGE (cabinet-wide, `versus_mirror`ed) — with a live 3D preview in the
//! row's box at song select (`viewport_pass` clones; RANDOM ⇒ a static
//! badge; no compositor ⇒ rows only). With `custom_content` on (next
//! launch), each MODEL FOLDER under
//! `data_mods/custom_models/{dancers,stages}/<Friendly Name>/` (`pl_<key>/`,
//! `mapset_<key>/`; flat export or unpacked-arc layout) is packed into a
//! fingerprinted cache arc under `data_mods/_cache/custom_models/` — a ready
//! `.arc` is still accepted — and mounted in `scene3d::arc_set`; entries are
//! labelled by folder name, keys colliding with stock are refused, and the
//! custom block follows the stock block of the catalog.
//!
//! **Threads:** engine calls happen on the game thread only (scene
//! callbacks, the mod's `input_manager::on_frame` callback,
//! `run_on_render_thread`); the parse thread and the enable-time custom scan
//! use `std` only. The scene node's `visit` / dtor run on the engine's
//! job-graph worker: NO engine API, locks, allocation or logging there;
//! `visit(2)` is the only writer of an attached item's pose (copied from the
//! frame board). `scene3d::viewport_pass::reap()` is called exactly once per
//! frame, from this file.
//!
//! **Config** `background_dancers` (DLL-written, rewritten whole by
//! `style.rs` on every GLOBAL SETTINGS edit): `style`, `outlines`,
//! `outline_style`, `outline_px`, `outline_px_stage`, `bpm_sync`,
//! `stop_slow`, `movie_mode`, `custom_content`, plus the operator-set
//! `outline_layer_colors` (re-emitted when present). All apply next song
//! except `custom_content` (next launch). Legacy `shader_fixes.dancer_*` /
//! `lit_models` seed `style` / `outlines` when absent.
//!
//! **Degradation:** `required_signatures` = the all-or-nothing `scene3d`
//! group anchor + FileManager (a miss skips the mod). Without the
//! `startup.arc` rlists or any stage/dancer arc the mod reports inactive.
//! Everything else is per-song fail-open, one WARN/INFO each. `is_active()`
//! = "this mod CAN work", never "something rendered this boot". Developer
//! knobs (`layeredfs.developer_mode`): `DDR_DANCERS_PIN`,
//! `DDR_DANCERS_STATIC`, `DDR_DANCERS_VIEWPORT_SMOKE`.
//!
//! **Host tests:** `scripts/validate_background_dancers.sh` mounts the pure
//! files marked † below plus the pure `scene3d` / `core::anm` layers; its
//! `core::anm` fixture leg needs `$DDR_WORLD_INSTALL`.
//!
//! ## Submodules
//!
//! - Tables + choice: [`lifecycle`] (tables, song window, gameplay wrapper),
//!   [`selection`]†, [`pick`]†, [`catalog`]†, [`options`], [`custom_content`]†
//!   (pure planner), [`custom_scan`] (folder walk, packing, mounts).
//! - Scene: [`session`] (parse + instance build), [`instance_plan`]†,
//!   [`scene_window`] (load / build / teardown, shared with previews),
//!   [`director`] + [`director_math`]†, [`schedule`]†, [`clock`]†,
//!   [`tempo`]† + [`tempo_source`], [`background_hide`].
//! - Movies: [`movie_mode`]†, [`movie_size`], [`movie_backdrop`],
//!   [`movie_camera`]†, [`screen_route`], [`song_movie`].
//! - Look: [`style`] (rows, live values, config), [`outline`]†.
//! - [`preview`] (`layout`†, `state`†, camera, scene, badge);
//!   [`viewport_smoke`] (dev compositor smoke).

pub mod background_hide;
pub mod catalog;
pub mod clock;
pub mod custom_content;
pub mod custom_scan;
pub mod director;
pub mod director_math;
pub mod instance_plan;
pub mod lifecycle;
pub mod movie_backdrop;
pub mod movie_camera;
pub mod movie_mode;
pub mod movie_size;
pub mod options;
pub mod outline;
pub mod pick;
pub mod preview;
pub mod scene_window;
pub mod schedule;
pub mod screen_route;
pub mod selection;
pub mod session;
pub mod song_movie;
pub mod style;
pub mod tempo;
pub mod tempo_source;
pub mod viewport_smoke;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::{input_manager, scene3d, scene_manager};
use crate::{log_info, log_warn};

/// Mod enabled (registry toggle); read by the callbacks.
static ENABLED: AtomicBool = AtomicBool::new(false);

pub struct BackgroundDancersMod {
    scene_cb: Option<usize>,
    frame_cb: Option<usize>,
}

impl BackgroundDancersMod {
    pub fn new() -> Self {
        Self {
            scene_cb: None,
            frame_cb: None,
        }
    }
}

impl Mod for BackgroundDancersMod {
    fn id(&self) -> &str {
        "background-dancers"
    }

    fn name(&self) -> &str {
        // Also the GLOBAL SETTINGS group header of the mod menu (maintainer:
        // no "Enable" prefix there).
        "Background Dancers"
    }

    fn description(&self) -> &str {
        "Random A3 3D stage + dancers behind the lane, rendered by the game's own model passes"
    }

    fn required_signatures(&self) -> &[&str] {
        &[
            "scene3d_scene_graph_manager",
            "file_manager_load",
            "file_manager_free",
            "file_manager_singleton",
        ]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        // Optional: the movie-size override and the fullscreen-movie probe
        // behind Background Movies = FULLSCREEN (fail-open without them).
        let _ = movie_size::init(ctx.signatures);
        let _ = movie_backdrop::init(ctx.signatures);
        // Optional: Background Movies = STAGE SCREENS (the layer-select
        // byte + the MovieActor fit fields; THUMBNAIL without them).
        let _ = screen_route::init(ctx.signatures);
        // Optional: the committed song's movie state, for the RANDOM stage
        // pool's screen rule (Unknown ⇒ stages without screens only).
        let _ = song_movie::init(ctx.signatures);
        scene3d::is_available()
    }

    fn enable(&mut self) {
        if !scene3d::is_available() {
            log_warn!("BackgroundDancers: scene3d service unavailable -- mod inactive");
            return;
        }
        if !lifecycle::tables_ready() && !lifecycle::init_tables() {
            return;
        }
        ENABLED.store(true, Ordering::Release);
        style::init_from_config();
        // The BACKGROUND DANCER / BACKGROUND STAGE rows (options.rs) over the
        // catalog derived from the tables (stock block first, then the custom
        // data_mods entries under their folder names); fail-open (absent rows
        // ⇒ random picks, one WARN inside).
        match lifecycle::tables_snapshot() {
            Some((stages, _camera_rows, dancers)) => {
                let custom = lifecycle::custom_labels_snapshot();
                options::register(catalog::build_catalog_with_custom(
                    &stages, &dancers, &custom,
                ));
            }
            None => log_warn!("BackgroundDancers: tables unreadable -- option rows not registered"),
        }
        // The live 3D previews behind the two rows (fail-open without the
        // compositor).
        preview::init();
        // Dev-mode compositor smoke (`DDR_DANCERS_VIEWPORT_SMOKE`).
        viewport_smoke::init_from_env();
        if self.scene_cb.is_none() {
            self.scene_cb = Some(scene_manager::on_scene_change(Box::new(|prev, next| {
                if !ENABLED.load(Ordering::Acquire) {
                    return;
                }
                lifecycle::on_scene_change(prev, next);
                preview::on_scene_change(prev, next);
                viewport_smoke::on_scene_change(prev, next);
            })));
        }
        if self.frame_cb.is_none() {
            self.frame_cb = Some(input_manager::on_frame(Arc::new(|| {
                if !ENABLED.load(Ordering::Acquire) {
                    return;
                }
                background_hide::on_frame();
                // STAGE SCREENS fit writer — independent of the scene build
                // (the fit must land before the movie starts playing).
                screen_route::on_frame();
                lifecycle::on_frame();
                preview::on_frame();
                viewport_smoke::on_frame();
                // The ONE per-frame reaper call for every viewport-pass
                // owner in this mod (frees detached sets after 2 frames).
                scene3d::viewport_pass::reap();
            })));
        }
        log_info!("BackgroundDancers: enabled -- random A3 stage + dancers every song");
    }

    fn disable(&mut self) {
        ENABLED.store(false, Ordering::Release);
        if let Some(id) = self.frame_cb.take() {
            input_manager::remove_frame_callback(id);
        }
        if let Some(id) = self.scene_cb.take() {
            scene_manager::remove_callback(id);
        }
        // Hide the option rows (values + persistence stay; a re-enable
        // re-arms them) and stop mirroring the stage row.
        options::set_available(false);
        preview::shutdown();
        viewport_smoke::shutdown();
        // Restore the 2D background first (its alpha is a write into a
        // game-owned layer), then neutralise any live scene.
        background_hide::disarm();
        lifecycle::teardown_on_disable();
        log_info!("BackgroundDancers: disabled");
    }

    fn is_active(&self) -> bool {
        scene3d::is_available() && lifecycle::tables_ready()
    }
}
