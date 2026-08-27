# Progress — overlay-draw-poc

Status: Complete (uncommitted — maintainer commits manually)

## Checklist

- [x] Encoder extension: `set_context_2d` (tag 0x07; handler decompiled) + tests
      (12 total green via scripts/validate_overlay_draw.sh)
- [x] `overlay_draw::init` (lib.rs 6c2) — default_shader global + env latch
- [x] `on_wrapper_render()` from `wrapper_render_hook`: per-scene diagnostics
      (bounded, always on) + POC emission behind `DDR_OVERLAY_DRAW_POC`
- [x] Gate ladder (list / bump invariant / soft cap / shader / program count) with
      latched WARNs; arena-reset frame gate; copy + single bump; 600-emission heartbeat
- [x] `cargo check` 0 warnings → `cargo fmt` no churn → `./build.sh` clean
- [x] Autonomous boot 1 (diagnostics): list valid + bump_ok in EVERY scene −1..16;
      default shader resident everywhere, progs=2; arena sizes ≤0x96EC
- [x] Autonomous boot 2 (POC): 18,000+ emissions across multiple attract cycles,
      zero WARNs, zero crashes
- [x] Maintainer session (2026-08-24): quad visible + pixel-exact scissor; quad ABOVE
      all game content, BELOW the DLL's menu text widgets (the exact sandwich the
      design needs). Mod menu also confirmed opening/rendering post-restructure
      (Step 1's deferred interactive check — done).
- [x] docs/overlay_draw_research.md finalized: **GO** verdict + production recipe

## Key finding

Active list switches mid-frame (≈5 lists/frame observed) — the POC emits into every
list the gate re-arms on. Production emitter needs layer identity (the visual session
shows what each layer's quad looks like). Full notes: docs/overlay_draw_research.md.

## Deviations

None from the task spec. The multi-list behavior is recorded as a spike finding, not
a defect.
