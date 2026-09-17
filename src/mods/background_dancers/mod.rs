//! Enable Background Dancers — revives the pre-World 3D background dancers:
//! during every gameplay song a randomly chosen A3 stage and randomly chosen
//! A3 dancer(s) animate behind the lane, rendered by the game's OWN model
//! passes from the character / stage / motion / camera arcs the stock World
//! install still ships but never opens.
//!
//! Architecture (design `.agents/planning/2026-09-16-enable-background-dancers/`):
//! World's engine kept the whole 3D pipeline (KTMDL loader, `MODEL:*` passes,
//! `agcs::scene::SceneGraph`, camera); Konami deleted only the game-side scene
//! layer. `services::scene3d` supplies that layer's engine contact points and
//! this mod supplies the game logic (selection, the A3 choreography/camera
//! rules, the per-song lifecycle). Zero rendering detours; nothing is patched.
//!
//! ## Shape (plan Steps 7–10)
//!
//! At the first entry into the song window {26, 27, 28} ([`lifecycle`]) a
//! seeded random pick ([`selection`], [`session::Pick`]) chooses one A3
//! stage row + one dancer per entered side (+ the accessory parts whose
//! arcs exist), overrides every entered side's fullscreen movie size to the
//! sized thumbnail for the song ([`movie_size`]), hands the pick's arcs to
//! the engine's FileManager and parses them on one background thread
//! ([`session::parse_pick`] → `core::anm`: stage `_play_loop`s, dance
//! clips, the body `.b2it`, the part / shadow bone tables, the stage's
//! `.camanm` sets). Every resident model becomes a render item + scene node
//! (stage parts, bodies, parts, one `pl_shadow00` per dancer). The scene is
//! shown from the DancePlaySequence's step-5 edge (`song_reset::dps_step`)
//! and clocked by the content-domain music count ([`clock`]) turned into
//! DANCE TIME through the chart's tempo map ([`tempo`], sourced from the
//! song's SSQ by [`tempo_source`]: half a second of the 120-BPM clips per
//! chart beat, phase-pinned to the measure grid, 1/12 speed through STOPs —
//! the two A3 `ConfigBank` switches, both ON by default via the
//! `background_dancers` config section); every frame
//! the [`director`] evaluates each body once and publishes bodies, parts
//! (`E · bone · body`), shadows (the A3 ground-bone rule) and stage loops
//! onto the `scene3d::frame_board` the nodes copy from, and writes the
//! camera slot from the A3 stage-mode camera sequencing ([`schedule`] over
//! the shuffled main / `_non` camanm lists). [`background_hide`] makes the
//! 2D background transparent for the song. Torn down at window exit
//! (nodes → destroy vector → dtors → arcs; movie size + hide restored).
//! `DEFAULT_OFF_MODS`: the maintainer flips the default once cabinet-proven.
//!
//! ## Degradation
//!
//! `required_signatures` names the `scene3d` group's anchor; the group is
//! all-or-nothing, so a miss on any build skips the mod cleanly. Without the
//! `startup.arc` rlists or without any stage/dancer arc in the install the
//! mod reports inactive. Everything else is per-song fail-open (a part, a
//! shadow, a camera set, the movie-size override — each degrades alone with
//! one WARN/INFO). `is_active()` = "this mod CAN work" (the service resolved
//! and the tables loaded), never "something rendered this boot".

pub mod background_hide;
pub mod clock;
pub mod director;
pub mod director_math;
pub mod lifecycle;
pub mod movie_size;
pub mod schedule;
pub mod selection;
pub mod session;
pub mod tempo;
pub mod tempo_source;

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
        "Enable Background Dancers"
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
        // Optional: the movie-size override (fail-open without it).
        let _ = movie_size::init(ctx.signatures);
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
        if self.scene_cb.is_none() {
            self.scene_cb = Some(scene_manager::on_scene_change(Box::new(|prev, next| {
                if !ENABLED.load(Ordering::Acquire) {
                    return;
                }
                lifecycle::on_scene_change(prev, next);
            })));
        }
        if self.frame_cb.is_none() {
            self.frame_cb = Some(input_manager::on_frame(Arc::new(|| {
                if !ENABLED.load(Ordering::Acquire) {
                    return;
                }
                background_hide::on_frame();
                lifecycle::on_frame();
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
