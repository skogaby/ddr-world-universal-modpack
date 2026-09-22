//! Camera matrices for the viewport-pass compositor (design §4.4): the two
//! 4×4 matrices the MODEL passes consume, built exactly like the engine's
//! own camera rebuilds — the LookAtRH view (`FUN_180220b80` on 20260825)
//! and the D3D off-centre projection (`FUN_1802376e0`, the one copied into
//! `pass+0x58`). Row-major, ROW-VECTOR convention (`v' = v · M`, so the
//! translation lives in the LAST row) — the engine's, and what `pass+0x98`
//! / `+0x58` hold.
//!
//! With these a preview pass never needs camera slot 0: the compositor
//! writes the clone's view/proj itself (research `preview-compositing.md`
//! §6, Ghidra-confirmed 2026-09-21).
//!
//! Dependency-free (std only) so the host harness mounts it beside the
//! other pure `scene3d` files.

/// Row-major 4×4 (`m[row*4 + col]`).
pub type Mat4 = [f32; 16];

pub const IDENTITY: Mat4 = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
];

/// A camera: eye / target / up plus the near-plane frustum extents at
/// depth `w` (the engine's `w` field: 1.0 for every perspective camera the
/// DLL builds; `l/r/b/t` are then the half-tangents) and the clip planes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frustum {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub up: [f32; 3],
    pub w: f32,
    pub l: f32,
    pub r: f32,
    pub b: f32,
    pub t: f32,
    pub near: f32,
    pub far: f32,
}

