# Implementation Plan: Perspective Expansion — Distant, Incoming, Space

Status: Approved 2026-07-31

Design: `.agents/planning/20260731-perspective-expansion/design/detailed-design.md`
(Approved 2026-07-31). This plan decomposes it; it does not restate design detail.

Per repo convention there are no unit tests — each code step's gate is
`cargo check` → `cargo fmt` (whole crate) → `./build.sh`. **All cabinet-level
validation is deliberately consolidated into the final step** (maintainer's call:
incremental on-cabinet deliverables carry little value for this feature). Maintain
`progress.md` in this planning directory from the first step onward (AGENTS.md
convention), including the Deploy & test log once Step 4 begins.

## Checklist

- [x] Step 1: Generalize the perspective constant pipeline (behavior-preserving)
- [x] Step 2: Add the three presets end-to-end (enum, config, constants, assets)
- [x] Step 3: Base-zoom VS extension + blob recompile (DISTANT/SPACE containment)
- [x] Step 4: Full cabinet verification matrix, default tuning, and docs

---

## Step 1: Generalize the perspective constant pipeline (behavior-preserving)

**Objective:** Replace the three inlined copies of the hallway math with the single
`compute_constants` source of truth, and start emitting `c49.y = z0` — with zero
intended behavior change (only HALLWAY remains selectable, `z0 = 1.0`).

**Implementation guidance:**
- `src/mods/player_perspective/mod.rs`: widen `PerspParams` to
  `{mode, k, z0, skew}` per the design; add `PerspConstants` and
  `compute_constants(...)`; `latched_params` keys off `mode != PERSP_OVERHEAD`
  (HALLWAY is still the only latchable non-overhead mode this step — clamps stay
  `[0, 1]`).
- `src/mods/player_perspective/pass_rewrite.rs`: notes pre-callback and spot detour
  call `compute_constants` and emit `c49 = {d_min, z0, 0, 0}` (today's emission
  leaves c49.y at 0.0 — after this step it must always be the latched z0, never 0;
  this is the invariant that lets Step 3's recompiled VS land safely).
- `src/mods/playfield_styling/guideline_hook.rs`: `PerspLine` carries a
  `PerspConstants` built via the same function; the record transform applies the
  generalized map from the design's §Components 3.
- No enum, config, or shader changes in this step.

**Tests:** Build gates. Behavior-preservation review: for HALLWAY inputs,
`compute_constants` must reproduce today's constant values exactly (desk-check the
table row against the pre-refactor code paths in all three consumers). Live
regression is covered by Step 4's matrix.

**Integration:** Pure internal refactor beneath the existing hooks; every later step
builds on `compute_constants`.

**Demo:** Clean build of a DLL in which OVERHEAD/HALLWAY flow through the
generalized pipeline, desk-checked constant-equivalent to the shipped mod.

## Step 2: Add the three presets end-to-end (enum, config, constants, assets)

**Objective:** DISTANT/INCOMING/SPACE selectable, persisted, latched, and feeding the
render path. INCOMING's constants are final with the existing shader blobs; DISTANT/
SPACE constants are final but render un-zoomed (`c49.y` ignored by the old VS) until
Step 3 — irrelevant here since nothing deploys before Step 4.

**Implementation guidance:**
- `mod.rs`: `PERSP_DISTANT/INCOMING/SPACE` constants; widen both clamps to
  `[PERSP_OVERHEAD, PERSP_SPACE]`; three `EnumValue::with_preview` entries; latch
  resolves per-preset tunables into `PerspParams`; cull-window contribution condition
  becomes "any side latched HALLWAY **or INCOMING**" (DISTANT/SPACE contribute
  nothing).
- `compute_constants`: implement the full per-preset table from the design's §Data
  Models (mid-field anchor, flipped dir, `distant_focal`/`distant_zoom`, skew lerp
  of cx toward 640).
- `src/mods/config.rs`: `distant_focal` (3000.0), `distant_zoom` (0.9),
  `skew_strength` (1.0) with serde defaults, latch-time clamps per the design's
  §Error Handling, and the containment-formula comment.
