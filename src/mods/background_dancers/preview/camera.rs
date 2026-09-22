//! Preview cameras (design §4.6 "preview/camera.rs" under the §4.7
//! amendment): the frustum a preview's pass clones render through this
//! frame — the stage's own `.camanm` director sample CROPPED to the box
//! aspect (vertical extent kept, horizontal = vertical × aspect), the
//! cropped gameplay fallback when the stage row has no camera set, or the
//! fixed frontal dancer camera — and its application to a `PassSet`.

use crate::services::scene3d::camera_math::{view_proj, Frustum};
use crate::services::scene3d::viewport_pass::PassSet;

use super::super::catalog::Kind;
use super::super::director;
use super::super::session::CameraSet;
use super::layout::{
    crop_to_aspect, dancer_extents, fallback_extents, Extents, DANCER_EYE, DANCER_FAR, DANCER_NEAR,
    DANCER_TARGET, DANCER_UP, FALLBACK_EYE, FALLBACK_FAR, FALLBACK_NEAR, FALLBACK_TARGET,
};
use super::scene::PreviewWindow;

fn with_extents(
    eye: [f32; 3],
    target: [f32; 3],
    up: [f32; 3],
    e: Extents,
    near: f32,
    far: f32,
) -> Frustum {
    Frustum {
        eye,
        target,
        up,
        w: 1.0,
        l: e.l,
        r: e.r,
        b: e.b,
        t: e.t,
        near,
        far,
    }
}

/// The dancer preview's fixed camera at the box aspect.
pub fn dancer_frustum(aspect: f32) -> Frustum {
    with_extents(
        DANCER_EYE,
        DANCER_TARGET,
        DANCER_UP,
        dancer_extents(aspect),
        DANCER_NEAR,
        DANCER_FAR,
    )
}

/// The no-camera-set fallback (the gameplay fixed camera, cropped).
pub fn fallback_frustum(aspect: f32) -> Frustum {
    with_extents(
        FALLBACK_EYE,
        FALLBACK_TARGET,
        [0.0, 1.0, 0.0],
        fallback_extents(aspect),
        FALLBACK_NEAR,
        FALLBACK_FAR,
    )
}

/// This frame's frustum for a live preview at scene time `t`.
pub fn frustum_for(window: &mut PreviewWindow, t: f32, aspect: f32) -> Frustum {
    match window.identity.0 {
        Kind::Dancer => dancer_frustum(aspect),
        Kind::Stage => {
            let sample = window
                .scene
                .session_mut()
                .filter(|s| s.has_camera())
                .and_then(|sess| director::camera_frame(sess, t, CameraSet::Stage));
            match sample {
                Some(cam) => {
                    // The `.camanm` frustum is symmetric and authored at 16:9
                    // (`l = -r`, `r = t × 16/9`): keep its vertical extent.
                    let f = cam.frustum();
                    with_extents(
                        f.eye,
                        f.target,
                        f.up,
                        crop_to_aspect(f.t.abs(), aspect),
                        f.near,
                        f.far,
                    )
                }
                None => fallback_frustum(aspect),
            }
        }
    }
}

/// Write the frustum's view/proj into both pass clones.
pub fn apply(frustum: &Frustum, passes: &mut PassSet) {
    let (view, proj) = view_proj(frustum);
    passes.set_camera(&view, &proj);
}
