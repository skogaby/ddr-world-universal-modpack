# progress — viewport-pass (Step 3 task-02)
- [x] `viewport_pass_layout.rs` (pure): `ClearViewport` repr(C) 0x40 with const offset pins (rect +8, flags +0x24,
      payload +0x28), `RtRect::from_canvas`/`aspect`, `ClearSpec::d3d_flags`, `encode_clear_record` (byte-exact test),
      `PRIO_BASE`/`FILTER_BIT`/`STOCK_FILTER_UNION`/`private_bits_free`; 4 tests (layout, record bytes, rect at
      720p/1080p/480p, tables).
- [x] `viewport_pass.rs` (engine-facing): `is_available()` (sub-group + four live stock passes identity-gated
      (vftable + self back-pointer) + filters 0x56/0x46 + private bits free, once, WARN once), RWX 2-slot vtable,
      `clear_render` (worker: 0x14-byte record at `*(ctx+gd_write_off)`, catch_unwind, no engine API),
      `render_target_dims`, `create` (clear vp + OPACITY/TRANS byte-clones: self/rect/minZ/maxZ/filter/flags patched;
      attach at base/+1/+2; every pointer probed; layout cross-check vs derived vp offsets), `PassSet::{set_rect,
      set_camera, set_enabled, detach}` (+ Drop detaches), `reap()` 2-frame grace.
- [x] `cargo check` clean; harness 127 ✓.
## Deviations
- Pure parts split into `viewport_pass_layout.rs` so the harness can mount them (the task anticipated this).
- Reaper stores block addresses as `usize` (a `Mutex<Vec<*mut u8>>` static is not `Sync`).
Status: Complete (uncommitted — maintainer commits manually)
