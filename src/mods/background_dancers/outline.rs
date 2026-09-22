//! Scene-outline LAYER plan — the pure half of the inverted-hull outlines
//! (`style.rs` owns the live values, `session.rs` builds the hull items).
//!
//! Dependency-free on purpose (no `crate::` imports) so
//! `scripts/validate_background_dancers.sh` can `#[path]`-mount it.
//!
//! ## The two outline styles
//!
//! * **INK** — one hull per eligible mesh, the 0.03 grey the first cabinet
//!   build baked into the outline PS (`OUTLINE_RGB × a white tint`).
//! * **LAYERED** — the DDR World UI text look: several strokes stacked, the
//!   narrowest on top — black, then red, then blue by default. Each layer is
//!   its OWN hull item (records are counted from the resource, so one item is
//!   one draw per mesh — RE §4.6) whose rim is one `band` wider than the
//!   previous layer's; the z-test does the stacking (a narrower hull's
//!   back-facing shell fragment comes from a vertex nearer the silhouette —
//!   shallower on the mesh's far side — than a wider hull's at the same
//!   pixel, so layer 0 wins over layer 1 wins over layer 2 in ANY draw
//!   order). The colour travels in each hull record's own colour word
//!   (`render_item::set_record_colors`): the collector multiplies it into
//!   the body's white board tint and the outline PS emits COLOR0 verbatim,
//!   so one outline pair (program 0) serves every layer.
//!
//! Widths: layer `k` draws at `(k + 1) × base` (720p px, before the VS's
//! distance falloff), `base` = the per-kind width (`outline_px` /
//! `outline_px_stage`) — every stroke as wide as the black one, like the
//! text (2/4/6 px on dancers, 1.5/3/4.5 on props). A separate band knob was
//! tried and retired the same day (maintainer: equal strokes only).

/// The ink colour (0.03 grey — matches the first build's `OUTLINE_RGB`).
pub const INK_RGB: [f32; 3] = [0.03, 0.03, 0.03];
/// The default LAYERED palette, innermost first: black, red, blue.
pub const DEFAULT_LAYERED_RGB: [[f32; 3]; 3] = [INK_RGB, [0.90, 0.10, 0.15], [0.10, 0.35, 0.95]];
/// Every layer is a full extra draw of every eligible mesh — cap the count.
pub const MAX_LAYERS: usize = 4;

/// `background_dancers.outline_style`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutlineStyle {
    Ink,
    Layered,
}

impl OutlineStyle {
    /// Config spelling (case-insensitive). Unknown ⇒ `None` (the caller
    /// WARNs and falls back to INK).
    pub fn parse(s: &str) -> Option<OutlineStyle> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ink" | "single" | "black" => Some(OutlineStyle::Ink),
            "layered" | "layers" | "ddr" | "tricolor" | "tricolour" => Some(OutlineStyle::Layered),
            _ => None,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            OutlineStyle::Ink => "ink",
            OutlineStyle::Layered => "layered",
        }
    }

    /// Overlay-row value — INK / LAYERED.
    pub fn row_value(self) -> i32 {
        match self {
            OutlineStyle::Ink => 0,
            OutlineStyle::Layered => 1,
        }
    }

    pub fn from_row_value(v: i32) -> OutlineStyle {
        if v == 1 {
            OutlineStyle::Layered
        } else {
            OutlineStyle::Ink
        }
    }
}

/// One hull item's recipe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HullLayer {
    /// The record colour (alpha ALWAYS 1: the collector forces the blend
    /// group to 0x20 for an entry whose colour alpha is below 1).
    pub rgba: [f32; 4],
    /// Width step: layer 0 draws at the base width, layer `k` at `(k + 1) ×
    /// base`.
    pub step: usize,
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

    /// The plan for `style`: INK = one grey layer; LAYERED = one layer per
    /// palette entry (`palette` = the operator override or
    /// [`DEFAULT_LAYERED_RGB`]; capped at [`MAX_LAYERS`], an empty palette
    /// uses the default).
    pub fn for_style(style: OutlineStyle, palette: &[[f32; 3]]) -> HullPlan {
        let layers: Vec<HullLayer> = match style {
            OutlineStyle::Ink => vec![HullLayer {
                rgba: rgba(INK_RGB),
                step: 0,
            }],
            OutlineStyle::Layered => {
                let pal: &[[f32; 3]] = if palette.is_empty() {
                    &DEFAULT_LAYERED_RGB
                } else {
                    palette
                };
                pal.iter()
                    .take(MAX_LAYERS)
                    .enumerate()
                    .map(|(step, rgb)| HullLayer {
                        rgba: rgba(clamp_rgb(*rgb)),
                        step,
                    })
                    .collect()
            }
        };
        HullPlan { layers }
    }

    /// Rim width (720p px, pre-falloff) of layer `step` for a kind whose ink
    /// width is `base_px`.
    pub fn width(&self, base_px: f32, step: usize) -> f32 {
        layer_width(base_px, step)
    }

    pub fn is_none(&self) -> bool {
        self.layers.is_empty()
    }
}