- `scripts/gen_option_labels.py`: three `RIBBONS` entries + three WIDE `Preview`
  entries (copy per design §Components 6); regenerate; **commit net-new PNGs only,
  `git restore` the encoder churn** on pre-existing labels.

**Tests:** Build gates. Desk-check the four preset rows of `compute_constants`
against the design table (spot-check the derived properties: DISTANT entrance-edge
scale ≈ 1.004 at defaults, receptor scale ≈ 0.82; INCOMING at `skew_strength = 0`
degenerates to HALLWAY; doubles cx lerp is a no-op). Visually inspect the six
generated PNGs locally (chips legible at 132×24; preview copy fits the WIDE panel).

**Integration:** Fills in the preset table and UI on top of Step 1's pipeline;
DISTANT/SPACE consume the same path Step 3 completes.

**Demo:** Clean build with the full five-value row wired end-to-end; generated
label/preview assets viewable in the repo.

## Step 3: Base-zoom VS extension + blob recompile (DISTANT/SPACE containment)

**Objective:** The one-instruction `s *= c49.y` VS change, completing the DISTANT/
SPACE math per R4.

**Implementation guidance:**
- Edit `vs_persp_main` in both `shaders/src/gs_screencommand_arrow.hlsl` and
  `shaders/src/gs_screencommand_default.hlsl` (apply z0 after the clamp, before
  position reconstruction and `w = 1/s`; default copy keeps `o3 = c23`).
- Rebuild via `scripts/build_shaders.sh` (fxc 9.29 under the CrossOver bottle);
  recommit the two perspective `.d3dbc` blobs under `data_mods/shader_fixes/blobs/`.
  No `shader_synthesis.rs` changes — the fingerprint cache regenerates containers
  at next boot.

**Tests:** Build gates + a clean shader build. Sanity-check the compiled blobs
(disassembly or size delta shows exactly the one added multiply; both entry points
still compile against vs_3_0 with c48/c49 as the only new-range constants).

**Integration:** Completes the DISTANT/SPACE path opened in Step 2; last code
change. The full artifact set (DLL + blobs + assets) is now ready to deploy as one
unit — Step 1's z0-emission invariant guarantees the new VS never sees a zero scale.

**Demo:** The complete, buildable, deploy-ready feature: DLL, recompiled blobs, and
generated assets, all consistent with the design's constant contract.

## Step 4: Full cabinet verification matrix, default tuning, and docs

**Objective:** First deploy of the feature; run the design's entire verification
matrix (including the OVERHEAD/HALLWAY regression rows deferred from Steps 1–3),
tune the three new defaults live, and land the documentation.

**Implementation guidance:**
- Deploy DLL + blobs + assets together (standard `deploy.sh`); confirm in boot logs
  that shader synthesis regenerated from the new blob fingerprint.
- Work through the design §Testing Strategy table in full — regression rows first
  (OVERHEAD zero-footprint, HALLWAY pixel-identical under the recompiled blobs),
  then the new-preset rows (INCOMING/DISTANT/SPACE geometry, containment, skew
  spill + `skew_strength = 0` fallback, reverse, doubles degeneracy, entrance-edge
  continuity, missed-note behavior, persistence round-trip, mixed sides). Record
  each row's outcome in `progress.md`'s Deploy & test log.
- Tune `distant_focal` / `distant_zoom` / `skew_strength` by eye (as `hallway_focal`
  was tuned); commit revised defaults in `config.rs` if they move.
- Docs: update the `player_perspective` row in AGENTS.md (new presets, new tunables,
  skew-spill known limitation); note the INCOMING/SPACE filter-band spill and the
  doubles degeneracy wherever the hallway carve-outs are documented
  (`docs/playfield_styling_research.md` / `docs/custom_arrow_renderer_research.md`
  as appropriate); write `summary.md` for this planning directory.

**Tests:** The matrix itself is this step's test suite; final build gates on any
tuning/docs commits.

**Integration:** Closes the feature: verified behavior, tuned defaults, docs
matching reality.

**Demo:** A signed-off verification matrix and a cabinet where any player can pick
any of the five perspectives per side and get the SM-family behavior.
