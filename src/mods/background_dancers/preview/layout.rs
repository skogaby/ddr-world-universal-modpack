//! Preview geometry + constants — PURE (std only; the host harness mounts
//! it). Where the options modal's preview box sits on the 1280×720 canvas,
//! the per-side compositor constants (design §5.3) and the frustum EXTENT
//! maths the two preview cameras use under the 2026-09-21 amendment of
//! design §4.7: the box is 170×150, not 16:9, so a stage's `.camanm` frame
//! keeps its VERTICAL half-tangent and is CROPPED horizontally to the box
//! aspect, and the fixed dancer camera simply frames at that aspect.

/// The options modal's preview-panel origin per side in the 1280×720 canvas
/// (the WebUI overlays' measured constant; the template marker rect is
/// relative to it).
pub const CHROME_ORIGIN: [(f32, f32); 2] = [(185.0, 463.0), (742.0, 463.0)];
/// The shipped `background_dancer` / `background_stage` template marker
/// `(x, y, w, h)` relative to the panel — the fallback when the template
/// PNG is unreadable at runtime.
pub const FALLBACK_MARKER: (f32, f32, f32, f32) = (191.0, 11.0, 170.0, 150.0);

/// Value edits re-target the preview this long after the LAST change.
pub const SETTLE_MS: u64 = 150;
/// Frame-board slot base per side (each preview ≤ [`MAX_PREVIEW_INSTANCES`]).
pub const SLOT_BASE: [u32; 2] = [0, 16];
pub const MAX_PREVIEW_INSTANCES: usize = 16;
/// The synthetic dance schedule's pseudo-clip length for stage-only scenes:
/// the camera event loop cuts every `9.0 − CUT_LEAD = 7.5 s`.
pub const STAGE_CUT_PERIOD_S: f32 = 9.0;

/// The fixed dancer camera: a frontal ¾ view framing a 1.8 m figure with
/// headroom (tunable; the final values are recorded here after the cabinet
/// pass).
pub const DANCER_EYE: [f32; 3] = [0.0, 1.05, 3.4];
pub const DANCER_TARGET: [f32; 3] = [0.0, 0.95, 0.0];
pub const DANCER_UP: [f32; 3] = [0.0, 1.0, 0.0];
/// Vertical half-tangent (`w = 1`): 2 × 0.32 × 3.4 m ≈ 2.2 m tall at the
/// dancer's depth.
pub const DANCER_HALF_TANGENT_Y: f32 = 0.32;
pub const DANCER_NEAR: f32 = 0.1;
pub const DANCER_FAR: f32 = 100.0;

/// The no-camera-set fallback = the gameplay fixed camera (A3 add-on
/// framing, `docs/3d_model_format_research.md` §6): eye (0, 1.6, 5) →
/// (0, 0.9, 0), hFOV 76.8° AT 16:9 — cropped to the box like a `.camanm`.
pub const FALLBACK_EYE: [f32; 3] = [0.0, 1.6, 5.0];
pub const FALLBACK_TARGET: [f32; 3] = [0.0, 0.9, 0.0];
pub const FALLBACK_HFOV_DEG: f32 = 76.8;
pub const FALLBACK_NEAR: f32 = 0.1;
pub const FALLBACK_FAR: f32 = 500.0;
/// The aspect the `.camanm` cameras and the fallback are authored at.
pub const AUTHORED_ASPECT: f32 = 16.0 / 9.0;

/// The colour clear behind every preview (D3DCOLOR `0xAARRGGBB`): a dark,
/// near-black blue — the backdrop behind a dancer, and cover for the chrome
/// wherever a cropped stage has no geometry.
pub const BACKDROP_ARGB: u32 = 0xFF0C_0C14;

/// A rectangle on the 1280×720 canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl CanvasRect {
    /// Width / height (1.0 for a degenerate height).
    pub fn aspect(&self) -> f32 {
        if self.h > 0.0 {
            self.w / self.h
        } else {
            1.0
        }
    }
}

