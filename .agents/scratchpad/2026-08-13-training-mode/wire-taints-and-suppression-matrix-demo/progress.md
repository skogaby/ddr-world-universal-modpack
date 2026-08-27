# Progress: wire training + assist-tick taints (Step 5, task-02)

Updated: 2026-08-14
Status: Complete (uncommitted — the maintainer handles all git). Cabinet
demo Session A FULL PASS 2026-08-14 17:44–17:56 (log- and
server-verified; plan Step 5 ticked). Session B regression legs
(shipped Autoplay/quick-fail/rate policy) deferred by the maintainer to
end-of-feature regression testing.

## Checklist

- [x] Setup + Explore (context.md: producer sites, subscriber API,
      side_entered, assist_tick latch/disable, trigger_restart rationale)
- [x] Plan (plan.md)
- [x] Producer 1: `bounds::try_resolve_row_bounds` — taint at BOTH
      `SESSION_ACTIVE` latch sites (loop latch pre-digest-gate + row
      engagement), entered side
- [x] Producer 2: `bounds::set_marker(which, side)` — side threaded from
      `on_input_event`; taint beside each arm's latch (triple-5 clear
      deliberately taints nothing — clearing never untaints)
- [x] Producer 3: `song_reset::on_song_reset` subscriber in
      `training_mode/mod.rs` enable()/disable() (`reset_cb` handle,
      assist_tick's pattern); body panic-free/allocation-light
- [x] Producer 4: assist_tick GAMEPLAY-entry level-write (both sides, true
      AND false every song) + disable() level-write false (staleness guard)
- [x] Docs: assist_tick module doc (Score suppression section), README
      assist-tick row + the Sanitised-logout-saves policy row (consistency),
      training_mode module doc (Step-5 paragraph replaces the "arrives in
      Step 5" NOTE)
- [x] No enforcement-path changes (`custom_options_persistence`,
      score_guard election/sanitisation untouched — verified by diff)
- [x] Gates: harness **255/255** → `cargo check` x86_64 clean → `cargo fmt`
      → `./build.sh` → release DLL at
      `target/x86_64-pc-windows-msvc/release/ddr_world_hook.dll`
      (logs/test.log, check.log, build.log)
- [x] Step-5 cabinet + server demo (maintainer-run 2026-08-14; log-verified
      leg map in the feature progress.md Deploy & test log) — AC 1–4 pass;
      AC 5 (shipped-source regression) deferred to end-of-feature testing
      by maintainer decision

## Deviations

- **Subscriber predicate widened from the task text's `t_ms > 0` to
  `t_ms > 0 || bounds::training_session_active()`** — required by the
  approved design, found in the Validate review: a triple-1 quick restart
  during a B-engaged (LOOP OFF early-end) song wipes the training taint at
  the trigger (`trigger_restart` → `reset_song_taint()`), restarts at t=0
  (no A marker ⇒ the task's subscriber would not re-taint), yet the
  truncated end thresholds PERSIST across the in-place reset — the replay
  still ends early at B and its partial score would have submitted,
  violating design R5 and this task's own AC 1 ("bound rows alone … none
  submit"). Design §4.1's session-active predicate ("latched per song") is
  the design's stated taint condition, so gating the t=0 re-taint on
  `training_session_active()` implements the design rather than departing
  from it; AC 3's observable contract is preserved (untouched song ⇒ latch
  false ⇒ the replay stays clean and submits). Conservative direction:
  fail-closed on score integrity. Also covers the same laundering via a
  post-triple-5 restart (session was active earlier in the song ⇒ taint).
- **assist_tick disable() also level-writes the taint false (both sides)**
  — the producer's scene callback disappears on disable; without this a
  taint from the last clapped song would go stale and suppress an honest
  later song. Natural completion of level semantics (recorded in
  context.md), not a behavior contradiction.
- **README Sanitised-logout-saves policy row updated too** (names the two
  new taint sources) — one sentence, consistency with the assist-tick row
  update the task required.
- **Commit deferred to the maintainer** (repo AGENTS.md Workflow + the
  Step-5 handoff instruction). Changed files: `src/mods/training_mode/{mod,
  bounds}.rs`, `src/mods/assist_tick.rs`, `README.md` (+ task-01's two
  score_guard files).

## Step-5 demo handoff (maintainer-run)

Deploy the fresh release DLL. All legs on a carded-in session with Autoplay
OFF (its own taint would mask the new sources). Expected log line per
suppressed save: `score_guard: ... savekind=2 save SUPPRESSED`; server-side
verification (bemani-buddy) is the maintainer's.

1. **Assist tick alone**: ASSIST TICK ON, play an ordinary song clean → no
   per-stage score server-side; card-out save sanitised (profile persists).
   Next song with it OFF submits normally (AC 4).
2. **Bounds alone (LOOP OFF partial results)**: SONG END TIME below the
   song length, LOOP OFF → early natural end + partial results → suppressed.
3. **Marker gesture alone**: untouched rows, mid-song triple-4 (or -6) →
   suppressed.
4. **Restart-from-A**: set A, triple-1 → the replay (and the session) stays
   suppressed (the trigger's taint wipe is re-covered by the subscriber).
5. **LOOP ON grind**: any grind → suppressed.
6. **Clean song still submits** (AC 2): same session, untouched song, all
   training rows default, assist tick OFF → score submits normally.
7. **Honest replay** (AC 3): untouched song, triple-1 mid-song, finish the
   replay → submits.
8. **Regression** (AC 5): one Autoplay-ON song and one quick-fail —
   suppression lines unchanged from shipped behavior.
9. Card-out after the tainted legs: logout save sanitised, not suppressed
   (profile/options persist server-side, scores stripped).
