# Progress: driver-and-silent-skip-first-start (Step 3, task-03)

Updated: 2026-08-13
Status: Complete (uncommitted — maintainer handles git; code-complete and
gates-green; the ADJUST itself is cabinet-validated by the Step-3 demo per
the task's testing note)

## Checklist

- [x] Explore (perform_seek anatomy, anchor field +0x160, ms_to_blocks grid pin → WALL-domain ms confirmed, scene ids 25/26/28, entered-side source PlayerWork+0x4)
- [x] Step 1: `perform_adjust(dps, actors, plan)` factored out of `perform_seek`
  (everything except `reset_side_state`; 0x1043 stays in the shared core —
  research §5.4's tail is "0x1043 + 0x1044", proven by the shipped reset,
  and it keeps perform_seek byte-identical). FULL gates re-run after the
  extraction alone: 229/229 + check clean.
- [x] Step 2: `seek::blocks_to_wall_ms` (round-half-up inverse of
  `quantize_seek.t_q_ms` — round-trip property test + explicit 8 kHz values
  + degenerate-grid refusals) and `section_math::pre_shift_wall_ms`
  (content·100/percent; identity/50 %/175 %/zero/defensive tests).
  229 → 232.
- [x] Step 3: `song_reset::adjust_run_to(t_q_ms, lead_wall_ms)` — the
  seek-identical gate set (available/scene/DPS step 7/non-course/actors
  step 4/end-cascade clamp incl. `t_q < min_end − SEEK_END_MARGIN_MS`),
  snapshot-free (no accumulator/gauge block — the run just started), ends
  in `notify_subscribers(t_q)`. Plus `first_anchored_frame()` (live DPS
  step 7 + all actors step 4 + anchor `+0x160` ≠ 0; the DPS-step term keeps
  the one-shot out of the 6→7 transition frame) + `GPA_ANCHOR_OFFSET`.
- [x] Step 4: pre-shift arming — `refresh_pre_shift()` in
  training_mode/mod.rs (entered side via new `stage_records::side_entered`
  accessor: exactly-one ⇒ that side; both ⇒ clear (versus — ineligible);
  unavailable ⇒ P1-preferring nonzero-skip side), effective audio clamp
  composed, wall conversion at the DESIRED percent, `TRAINING_LEAD_MS`
  lead; refresh points = skip-row on_change + scene 25/26 entries (via
  bounds' scene callback) + enable seed; skip 0 ⇒ `(0,0)`; disable already
  cleared it (Step-1 code).
- [x] Step 5: `driver.rs` — render-thread self-requeueing, generation
  bumped at GAMEPLAY entry AND exit, armed only when work exists (pending
  row resolution OR mapping shift > 0 — zero rows/no shift ⇒ never arms,
  req 5's zero footprint); per frame retries `try_resolve_row_bounds`
  (the task-02 resolution + its b_ms INFO — AC 5), then on the first
  anchored frame fires the ONE-shot adjust: mapping read-back
  (`active_content_mapping`) → `blocks_to_wall_ms` → `content_ms` → t_q —
  the desired-vs-committed rate epsilon never reaches the anchor; 60 s
  soft-timeout WARN. Fallback ladder: no binding ⇒ WARN + song plays from
  0 (a fallback seek would need the same binding); unshifted binding /
  refused adjust ⇒ WARN + `request_reset(row_a, TRAINING_LEAD_MS, Zero,
  None)`; that refusing ⇒ WARN + plays from 0.
- [x] Step 6: full gates — harness **232 passed / 0 failed**, `cargo
  check` clean (no warnings), `cargo fmt` (whole crate), `./build.sh`
  clean → `target/x86_64-pc-windows-msvc/release/ddr_world_hook.dll`
  (build log: logs/build.log).

## TDD cycles

1. Refactor-only leg: extraction → full gates (behavior-identity pinned by
   the whole existing suite; no semantic change).
2. Pure helpers: tests + impl → 232/232.
3. Engine-facing (adjust/driver/arming): compile-clean via cargo check; the
   behavior legs are the cabinet demo's.

## Deviations

- 0x1043 (pre-start arm) included in the adjust block (design §4.3's text
  names only 0x1044; research §5.4 line 350 names the pair, and the shared
  core keeps `perform_seek` byte-identical). Recorded as design-conformant
  per the research.
- "Session ineligible ⇒ clear the pre-shift" (req 1) is implemented as:
  versus (both sides entered) clears at arm time; other ineligibility
  (course/event) is NOT duplicated mod-side (Step-1's standing-request
  eligibility model — the classifier owns the predicate); an unconsumed
  sticky mapping is inert (no binding ever reads it) and the driver's
  no-binding WARN covers diagnosis.
- `stage_records::side_entered` added as a shared accessor (quick_logout's
  private `PLAYER_WORK_ENTERED_OFFSET` read left untouched — shipped,
  cabinet-validated code; consolidation is optional future cleanup).
- The driver also retries the row-bound resolution when NO pre-shift exists
  (OMIT LAST alone) — required for AC 5's b_ms log visibility; still
  zero-footprint at zero rows.

Status: Complete (uncommitted — maintainer commits; no hash by repo convention)
