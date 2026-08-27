# Plan: LOOP SONG row + LOOP OFF early natural end

Status: Approved (verified upstream approval — see context.md; auto mode
per the sop's Step-1 rule.)

## Test scenarios (host)

1. `apply_action_transition_table` (section_math tests) — the pure
   apply-state machine over `{policy} × {thresholds_written}`:
   - (WriteThresholds{b}, _) ⇒ Write{b} (idempotent rewrite on a new b)
   - (Natural, written) ⇒ Restore
   - (Natural, !written) ⇒ Nothing
   - (ArmLoop, !written) ⇒ Nothing (LOOP ON never writes — AC 4)
   - (ArmLoop, written) ⇒ Restore (defensive, unreachable — documented)

The latch/predicate composition and the engine-facing write/stash/restore
are cabinet-validated by task-03's step demo (task text). Zero regression:
the full pre-existing suite stays green.

## Implementation shape

1. **section_math**: `ApplyAction {Write{b_ms}, Restore, Nothing}` +
   `apply_action(policy: EndPolicy, thresholds_written: bool)`; tests.
2. **bounds.rs**:
   - `ROW_LOOP_SONG: [AtomicBool; 2]` + `set_row_loop_song`/
     `row_loop_song`; `LOOP_LATCHED: AtomicBool` (per song) +
     `loop_latched()` (task-03's driver gate).
   - Per-song apply state: `THRESHOLDS_WRITTEN: AtomicBool`,
     `STASH_DISPLAY`/`STASH_RAW: AtomicI32` + `STASH_VALID: AtomicBool`,
     `END_APPLY_WARNED: AtomicBool`; all cleared in
     `clear_session_state` (which first restores the stash when written
     and the scene is still GAMEPLAY).
   - `try_resolve_row_bounds`: latch the entered side's loop value (BOTH
     digest paths — the loop row is not song-scoped); loop-ON sets
     `SESSION_ACTIVE`; call `apply_end_policy()` after the latches.
   - `apply_end_policy()`: policy = `end_policy(loop_latched, B_MS)`;
     dispatch on `apply_action(policy, written)` — Write: stash-once via
     `chart_end_thresholds` then `display_for_raw(decoded_notes)` +
     `set_chart_end_thresholds` (any failure ⇒ WARN-once, nothing
     written); Restore: `set_chart_end_thresholds(stash)` + clear
     written; Nothing: return.
   - Call sites: resolution completion, `set_marker('B')` tail, the
     triple-5 restore tail.
   - `on_scene_change` GAMEPLAY entry: `rows_engaged` gains
     `|| row_loop_song(side)`.
3. **mod.rs**: `OPT_LOOP_SONG`; `on_loop_song_change` → atomic; register
   in `register_bound_rows` beside the bound rows (Session, default OFF);
   registry re-seed on the enable path; availability true/false in
   enable/disable; disable resets the atomics. NOT touched by
   `seed_rows_for_highlight` / digest.
4. **Textures**: `scripts/gen_option_labels.py` — OPTIONS entry
   ("LOOP SONG") + two WIDE previews (off/on copy); run the script.
5. Gates: harness → check → fmt.

## Risks

- Latch on the stale-digest early-return path is easy to miss — the loop
  row deliberately survives song switches; covered by explicit code on
  both paths.
- The apply must never fire under ArmLoop (one-way cascade) — enforced by
  the pure table + task-01's policy exclusivity.
