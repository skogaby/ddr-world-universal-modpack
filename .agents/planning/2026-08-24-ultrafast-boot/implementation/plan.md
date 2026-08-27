# Ultrafast Boot — Implementation Plan

Status: Approved 2026-08-24
Design: `.agents/planning/2026-08-24-ultrafast-boot/design/detailed-design.md` (Approved 2026-08-24)

## Checklist

- [x] Step 1: Module restructure + pure layers (cache format, replay arithmetic, boot plan)
- [x] Step 2: Signature derivations
- [x] Step 3: Loader pacing raise + measurement
- [x] Step 4: Analyze dispatcher service + NTX migration
- [x] Step 5: Capture, identity, and cache writing (capture-only boots)
- [x] Step 6: Temporary parity diff
- [x] Step 7: Replay path (cache-hit boots)
- [x] Step 8: Mutation drills, cleanup, and documentation

---

Step 1: Module restructure + pure layers (cache format, replay arithmetic, boot plan)

- **Objective:** Convert `src/mods/fast_bootup.rs` into `src/mods/fast_bootup/`
  (`mod.rs` carries the existing behavior verbatim — hook, gates, batch loop
  unchanged) and land the three pure, host-testable layers: `cache.rs` (bin
  format per design §Data Models), `replay.rs::compute_slot` (the write-set
  arithmetic), `plan.rs::compute` (boot-plan invariants).
- **Guidance:** No game interaction in any new file yet; `compute_slot` and
  `plan::compute` take plain structs so fixtures need no unsafe. Keep the
  existing mod public surface identical so `lib.rs` registration is untouched.
- **Tests (same step):** the full host-test matrix from design §Testing
  Strategy for cache round-trip/truncation/version, `compute_slot` fixtures
  (flags truth table, u16 accumulate skip-zero, double truncation, zeroed
  payloads), and `plan::compute` invariants (final-item-stock, shared-record
  flip eligibility, split files, `entry_index <= 0`, absent entries).
- **Integration:** Pure additions beneath the existing mod; behavior
  identical.
- **Demo:** `cargo test` green; `cargo check`/`./build.sh` clean; a deployed
  build boots exactly as today.

Step 2: Signature derivations

- **Objective:** Resolve the four new addresses (`step_data_release`,
  `find_music_by_mcode`, `music_db_global`, `variable_bpm_threshold`) in
  `core/signatures.rs` by instruction-decoding the already-resolved onUpdate
  body, per the design's derivation table.
- **Guidance:** Use the existing scanner primitives; validate each decode
  (target inside module, expected instruction shape) and log resolved
  offsets. Statically verify the same derivation logic against both Ghidra
  builds (20260721 + 20260616) before deploying. These are soft requirements:
  resolution failure must only latch the cache/pacing feature off (design
  §Error Handling), never fail the mod.
- **Tests:** Derivation helpers that are pure over instruction bytes get unit
  tests with byte fixtures lifted from both builds where practical; otherwise
  validation is the boot log.
- **Integration:** Consumed by nothing yet — log-only.
- **Demo:** Cabinet boot log shows all four `[+]` lines with offsets matching
  the Ghidra referents.

Step 3: Loader pacing raise + measurement

- **Objective:** Raise `mgr+0x70` to 64 on the first hooked onUpdate call and
  restore it at completion and in `disable()` (FR-6). Add one-shot INFO
  timestamps (first item processed / last item processed / items+files
  counts) to quantify the pass.
- **Guidance:** The manager pointer comes from the existing
  `step_data_global_table`; guard the write behind pointer validation and
  make restore idempotent.
- **Tests:** None host-side (two dword writes); the measurement is the test.
- **Integration:** First consumer of the boot-lifecycle edges (first-call /
  completion detection) that Step 7 reuses.
- **Demo:** Cache-less cabinet boot with cap 4 vs cap 64 logged: SSQ window
  duration comparison (expected ~2× or better at 60 Hz), no
  regression in stability. This measurement also decides Appendix B's
  bounded drain (expected: not needed).

Step 4: Analyze dispatcher service + NTX migration

- **Objective:** Create `services/analyze_hook.rs` owning the single Analyze
  detour with post-subscriber dispatch; migrate NoteTypesExpansion's mine
  injection to a subscriber; move the Analyze signature ownership
  accordingly.
