# Progress — widget-pool-diagnostic

Status: Complete (uncommitted — maintainer commits manually)

## Checklist

- [x] `log_free_pool_count_once()` + `POOL_DIAG_LOGGED` latch in
      `src/services/widget_renderer.rs`; one-shot from `wrapper_render_hook`
      (relaxed atomic after first frame)
- [x] Guard ladder: scene_manager_global / scene manager / render-list manager /
      head+sentinel null checks + 4096-node walk cap; poison-recover lock
      (extern "C" frame)
- [x] `cargo check` (0 warnings), `cargo fmt` (no churn), `./build.sh` clean
- [x] Autonomous CrossOver boot validation (maintainer-authorized): DLL deployed to the
      bottle install (previous DLL backed up as `ddr_world_hook.dll.bak-step01`),
      game booted through attract, exactly one INFO line observed:
      `WidgetRenderer: render list free pool: 254 node(s) available`
- [x] Log regression check: warn/error profile identical to the previous run
      (7 pre-existing lines: crash-handler banner, 3 known missing signatures on this
      build, series-expansion unconfigured). Game killed cleanly after harvest.

## Key result

**Pool size answered: 254 free nodes at first wrapper render.** Current repo worst-case
consumption ~49; the new menu's ~39 additional widgets fit with ~5× headroom. The
design's "pool unknown" risk is retired (design §4.5 assumption validated).

## Runbook note

Fast Bootup makes attract reachable ~30 s after window creation — 35 s of wait is
plenty for future boot-and-harvest runs (maintainer guidance, this session).

## Deviations

None.
