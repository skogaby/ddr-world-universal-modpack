# Plan: Assist Tick Volume Option (code-assist run)

Status: Complete 2026-08-12 — no commit hash recorded: the maintainer directed the agent
never to commit; all changes were handed off in the working trees of both repos and
cabinet-validated end-to-end. Canonical record: `../progress.md`.

Status: Approved 2026-08-12 (stands on the session-approved PDD plan
`.agents/planning/20260812-assist-tick-volume/implementation/plan.md` and design; the
maintainer waived code-task-generator and directed running code-assist directly).

This document maps the PDD plan's steps onto concrete edits and their verification.
Specifications live in the design document — not restated here.

## Test scenarios (verification per step)

Per the approved plan, per-step verification is the build gates; the behavioral test
scenarios (design Testing Strategy items 1–7: visibility, scroll semantics, 25/100/175
audibility, chosen side, next-song latch, persistence round-trips, fail-open) are
consolidated into the maintainer's single Step 6 cabinet pass. Those scenarios were
written before any code (in the approved design) and constitute this task's
tests-before-implementation record.

Per-step checks the agent runs:

- Step 1: `python3 scripts/gen_option_labels.py` exits 0; exactly two new PNGs appear
  (`seop_item_assist_tick_volume.png` 176×16, `seop_image_assist_tick_volume.png`
  368×172); no other generated file changes; visual spot-check of both PNGs.
- Step 2: `cargo check` → `cargo fmt` → `./build.sh` clean. Code review against R1, R2,
  R6, R7, R8 (registration shape, latch ordering before rebuild arm, clear() default,
  Duplicate reseed, WARN paths).
- Step 3: gates + `./scripts/validate_se_bank_synth.sh` passes (existing paths
  untouched). Code review against R3, R4, R5 (identity shortcut, saturation math,
  chosen-side value threading).
- Step 4 (bemani-buddy): `cargo build`, `cargo test`, `cargo clippy --workspace
  --all-targets`, `cargo fmt`; codegen diff limited to the new field; migration numbered
  after 012; `.sqlx` regeneration if a local DB is available (escalate if not).
- Step 5: gates; proofread docs against shipped constants (25–175/5/10/100).

## Implementation order

1. `scripts/gen_option_labels.py` entries → run → verify PNGs.
2. `src/mods/assist_tick.rs`: constants → statics → normalize/on_change → SongState field
   → scene latch → rebuild_for read + log → registration block → disable resets.
   (Anchor/synthesis threading deferred to step 3 so step 2 builds standalone: the field
   is latched + logged but unconsumed.)
3. `se_bank_synth::scale_pcm` (+ re-export) → `Action::Anchor` + `spawn_synthesis`
   threading → synthesis log field.
4. bemani-buddy: model JSON → codegen → migration → db model/queries → handler → gates →
   `.sqlx` (DB permitting).
5. README + AGENTS.md.

No commits — the maintainer commits each step themselves (explicit directive).
