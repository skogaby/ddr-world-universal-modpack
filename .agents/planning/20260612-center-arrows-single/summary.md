# Summary — Center Arrows for Single Player (PDD)

A per-player, single-player-gated mod that centers the playfield (arrow receptors + the
lane-relative readouts), implemented as a hook on the 64-bit HUD layout builder rather than a
byte-patch port of the 32-bit hex hack.

## Artifacts

```
.agents/planning/20260612-center-arrows-single/
├── rough-idea.md                      # initial concept (from docs/hex_edit_porting.md Hack 2)
├── idea-honing.md                     # Q1–Q9 requirements Q&A + provisional summary
├── research/
│   ├── r1-lane-skin-reposition.md     # FEASIBLE — single lane is an AFP layer; A→B→fallback
│   ├── r2-singleplayer-active-side.md # play-state @ root+0x84+side*4; parent→side mapping
│   ├── r3-hook-and-coords.md          # FUN_18006f5d0(parent,name,coord); coord[0]=X
│   └── r4-option-texture.md           # gen_option_labels.py LABELS entry; reuse on/off ribbons
├── design/
│   └── detailed-design.md             # full design (two detours, gating, lane-skin strategy)
├── implementation/
│   └── plan.md                        # 9-step incremental plan + checklist
└── summary.md                         # this file
```

## Design in brief

- **Two detours** on the gameplay HUD layout builder:
  - Entry hook on `FUN_18006c230` → captures `builder_root`, computes
    `{single_player, active_side}` from per-side play-states (`root+0x84+side*4`, `==2` ⇒
    inactive).
  - Setter hook on `FUN_18006f5d0(parent, name, coord)` → maps `parent→side`, and for the
    active single-player side rewrites `coord[0]` (X) to `CENTER_X=495` for the lane-relative
    keys: `arrow_raw, arrow, freeze_judge, judge, combo, fast_slow, filter, score_compare`
    (score/gauge left alone).
- **Per-player option** "CENTER ARROWS (1P ONLY)" (`center_arrows_1p`, bool, default OFF) via
  `custom_options`; no cross-sync; standard persistence. Registered **only if both detours
  install** (no inert rows).
- **Hard single-player gate:** never centers in 2P/versus regardless of option values.
- **Lane skin:** staged — A (element-only rewrite), B (reposition single lane AFP layer via
  `bm2d_api::set_position`), fallback (force-double); A↔B settled by one cabinet test.
- **Safety:** graceful degradation, panic-guarded FFI callbacks, range/alignment-checked side
  index, no allocation on the centering path. Apply-at-build-time (passive); no live re-layout.

## Implementation plan (9 steps)

Signatures → scaffold → builder-entry detour + detection diagnostic → setter detour + key
diagnostic → apply X-centering (Strategy A) → per-player option row + texture → gate
registration on hook success + safety pass → cabinet validation matrix + lane-skin decision →
(conditional) Strategy B lane reposition. Each step is `cargo check`-gated and most end in a
diagnostic/visual deploy check; full `./build.sh` before deploys.

## Next steps

1. Review `design/detailed-design.md` and `implementation/plan.md`.
2. Begin Step 1 (signatures) — author + uniqueness-check the two AOB patterns on both builds.
3. Proceed through the checklist, deploying the diagnostic builds at Steps 3–5 to confirm
   detection and the lane-skin behavior before committing to Strategy A vs B.

## Areas that may need refinement

- **Lane-skin strategy (A/B/fallback)** is intentionally deferred to a runtime test (Step 8) —
  the only genuinely empirical unknown.
- **`+0x84` play-state semantics** and the `parent→side` offsets are from the 20260324
  decompile; validated live in Step 3 and spot-checked on 20260526.
- **AOB uniqueness** for both functions must be confirmed on both builds (Step 1).
- **`CENTER_X` in AFP units** (Step 9 only) may differ from the layout-space 495 and would be
  derived if Strategy B is needed.