- **Guidance:** Mirror `judge_hook`'s registration/catch_unwind shape. NTX's
  callback body moves verbatim (it already runs post-original). Init order in
  `lib.rs`: service before NTX registration. Keep NTX's install-in-`init()`
  semantics (detour lives in the service's init).
- **Tests:** Host tests for the subscriber registry (slot limits, dispatch
  order); NTX behavior is cabinet-validated.
- **Integration:** No fast_bootup consumer yet; NTX is the proving load.
- **Demo:** Cabinet boot + play a mines song: NTX boot-time log lines
  (injection + chunk-less resets) and in-song mine behavior identical to
  pre-refactor; exactly one detour on Analyze.

Step 5: Capture, identity, and cache writing (capture-only boots)

- **Objective:** Land `capture.rs` (boot-gated Analyze subscriber + store,
  keyed by the onUpdate loop's per-item stash), `identity.rs` (resolution +
  background verifier thread at enable), and the completion-time writer
  thread (merge, stat, serialize, tmp+rename). Replay stays off.
- **Guidance:** Boot gate = between first hooked call and completion AND
  `IN_HOOK`. Never stat or write on the game thread. On boots with an
  existing bin, log verdict statistics ("would hit X/Y files") without acting
  on them.
- **Tests (same step):** identity resolution pure part (mod-folder override
  precedence, absent classification); merge semantics (fresh-wins,
  absent↔present transitions). Serialization already covered by Step 1.
- **Integration:** Uses Step 2's nothing (capture needs no new derivations),
  Step 3's lifecycle edges, Step 4's dispatcher.
- **Demo:** First cabinet boot writes `data_mods/_cache/step_data/v1.bin`
  (~1.3 MiB, entry count == library size, absent entries for chartless
  customs); second boot logs a ~100 % would-hit rate with stock behavior
  otherwise unchanged.

Step 6: Temporary parity diff

- **Objective:** Implement the D6-override validation: on a boot with a cache
  present, process everything stock but diff each fresh capture against the
  cached payload, logging any `path/difficulty/mode/field` mismatch plus an
  end-of-pass summary. Clearly mark the code as temporary (removed in
  Step 8).
- **Guidance:** Reuses Step 5's capture store and Step 1's cache load; the
  diff is a pure comparison — trivial code, high confidence.
- **Tests:** One host test over the diff comparator (mismatch field naming).
- **Integration:** Sits beside the would-hit logging from Step 5.
- **Demo:** Cabinet boot reports **zero mismatches** across the full library
  (two consecutive analyze passes are deterministic and the cache carries
  them faithfully). This is the gate for Step 7.

Step 7: Replay path (cache-hit boots)

- **Objective:** The payoff: boot-plan construction on first hooked call,
  record flips (1→6, eligible records only), per-item replay
  (`compute_slot` + unsafe appliers: music-DB writes, actor accumulators,
  release via `step_data_release`, cursor advance, percent), last-item-stock
  completion, uncapped hit-loop with the stock path (and its existing gates)
  handling misses and the final item.
- **Guidance:** Follow design §Components and Interfaces / §Data Models
  exactly; every replay-path failure follows the design's Error Handling
  table (fail-open per item or per session). FR-3 check: the replay code path
  must contain no call to the game's error reporter — enforce by review and
  by the plan invariant tests from Step 1.
- **Tests (same step):** host tests for the applier-facing glue that is pure
  (slot-index math, accumulator fold, sota/thr8 filename gate); cabinet does
  the rest.
- **Integration:** Consumes everything: Step 1 plan/arithmetic, Step 2
  derivations, Step 3 lifecycle + cap, Step 5 identity verdicts and cache.
- **Demo:** Cached cabinet boot: SSQ window collapses (log shows ssq opens
  only for the final file ± the pre-batch in-flight few), boot-to-title time
  measured (~13 s expected on the reference cabinet), wheel BPM / EX score /
  shock icons / radar spot-checked identical to a stock boot, percent reaches
  100, a song plays normally, and the post-boot log shows the manager reached
  its stock end-state (no stuck entries, completion line present).

Step 8: Mutation drills, cleanup, and documentation

- **Objective:** Field-harden and close out: run the design §Testing Strategy
  mutation drills; remove the Step 6 diff code; update docs.
- **Guidance:** Drills — touch one SSQ (only it re-analyzes; cache refreshes),
  add/remove a LayeredFS override for one chart, delete the bin (full
  rebuild), corrupt the header (WARN + rebuild), toggle the mod off (stock
  slow boot). Documentation: rewrite the `fast_bootup` module docs for the
  new architecture, update the AGENTS.md key-entry-points row and README
  operator notes (cache location, delete-to-rebuild), and refresh
  `docs/ultrafast_boot_research.md`'s cross-version table if new builds were
  validated.
- **Tests:** No new functionality; full `cargo test` + readiness gates
  (`cargo check` → `cargo fmt` → `./build.sh`) as final validation.
- **Integration:** Final state — feature complete, temporary code gone.
- **Demo:** Each drill behaves per the Error Handling table on the cabinet;
  the repo docs describe the shipped behavior.
