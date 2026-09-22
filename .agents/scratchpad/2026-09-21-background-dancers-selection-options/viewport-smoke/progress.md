# progress — viewport-smoke (Step 3 task-03)
- [x] `background_dancers/viewport_smoke.rs`: gate = developer_mode ∧ `DDR_DANCERS_VIEWPORT_SMOKE`; scene callback
      records arm/teardown requests; `on_frame` creates the P1 `PassSet` (marker rect from the template via
      `marker_rect_for`, fallback (191,11,170,150); `CHROME_ORIGIN` P1 (185,463); rect scaled by
      `render_target_dims`), violet 0xFF20A0FF depth+colour clear, 3 s attached → 1 s DISABLED → detach; INFO per
      transition; `shutdown()` on disable.
- [x] `mod.rs`: `init_from_env` at enable, scene/frame wiring, the ONE `viewport_pass::reap()` call per frame.
- [x] `cargo check`, `cargo fmt`, `./build.sh` release clean (58 s).
- [ ] CABINET SMOKE — pending (maintainer).
Status: Complete (uncommitted — maintainer commits manually); cabinet validation pending