/// `(step + 1) × base` — equal strokes.
pub fn layer_width(base_px: f32, step: usize) -> f32 {
    base_px * (step as f32 + 1.0)
}

/// Palette entries are plain 0..=1 colours (COLOR0 is emitted verbatim; the
/// draw is opaque so nothing above 1 can add).
pub fn clamp_rgb(rgb: [f32; 3]) -> [f32; 3] {
    let c = |v: f32| {
        if v.is_finite() {
            v.clamp(0.0, 1.0)
        } else {
            0.0
        }
    };
    [c(rgb[0]), c(rgb[1]), c(rgb[2])]
}

fn rgba(rgb: [f32; 3]) -> [f32; 4] {
    [rgb[0], rgb[1], rgb[2], 1.0]
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
    fn style_round_trips() {
        for s in [OutlineStyle::Ink, OutlineStyle::Layered] {
            assert_eq!(OutlineStyle::parse(s.key()), Some(s));
            assert_eq!(OutlineStyle::from_row_value(s.row_value()), s);
        }
        assert_eq!(
            OutlineStyle::parse(" LAYERED "),
            Some(OutlineStyle::Layered)
        );
        assert_eq!(OutlineStyle::parse("ddr"), Some(OutlineStyle::Layered));
        assert_eq!(OutlineStyle::parse("nope"), None);
        // Unknown row values degrade to INK.
        assert_eq!(OutlineStyle::from_row_value(7), OutlineStyle::Ink);
    }

    #[test]
    fn ink_is_one_grey_layer_at_the_base_width() {
        let p = HullPlan::for_style(OutlineStyle::Ink, &DEFAULT_LAYERED_RGB);
        assert_eq!(p.layers.len(), 1);
        assert_eq!(p.layers[0].rgba, [0.03, 0.03, 0.03, 1.0]);
        assert_eq!(p.layers[0].step, 0);
        assert_eq!(p.width(2.0, 0), 2.0);
        assert!(!p.is_none());
    }

    #[test]
    fn layered_default_is_black_red_blue_equal_strokes() {
        let p = HullPlan::for_style(OutlineStyle::Layered, &[]);
        assert_eq!(p.layers.len(), 3);
        assert_eq!(p.layers[0].rgba[..3], INK_RGB);
        assert_eq!(p.layers[1].rgba[..3], DEFAULT_LAYERED_RGB[1]);
        assert_eq!(p.layers[2].rgba[..3], DEFAULT_LAYERED_RGB[2]);
        for (k, l) in p.layers.iter().enumerate() {
            assert_eq!(l.step, k);
            assert_eq!(l.rgba[3], 1.0, "alpha must stay 1 (blend-group force)");
        }
        // Equal strokes of the base width: 2 / 4 / 6 dancers, 1.5 / 3 / 4.5
        // stage props.
        assert_eq!(p.width(2.0, 0), 2.0);
        assert_eq!(p.width(2.0, 1), 4.0);
        assert_eq!(p.width(2.0, 2), 6.0);
        assert_eq!(p.width(1.5, 2), 4.5);
        assert_eq!(hex(p.layers[1].rgba), "#e61a26");
    }

    #[test]
    fn layered_palette_override() {
        let pal = [[1.0, 1.0, 1.0], [0.0, 0.0, 0.0]];
        let p = HullPlan::for_style(OutlineStyle::Layered, &pal);
        assert_eq!(p.layers.len(), 2);
        assert_eq!(p.layers[0].rgba, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(p.layers[1].rgba, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(p.width(2.0, 0), 2.0);
        assert_eq!(p.width(2.0, 1), 4.0);
        // Cap + channel clamp.
        let many = [[2.0, -1.0, f32::NAN]; 9];
        let p = HullPlan::for_style(OutlineStyle::Layered, &many);
        assert_eq!(p.layers.len(), MAX_LAYERS);
        assert_eq!(p.layers[0].rgba, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn no_plan_has_no_layers() {
        assert!(HullPlan::none().is_none());
        assert_eq!(layer_width(2.0, 3), 8.0);
        assert_eq!(layer_width(1.5, 2), 4.5);
    }
}