/// The preview box of `side` (0 = P1, 1 = P2; anything else = P2) for a
/// template marker `(x, y, w, h)`: the panel origin plus the marker.
pub fn box_rect(side: usize, marker: (f32, f32, f32, f32)) -> CanvasRect {
    let (ox, oy) = CHROME_ORIGIN[side.min(1)];
    CanvasRect {
        x: ox + marker.0,
        y: oy + marker.1,
        w: marker.2,
        h: marker.3,
    }
}

/// Symmetric frustum extents at `w = 1` (half-tangents) — what
/// `camera_math::Frustum` carries as `l / r / b / t`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Extents {
    pub l: f32,
    pub r: f32,
    pub b: f32,
    pub t: f32,
}

/// Keep a vertical half-tangent, set the horizontal one to `v × aspect`: a
/// centre crop (or widening) of the authored frame to the box aspect.
pub fn crop_to_aspect(vertical_half_tangent: f32, aspect: f32) -> Extents {
    let v = vertical_half_tangent;
    let h = v * aspect;
    Extents {
        l: -h,
        r: h,
        b: -v,
        t: v,
    }
}

/// The dancer camera's extents at the box aspect.
pub fn dancer_extents(aspect: f32) -> Extents {
    crop_to_aspect(DANCER_HALF_TANGENT_Y, aspect)
}

/// The fallback camera's extents: its 16:9 hFOV turned into the vertical
/// half-tangent, then cropped to the box aspect.
pub fn fallback_extents(aspect: f32) -> Extents {
    let half_x_16_9 = (FALLBACK_HFOV_DEG * 0.5).to_radians().tan();
    crop_to_aspect(half_x_16_9 / AUTHORED_ASPECT, aspect)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn box_rects_per_side() {
        let p1 = box_rect(0, FALLBACK_MARKER);
        assert_eq!(
            p1,
            CanvasRect {
                x: 376.0,
                y: 474.0,
                w: 170.0,
                h: 150.0
            }
        );
        let p2 = box_rect(1, FALLBACK_MARKER);
        assert_eq!(
            p2,
            CanvasRect {
                x: 933.0,
                y: 474.0,
                w: 170.0,
                h: 150.0
            }
        );
        assert_eq!(box_rect(7, FALLBACK_MARKER), p2);
        assert!(close(p1.aspect(), 170.0 / 150.0));
        assert_eq!(
            CanvasRect {
                x: 0.0,
                y: 0.0,
                w: 5.0,
                h: 0.0
            }
            .aspect(),
            1.0
        );
    }

    #[test]
    fn crop_keeps_vertical_sets_horizontal() {
        let e = crop_to_aspect(0.5, 2.0);
        assert_eq!(
            e,
            Extents {
                l: -1.0,
                r: 1.0,
                b: -0.5,
                t: 0.5
            }
        );
        let d = dancer_extents(170.0 / 150.0);
        assert!(close(d.t, 0.32) && close(d.b, -0.32));
        assert!(close(d.r, 0.32 * 170.0 / 150.0), "{d:?}");
        assert!(close(d.l, -d.r));
    }

    #[test]
    fn fallback_reproduces_the_gameplay_frustum_at_16_9() {
        let wide = fallback_extents(AUTHORED_ASPECT);
        let half_x = (76.8f32 * 0.5).to_radians().tan();
        assert!(close(wide.r, half_x), "{wide:?}");
        assert!(close(wide.t, half_x / AUTHORED_ASPECT));
        // At the box aspect the vertical extent is unchanged (a crop).
        let boxed = fallback_extents(170.0 / 150.0);
        assert!(close(boxed.t, wide.t));
        assert!(boxed.r < wide.r);
        assert!(close(boxed.r, boxed.t * 170.0 / 150.0));
    }

    #[test]
    fn constants() {
        assert_eq!(SLOT_BASE, [0, 16]);
        assert_eq!(MAX_PREVIEW_INSTANCES, 16);
        assert_eq!(SETTLE_MS, 150);
        assert!((STAGE_CUT_PERIOD_S - 9.0).abs() < 1e-6);
        // ARGB: opaque, dark.
        assert_eq!(BACKDROP_ARGB >> 24, 0xFF);
        assert!(((BACKDROP_ARGB >> 16) & 0xFF) < 0x20);
        assert_eq!(CHROME_ORIGIN[1].0 - CHROME_ORIGIN[0].0, 557.0);
    }
}
