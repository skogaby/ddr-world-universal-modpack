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
pub mod flash;
pub mod state;

use std::sync::atomic::{AtomicBool, Ordering};

use crate::mods::config;
use crate::mods::mod_trait::{Mod, ModContext};
use crate::mods::power_user_statistics::data_feed;
use crate::services::{scene_manager, song_reset};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

/// Gates the scene/song-reset callback bodies: snapshot dispatch means a
/// removed callback can fire one last time after `disable`.
static ACTIVE: AtomicBool = AtomicBool::new(false);

pub struct SMarvelousMod {
    data_feed_installed: bool,
    scene_cb_id: Option<usize>,
    reset_cb_id: Option<usize>,
}

impl SMarvelousMod {
    pub fn new() -> Self {
        Self {
            data_feed_installed: false,
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

impl Mod for SMarvelousMod {
    fn id(&self) -> &str {
        "s-marvelous"
    }
    fn name(&self) -> &str {
        "S-Marvelous Judgement (12ms)"
    }
    fn description(&self) -> &str {
        "Discrete S-Marvelous judgement for Marvelous steps within +/-12ms (display-only)"
    }
    fn required_signatures(&self) -> &[&str] {
        &["judge_submit"]
    }
    fn is_active(&self) -> bool {
        self.data_feed_installed
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
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
        true
    }

    fn enable(&mut self) {
        if !self.data_feed_installed {
            return;
        }
        let window = configured_window();
        ACTIVE.store(true, Ordering::Release);

        if scene_manager::is_available() {
            let id = scene_manager::on_scene_change(Box::new(move |prev, next| {
                if !ACTIVE.load(Ordering::Acquire) {
                    return;
                }
                if prev == scene::GAMEPLAY && next != scene::GAMEPLAY {
                    // Per-song report (Step 1's cabinet demo), then disarm.
                    for side in 0..2usize {
                        let marv = state::marv_total(side);
                        let smarv = state::smarv_count(side);
                        if marv > 0 || smarv > 0 {
                            log_info!(
                                "SMarvelous: song end side={} smarv={} marv_total={} window={}",
                                side,
                                smarv,
                                marv,
                                window
                            );
                        }
                    }
                    state::disarm_all();
                }
                if next == scene::GAMEPLAY {
                    // Per-song latch: mid-song config/toggle changes apply
                    // next song (design D26).
                    state::reset_song_state();
                    state::arm(0, window);
                    state::arm(1, window);
                    flash::reset_latches();
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

        log_info!("SMarvelous: enabled (window {} ms)", window);
    }

    fn disable(&mut self) {
        ACTIVE.store(false, Ordering::Release);
        afp_patches::deactivate();
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
