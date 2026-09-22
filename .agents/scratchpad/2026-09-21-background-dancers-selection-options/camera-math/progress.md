# progress — camera-math (Step 3 task-01)
- [x] `src/services/scene3d/camera_math.rs` (pure): `Mat4`, `IDENTITY`, `Frustum` (+ `perspective`), `view_look_at_rh`
      (engine LookAtRH rows, degenerate ⇒ identity), `proj_off_centre` (engine D3D formula incl. far ≤ 0 branch),
      `view_proj`, `transform_point`; 6 tests (axis-aligned rows, oblique orthonormality/RH, degenerate, depth
      range 0..1, off-centre + far-0 branch, frustum framing).
- [x] `CamSample::frustum()` in scene_graph.rs; harness mount; `validate_background_dancers.sh` 123 ✓ (then 127 with task-02).
Status: Complete (uncommitted — maintainer commits manually)
