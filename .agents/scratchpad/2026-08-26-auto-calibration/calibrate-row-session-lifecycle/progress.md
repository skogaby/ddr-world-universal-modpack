# Progress — task-01-calibrate-row-session-lifecycle

Status: Complete (uncommitted — maintainer commits manually)
Date: 2026-08-26

- git mv src/mods/timing_offsets.rs -> src/mods/timing_offsets/mod.rs
- src/mods/timing_offsets/compute.rs: pure census + 3 host tests (harness auto-mounts; 7 tests total green)
- src/mods/timing_offsets/calibration.rs: ARMED + Session lifecycle, row registration (top of section), guards (census + rate), pulsing/refusal toasts, flip-OFF via idempotent re-registration, song_reset no-op seam, measurement seam stubs for Step 3
- mod.rs: mod decls + calibration::enable() before register_overlay_rows(), calibration::disable() first in disable()
- cargo check --target x86_64-pc-windows-msvc: clean
