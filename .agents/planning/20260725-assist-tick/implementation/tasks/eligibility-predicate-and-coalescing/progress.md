# Progress — Step 4 Task 01: eligibility predicate + coalescing

- [x] `should_tick` — transcription of the research reference implementation, returning
      `Result<i32, RejectReason>` so each rejection is counted exactly once; `length[]` /
      `FREEZE ARROW: OFF` reasoning in the doc block
- [x] `COALESCE_MS = 4` named constant, commented provisional pending Step 6's TPS-150 measurement
- [x] `build_tick_list` reworked: predicate → sort → single retain-based coalescing pass (exact
      duplicates are the zero-distance case, folded into `coalesced`); returns `(BuildStats, times)`
- [x] Once-per-song line extended: `results= kept= rej_kind= rej_shock= rej_panel= rej_neg=
      coalesced= first=[…]`; module header's "Step 3 status" paragraph updated
- [x] Per-frame path untouched (diff confirms: only the build + logging changed)
- [x] Gates: `cargo check` 0, `cargo fmt` clean, `./build.sh` 0; installed (sha256 match)

## Verification record (Ace out, Challenge — the Step 3 reference chart)

```
AssistTick: song build -- side=0 results=438 kept=340 rej_kind=7 rej_shock=91 rej_panel=0 rej_neg=0 coalesced=0 first=[8888, ...]
```

- **AC6 reconciliation exact:** 340 + 7 + 91 + 0 + 0 + 0 = 438; kept dropped from Step 3's 437.
- **Corroborated by the game itself:** kept(340) + shocks(91) = 431 = the max-combo denominator
  the results screen showed for this chart — the chart is a shock chart, the 7 kind-rejects are
  its freeze tails/markers, and the classification matches the game's own combo accounting.
- Tick deltas unchanged (≈ −148 ±6 with the 150 ms operator offset) — clock untouched.
- AC1–AC5 (jumps/freezes/shocks/mines/CUT/FREEZE-OFF listening rows): maintainer's §7.4 matrix,
  log-assisted by the rejection counts; the shock row is already log-corroborated above.

No deviations. Commit deliberately not made (maintainer owns commits).

Status: Complete
