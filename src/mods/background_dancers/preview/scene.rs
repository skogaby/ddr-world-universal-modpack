//! Preview scene construction (design §4.6 "preview/scene.rs"): the pick
//! for a chosen dancer or stage, and the `Session` a preview window builds
//! from its parse — slot base + private pass-mask bit per side, no outline
//! hulls, real-time dance clock, and a synthetic dance schedule when the
//! scene has no dancers (the camera event loop needs cut times).

use std::sync::Mutex;
use std::time::Instant;

use crate::log_warn;
use crate::services::scene3d::arc_set;
use crate::services::scene3d::viewport_pass_layout::FILTER_BIT;

use super::super::catalog::Kind;
use super::super::outline::HullPlan;
use super::super::pick::{dancer_only, stage_only, Pick};
use super::super::scene_window::SceneWindow;
use super::super::schedule::synthetic_schedule;
use super::super::selection::{DancerCandidate, Rng, StageCandidate};
use super::super::session::{Parsed, Session};
use super::super::style;
use super::super::tempo::TempoOptions;
use super::layout::{MAX_PREVIEW_INSTANCES, SLOT_BASE, STAGE_CUT_PERIOD_S};

/// The candidate tables as `lifecycle::tables_snapshot` hands them out.
pub type Tables = (
    Vec<StageCandidate>,
    Vec<(String, Vec<String>)>,
    Vec<DancerCandidate>,
);

/// One live (or starting) preview: what it shows, the shared asset/scene
/// lifecycle, and when it became visible (the wall-clock time base).
pub struct PreviewWindow {
    pub identity: (Kind, String),
    pub side: usize,
    pub scene: SceneWindow,
    /// First frame with a built instance — `t = built_at.elapsed()`.
    pub built_at: Option<Instant>,
    pub seed: u64,
}

impl PreviewWindow {
    /// Seconds since the scene became visible (0 before).
    pub fn t(&self) -> f32 {
        self.built_at
            .map(|b| b.elapsed().as_secs_f32())
            .unwrap_or(0.0)
    }
}

/// Stage keys whose truncation WARN has been logged (once per key per boot).
static TRUNCATION_WARNED: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn first_truncation(key: &str) -> bool {
    let Ok(mut seen) = TRUNCATION_WARNED.lock() else {
        return false;
    };
    if seen.iter().any(|k| k == key) {
        return false;
    }
    seen.push(key.to_string());
    true
}

/// The pick for `kind` / `key` over the tables. A stage with more parts than
/// the side's frame-board budget keeps the first [`MAX_PREVIEW_INSTANCES`]
/// (one WARN); `None` for a key the tables no longer carry.
pub fn build_pick(
    kind: Kind,
    key: &str,
    tables: &Tables,
    rng: &mut Rng,
    side: usize,
) -> Option<Pick> {
    let (stages, camera_rows, dancers) = tables;
    match kind {
        Kind::Stage => {
            let mut pick = stage_only(rng, stages, camera_rows, key)?;
            if let Some(stage) = pick.stage.as_mut() {
                if stage.parts.len() > MAX_PREVIEW_INSTANCES {
                    if first_truncation(&stage.key) {
                        log_warn!(
                            "BackgroundDancers: preview P{} -- stage {} has {} parts, the frame board gives a preview {} -- the rest is not shown",
                            side + 1,
                            stage.key,
                            stage.parts.len(),
                            MAX_PREVIEW_INSTANCES
                        );
                    }
                    stage.parts.truncate(MAX_PREVIEW_INSTANCES);
                }
            }
            Some(pick)
        }
        Kind::Dancer => {
            // Part arcs probed through the same LayeredFS-aware resolver as
            // gameplay (a missing part is silent — A3 behaviour).
            let arc_exists =
                |arc: &str| arc_set::resolve_path(&format!("data/arc/{arc}")).is_some();
            dancer_only(rng, dancers, key, arc_exists)
        }
    }
}

/// The session a preview builds when its parse lands: the gameplay style
/// (FR-12), NO hulls, real time, the side's slot base and private pass-mask
/// bit; a stage-only scene gets the synthetic schedule for its camera cuts.
pub fn make_session(side: usize, pick: &Pick, parsed: Parsed, requested_at: Instant) -> Session {
    let side = side.min(1);
    let eff = style::effective();
    let session = Session::new(
        pick.clone(),
        parsed,
        requested_at,
        TempoOptions::REAL_TIME,
        eff.style,
        HullPlan::none(),
        SLOT_BASE[side],
        Some(FILTER_BIT[side]),
    );
    match synthetic_schedule(STAGE_CUT_PERIOD_S) {
        Some(fallback) => session.with_schedule(fallback),
        None => session,
    }
}
