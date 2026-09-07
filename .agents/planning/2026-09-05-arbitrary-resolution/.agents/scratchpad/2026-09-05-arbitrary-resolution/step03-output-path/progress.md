
## Checkpoint run 1 → fixes (2026-09-05)
- Off-by-4 in `screen_w/h_global` (RIP after imm32) — fixed in `custom_resolution_anchors`; sweep values shifted +4 on every build.
- Window stayed 1280×720 under spice2x `-w`: root cause = hard-coded client size in the game's `main` window
  descriptor + spice2x swallowing MDX `SetWindowPos`. Added `window_client_size` AOB + `sites::window_client_sites`
  (+1 fixture test → 24) and two more imm writes in the OUTPUT set. Rebuilt; gates green.
Status: Complete (uncommitted — maintainer commits manually)

Checkpoint #1 run 3 PASSED 2026-09-05 (window 640x480, SD crop correct). Fixes along the way: RIP+4 screen globals, window_client_size AOB, present::fit_window (spice2x -w pins the client size).