impl Frustum {
    /// Symmetric perspective at `w = 1`: `half_tangent_x` = half the
    /// horizontal extent per unit depth, `aspect` = width / height.
    pub fn perspective(
        eye: [f32; 3],
        target: [f32; 3],
        up: [f32; 3],
        half_tangent_x: f32,
        aspect: f32,
        near: f32,
        far: f32,
    ) -> Frustum {
        let ty = if aspect > 0.0 {
            half_tangent_x / aspect
        } else {
            half_tangent_x
        };
        Frustum {
            eye,
            target,
            up,
            w: 1.0,
            l: -half_tangent_x,
            r: half_tangent_x,
            b: -ty,
            t: ty,
            near,
            far,
        }
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// `None` when the vector has no length (the engine keeps the raw vector
/// in that case; we treat it as a degenerate camera).
fn normalize(v: [f32; 3]) -> Option<[f32; 3]> {
    let len = dot(v, v).sqrt();
    if len == 0.0 || !len.is_finite() {
        return None;
    }
    Some([v[0] / len, v[1] / len, v[2] / len])
}

/// The engine's LookAtRH view (`FUN_180220b80`): `f = normalize(eye −
/// target)`, `s = normalize(up × f)`, `u = normalize(f × s)`; rows
/// `(s.x,u.x,f.x,0) (s.y,u.y,f.y,0) (s.z,u.z,f.z,0) (−eye·s, −eye·u,
/// −eye·f, 1)`. A zero-length `f` or `s` (eye == target, or up ∥ f)
/// returns [`IDENTITY`] — the caller logs once.
pub fn view_look_at_rh(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> Mat4 {
    let Some(f) = normalize(sub(eye, target)) else {
        return IDENTITY;
    };
    let Some(s) = normalize(cross(up, f)) else {
        return IDENTITY;
    };
    let u = normalize(cross(f, s)).unwrap_or(cross(f, s));
    [
        s[0],
        u[0],
        f[0],
        0.0,
        s[1],
        u[1],
        f[1],
        0.0,
        s[2],
        u[2],
        f[2],
        0.0,
        -dot(eye, s),
        -dot(eye, u),
        -dot(eye, f),
        1.0,
    ]
}

/// The engine's D3D projection (`FUN_1802376e0`, `w > 0` branch, depth in
/// `[0, 1]`): `[0][0] = 2w/(r−l)`, `[1][1] = 2w/(t−b)`, `[2][0] =
/// (r+l)/(r−l)`, `[2][1] = (t+b)/(t−b)`, `[2][2] = −far/(far−near)`,
/// `[2][3] = −1`, `[3][2] = −far·near/(far−near)`; `far ≤ 0` degenerates to
/// `[2][2] = −1`, `[3][2] = −near`. Rest zero. (`w ≤ 0` selects the
/// engine's orthographic branch — not reproduced; callers pass `w > 0`.)
pub fn proj_off_centre(w: f32, l: f32, r: f32, b: f32, t: f32, near: f32, far: f32) -> Mat4 {
    let mut m = [0.0f32; 16];
    let w2 = w * 2.0;
    m[0] = w2 / (r - l);
    m[5] = w2 / (t - b);
    m[8] = (r + l) / (r - l);
    m[9] = (t + b) / (t - b);
    if far <= 0.0 {
        m[10] = -1.0;
        m[14] = -near;
    } else {
        m[10] = -far / (far - near);
        m[14] = -(far * near) / (far - near);
    }
    m[11] = -1.0;
    m
}

/// `(view, proj)` for a frustum — what the compositor writes into a pass
/// clone's `+0x98` / `+0x58`.
pub fn view_proj(f: &Frustum) -> (Mat4, Mat4) {
    (
        view_look_at_rh(f.eye, f.target, f.up),
        proj_off_centre(f.w, f.l, f.r, f.b, f.t, f.near, f.far),
    )
}

/// Row-vector transform `(p, 1) · m` (for the tests and CPU-side checks).
pub fn transform_point(m: &Mat4, p: [f32; 3]) -> [f32; 4] {
    let v = [p[0], p[1], p[2], 1.0];
    let mut out = [0.0f32; 4];
    for (col, o) in out.iter_mut().enumerate() {
        *o = v[0] * m[col] + v[1] * m[4 + col] + v[2] * m[8 + col] + v[3] * m[12 + col];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    fn assert_mat(got: &Mat4, want: &Mat4) {
        for i in 0..16 {
            assert!(approx(got[i], want[i]), "m[{i}] {} vs {}", got[i], want[i]);
        }
    }

    #[test]
    fn view_axis_aligned_matches_the_engine_rows() {
        // Camera on +z looking down −z: f = +z, s = up × f = (0,1,0)×(0,0,1)
        // = +x, u = f × s = +y. Last row = (−eye·s, −eye·u, −eye·f, 1).
        let v = view_look_at_rh([0.0, 1.0, 5.0], [0.0, 1.0, 0.0], [0.0, 1.0, 0.0]);
        assert_mat(
            &v,
            &[
                1.0, 0.0, 0.0, 0.0, //
                0.0, 1.0, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0, //
                0.0, -1.0, -5.0, 1.0,
            ],
        );
        // The eye maps to the origin; the target sits 5 in FRONT (−z, RH).
        let e = transform_point(&v, [0.0, 1.0, 5.0]);
        assert!(approx(e[0], 0.0) && approx(e[1], 0.0) && approx(e[2], 0.0) && approx(e[3], 1.0));
        let t = transform_point(&v, [0.0, 1.0, 0.0]);
        assert!(approx(t[2], -5.0), "target depth {}", t[2]);
    }

    #[test]
    fn view_oblique_is_orthonormal_and_centres_the_eye() {
        let eye = [3.0, 2.0, 7.0];
        let v = view_look_at_rh(eye, [0.5, 0.9, -1.0], [0.0, 1.0, 0.0]);
        let s = [v[0], v[4], v[8]];
        let u = [v[1], v[5], v[9]];
        let f = [v[2], v[6], v[10]];
        for a in [s, u, f] {
            assert!(approx(dot(a, a), 1.0));
        }
        assert!(approx(dot(s, u), 0.0) && approx(dot(u, f), 0.0) && approx(dot(s, f), 0.0));
        // Right-handed: s × u = f.
        let sxu = cross(s, u);
        assert!(approx(sxu[0], f[0]) && approx(sxu[1], f[1]) && approx(sxu[2], f[2]));
        let e = transform_point(&v, eye);
        assert!(approx(e[0], 0.0) && approx(e[1], 0.0) && approx(e[2], 0.0));
        // Anything at the target lies on the −z axis (x = y = 0).
        let t = transform_point(&v, [0.5, 0.9, -1.0]);
        assert!(approx(t[0], 0.0) && approx(t[1], 0.0) && t[2] < 0.0);
    }

    #[test]
    fn view_degenerate_returns_identity() {
        assert_eq!(
            view_look_at_rh([1.0, 2.0, 3.0], [1.0, 2.0, 3.0], [0.0, 1.0, 0.0]),
            IDENTITY
        );
        // up parallel to f: s has no length.
        assert_eq!(
            view_look_at_rh([0.0, 5.0, 0.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            IDENTITY
        );
    }

    #[test]
    fn projection_symmetric_depth_range_and_layout() {
        let p = proj_off_centre(1.0, -0.5, 0.5, -0.28125, 0.28125, 0.1, 100.0);
        assert!(approx(p[0], 2.0));
        assert!(approx(p[5], 2.0 / 0.5625));
        assert!(approx(p[8], 0.0) && approx(p[9], 0.0));
        assert!(approx(p[11], -1.0));
        assert!(approx(p[15], 0.0));
        // Only the seven engine-written cells are non-zero.
        for i in [1, 2, 3, 4, 6, 7, 12, 13, 15] {
            assert_eq!(p[i], 0.0, "m[{i}]");
        }
        // View-space depth near ⇒ NDC 0, far ⇒ NDC 1 (RH: in front is −z).
        let n = transform_point(&p, [0.0, 0.0, -0.1]);
        assert!(approx(n[2] / n[3], 0.0), "near ndc {}", n[2] / n[3]);
        let f = transform_point(&p, [0.0, 0.0, -100.0]);
        assert!(approx(f[2] / f[3], 1.0), "far ndc {}", f[2] / f[3]);
        // w after the transform is the positive depth.
        assert!(approx(n[3], 0.1) && approx(f[3], 100.0));
    }

    #[test]
    fn projection_off_centre_and_far_zero_branch() {
        // A .camanm-style asymmetric frustum: the [2][0]/[2][1] shears.
        let p = proj_off_centre(1.0, -0.2, 0.6, -0.3, 0.1, 0.5, 50.0);
        assert!(approx(p[0], 2.0 / 0.8));
        assert!(approx(p[5], 2.0 / 0.4));
        assert!(approx(p[8], 0.4 / 0.8));
        assert!(approx(p[9], -0.2 / 0.4));
        assert!(approx(p[10], -50.0 / 49.5));
        assert!(approx(p[14], -(50.0 * 0.5) / 49.5));
        // far ≤ 0 ⇒ the engine's degenerate depth mapping.
        let q = proj_off_centre(1.0, -1.0, 1.0, -1.0, 1.0, 0.25, 0.0);
        assert!(approx(q[10], -1.0) && approx(q[14], -0.25));
    }

    #[test]
    fn frustum_perspective_and_view_proj() {
        let fr = Frustum::perspective(
            [0.0, 1.05, 3.4],
            [0.0, 0.95, 0.0],
            [0.0, 1.0, 0.0],
            0.32 * (170.0 / 150.0),
            170.0 / 150.0,
            0.1,
            100.0,
        );
        assert!(approx(fr.r, -fr.l) && approx(fr.t, -fr.b));
        assert!(approx(fr.t, 0.32));
        assert_eq!(fr.w, 1.0);
        let (v, p) = view_proj(&fr);
        // The target projects to the screen centre.
        let vt = transform_point(&v, fr.target);
        let ct = transform_point(&p, [vt[0], vt[1], vt[2]]);
        assert!(approx(ct[0] / ct[3], 0.0) && approx(ct[1] / ct[3], 0.0));
        // A point at the top of the frustum at the target's depth lands at
        // NDC y = +1.
        let depth = -vt[2];
        let top = transform_point(&p, [0.0, fr.t * depth, -depth]);
        assert!(
            approx(top[1] / top[3], 1.0),
            "top ndc y {}",
            top[1] / top[3]
        );
        // Zero aspect keeps the vertical extent equal to the horizontal.
        let z = Frustum::perspective(
            [0.0; 3],
            [0.0, 0.0, -1.0],
            [0.0, 1.0, 0.0],
            0.5,
            0.0,
            1.0,
            2.0,
        );
        assert_eq!(z.t, 0.5);
    }
}
