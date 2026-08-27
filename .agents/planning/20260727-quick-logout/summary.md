# Quick Logout — PDD Summary

Planning completed 2026-07-27. All gates passed: register settled (26 decisions,
none open), `Readiness Confirmed 2026-07-27` (idea-honing.md), design
**Approved 2026-07-27**, implementation plan **Approved 2026-07-27**.

**Implementation completed 2026-07-28** (Steps 1–4 build-gated, Step 5's single
cabinet validation pass reported clean by the maintainer — assumption A1
CONFIRMED, neither FR4 WARN fired, profile round-trip verified; R3 answered:
the gesture fires inside the options modal, accepted as-is). Feature status:
**done**. Outcomes folded into `docs/quick_logout_research.md` §13; operator
docs in README ("Quick Logout" + "Sanitised logout saves" rows) and the two
AGENTS.md entry-point rows.

## What this feature is

1. **Quick Logout mod** (`quick-logout`): triple-press numpad **9** on either
   pinpad during music selection → the game's own end-of-session tail runs
   immediately (TOTAL RESULTS → e-amusement logout save → THANK YOU → attract).
   No confirmation, no UI, no config. One new AOB (`agcs::Sequence::finish`),
   zero new detours.
2. **Logout-save sanitisation** (policy change): a session tainted by
   Autoplay/Quick-Fail no longer has its whole card-out save suppressed —
   instead the score content is stripped (per-stage records + course record
   virginised at scene-34 entry; `<league>` node removed in the save hook) and
   the profile/customize write-back is forwarded. Applies to quick *and*
   natural logouts; fails closed back to full suppression.

## Artifacts

| File | Content |
|---|---|
| `rough-idea.md` | The ask + what "logout" means in this binary |
| `idea-honing.md` | 26-decision register (D1–D26) with rationale + research outcomes table |
| `research/orientation.md` | Repo blind-spot pass; what existing infra covers |
| `research/mechanism-verification.md` | R1/R2/R6/R7 Ghidra verification + re-entrancy analysis |
| `research/savekind3-marshal.md` | The logout marshal teardown — skip predicate, course record, league leak, sanitiser timing |
| `design/detailed-design.md` | **The implementation source of truth** (self-contained; Approved) |
| `implementation/plan.md` | 5-step plan, checklist format (Approved; deferred-validation convention) |
| `progress.md` | Live resume point — keep updated per step |

## Design in one paragraph

The trigger arms a one-shot scene redirect `30 → 32` (0-indexed) and calls
`sequence_finish(active_child, 30₁ᵢₙdₑₓ)` from the input callback — routing
through the 0-idx 29 loader (the only loader that makes TOTAL RESULTS'
`scene_result` package resident) into the summary, then the game's own
`getNextID` chain drives 33 → 34 (credit expire + `savekind == 3` save) → 35 →
attract. Four gates (scene 25, side entered, child alive, fired latch); tail
diagnostics WARN if scene 34 is skipped or exits < 500 ms. The sanitiser rides
a scene-change callback on 34: for tainted sides it writes `mcode = -1` into
the 5 record slots + course record (layout decoded from `stage_record_accessor`
via the new shared `stage_records` service), marks the side sanitised, and the
`save_sender` trampoline then strips `<league>` (libavs Ordinal 164) and
forwards — or suppresses entirely if any piece failed to arm.

## Plan shape

Steps 1–4 are build-gate-only (no cabinet deploys); Step 5 is one consolidated
cabinet validation pass in risk order, then docs + research fold-back.

1. Transition plumbing (signature, scene constants 32–35, `redirect_repair_available()`)
2. `stage_records` service + `premium_free` refactor onto it
3. `quick_logout` mod (trigger, gates, FIRED latch, tail diagnostics)
4. Sanitised logout saves (`score_guard` rename + flags, scene-34 sanitiser, Ordinal 164 league strip, 3-way save policy)
5. Cabinet validation (A1 first), AGENTS.md/README, `docs/quick_logout_research.md` fold-back

## Assumptions / open items to watch

- **A1 (the one real unknown):** a forced `EAmExitRootSequence` performs the
  logout save — ark-side behaviour, statically supported, verified only at
  Step 5. The FR4 diagnostics make failure loud. If it fails: stop, diagnose,
  return to design (do not work around).
- R3 (does the gesture fire inside the options modal) — answered at Step 5.
- Under Premium Free the TOTAL RESULTS screen will be empty — accepted cosmetic.
- Never close the shutter before triggering (soft-locks TOTAL RESULTS' exit).

## Next steps

None — implemented, validated, documented. Future-work item recorded in the
research doc (§13.2 / Appendix B of the design): Mechanism C, "make this my
last stage" (`GameWork+0x10 = GameWork+0xC`), a zero-risk vanilla-path
complement that was descoped.
