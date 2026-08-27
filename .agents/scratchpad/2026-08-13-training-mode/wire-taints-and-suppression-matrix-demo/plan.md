# Plan: wire training + assist-tick taints (Step 5, task-02)

Status: Approved 2026-08-14 (verified upstream approval — same chain as
task-01; auto mode)

## Implementation approach

Producers only — the enforcement path (`custom_options_persistence`) and
`score_guard`'s election/sanitisation logic are untouched.

1. `src/mods/training_mode/bounds.rs`:
   - import `score_guard`;
   - loop-latch site in `try_resolve_row_bounds`: `set_training_taint(side)`
     beside the `SESSION_ACTIVE` store (covers both digest paths — the latch
     precedes the coherence gate);
   - row-engagement site (`a_ms > 0 || b_ms > 0`): `set_training_taint(side)`;
   - `set_marker` gains a `side: usize` parameter (threaded from
     `on_input_event`, which already computes it); taint beside each arm's
     `SESSION_ACTIVE` store. Triple-5 clear: no taint change (clearing does
     not untaint; the set already tainted).
2. `src/mods/training_mode/mod.rs`:
   - struct field `reset_cb: Option<usize>`;
   - enable(): register `song_reset::on_song_reset(|t_ms| ...)` — body:
     `t_ms > 0` ⇒ `taint_entered_sides()` (a module fn: taint exactly the
     sides `stage_records::side_entered` reports `Some(true)`; none reported
     ⇒ taint both conservatively). Panic-free: atomics + guarded reads only.
   - disable(): `song_reset::remove_callback` (assist_tick's handle pattern);
   - module doc: retire the "score containment arrives in Step 5" NOTE.
3. `src/mods/assist_tick.rs`:
   - import `score_guard`;
   - GAMEPLAY-entry latch loop: `set_assist_tick_taint(side, enabled)` for
     both sides (level semantics — true AND false every song);
   - disable(): `set_assist_tick_taint(side, false)` beside the latch zeroing
     (staleness prevention — see context.md decision);
   - module doc: score-suppression paragraph.
4. `README.md` assist-tick row: score-suppression sentence (matching the
   autoplay row's phrasing style).

## Test strategy

No new host tests (requirement 5 — the producers are engine-facing; the taint
state machine itself is task-01's host-tested surface). Validation:
- Full harness suite stays green (255).
- `cargo check` x86_64 target + fmt + `./build.sh`.
- The Step-5 cabinet + server suppression-matrix demo (maintainer-run; AC 1–5)
  closes plan Step 5 — demo script recorded in the feature progress.md.

## Risks

- Frame-thread subscriber: kept to guarded reads + atomic stores; no locks,
  no allocation, no engine calls.
- Stale assist-tick taint on mod disable: addressed (level-write false).
- Cross-mod callback ordering: by design not a risk — training producers fire
  frames after the scene change; assist-tick taint is never touched by
  `reset_song_taint` (task-01's contract).
