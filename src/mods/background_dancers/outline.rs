//! Scene-outline plan — the pure half of the inverted-hull outlines
//! (`style.rs` owns the on/off switch, `session.rs` builds the hull items).
//!
//! Dependency-free on purpose (no `crate::` imports) so
//! `scripts/validate_background_dancers.sh` can `#[path]`-mount it.
//!
//! Since 2026-09-28 the outline is DANCING STAGE UNLEASHED's own (design
//! `.agents/planning/2026-09-28-dsu-dancer-port/design.md` D4): ONE black hull
//! per eligible mesh, pushed out along the skinned normal by DSU's
//! depth-scaled amount — the geometry lives in the outline VS
//! (`shaders/src/mdl_cel.hlsl`), the colour rides the hull's draw records
//! (`render_item::set_record_colors` → c23 → the outline PS emits it
//! verbatim). The 2026-09-21 LAYERED style (stacked black/red/blue strokes)
//! and the per-kind 720p pixel widths were retired with it.
//!
//! The instance table (`instance_plan.rs`) still takes a layer COUNT, so a
//! future multi-stroke look can come back without touching it.

/// The ink colour — DSU's `def c3, 0, 0, 0, 1` in the toon outline VS.
pub const INK_RGB: [f32; 3] = [0.0, 0.0, 0.0];

/// `ModelParameters.w` of a hull item: the multiplier the outline VS applies
/// to DSU's push (`1.0` = DSU; the shader reads 0 as 1).
pub const PUSH_SCALE: f32 = 1.0;

/// One hull item's recipe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HullLayer {
    /// The record colour (alpha ALWAYS 1: the collector forces the blend
    /// group to 0x20 for an entry whose colour alpha is below 1).
    pub rgba: [f32; 4],
    /// `ModelParameters.w` for this hull (see [`PUSH_SCALE`]).
    pub push_scale: f32,
}

/// What a session builds for its outlines: no layers = no hulls.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct HullPlan {
    pub layers: Vec<HullLayer>,
}

impl HullPlan {
    pub fn none() -> HullPlan {
        HullPlan::default()
    }

    /// The DSU outline: one black hull at DSU's push.
    pub fn ink() -> HullPlan {
        HullPlan {
            layers: vec![HullLayer {
                rgba: [INK_RGB[0], INK_RGB[1], INK_RGB[2], 1.0],
                push_scale: PUSH_SCALE,
            }],
        }
    }

    pub fn is_none(&self) -> bool {
        self.layers.is_empty()
    }
}

/// A short `#rrggbb` for the log lines.
pub fn hex(rgba: [f32; 4]) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(rgba[0]), b(rgba[1]), b(rgba[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ink_is_one_black_hull_at_the_dsu_push() {
        let p = HullPlan::ink();
        assert_eq!(p.layers.len(), 1);
        assert_eq!(p.layers[0].rgba, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(
            p.layers[0].rgba[3], 1.0,
            "alpha must stay 1 (blend-group force)"
        );
        assert_eq!(p.layers[0].push_scale, 1.0);
        assert!(!p.is_none());
        assert_eq!(hex(p.layers[0].rgba), "#000000");
    }

    #[test]
    fn no_plan_has_no_layers() {
        assert!(HullPlan::none().is_none());
        assert_eq!(HullPlan::none().layers.len(), 0);
    }
}
