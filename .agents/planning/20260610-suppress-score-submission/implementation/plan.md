# Implementation Plan: Suppress Score Submission

Derived from `design/detailed-design.md`. Each step is an independently
`cargo check`-clean, deployable increment. **There is no unit-test harness** — per
CLAUDE.md, "tests" here means `cargo check --target x86_64-pc-windows-msvc` plus a
named cabinet deploy observation (log lines / behavior). The early steps follow the
learnings-doc discipline: **ship a diagnostic/observe build before trusting a
runtime claim** (savekind enum, busy-flag settling) rather than committing to the
final suppression behavior blind.

Build/check command for every step:
```
cargo check --target x86_64-pc-windows-msvc
```
Deploy for observation steps: `./scripts/deploy.sh` then read `[DDR-Hook]` log lines.

---

## Checklist

- [x] **Step 1** — `score_guard` service: state module + API (no callers yet)
- [x] **Step 2** — Wire `score_guard` readiness + session reset into `custom_options_persistence::init` / `load_receiver`
- [x] **Step 3** — Diagnostic: log `savekind` + `playside` on every `save_sender` (confirm enum, no behavior change) — confirmed stage=2, logout=3 on cabinet
- [x] **Step 4** — Trigger taint writers: `autoplay` (per-side) + `quick_restart_or_fail` (quick-fail + restart reset)
- [x] **Step 5** — Taint reset on gameplay entry (scene wiring)
- [x] **Step 6** — Suppression in the `save_sender` trampoline (per-stage, kind=2) — **first end-to-end suppression**
- [x] **Step 7** — Logout suppression (kind=3, session-sticky) — R8
- [x] **Step 8** — Autoplay fail-closed gate; confirm Quick-Fail fail-open
- [x] **Step 9** — Cabinet acceptance: PASSED. R-A/R-B/R-C resolved (research §6b). Latch-timing bug found + fixed + confirmed (autoplay-P1/honest-P2: only side 0 latched, P2 logout saved).
- [x] **Step 10** — Docs: README / AGENTS.md / `.agents/summary` updates + feature summary

> **Post-implementation note (latch fix):** the session-sticky logout flag now
> latches at actual per-stage suppression time (`score_guard::mark_session_tainted`,
> called from the trampoline), not when a trigger is armed — fixing a false-positive
> where merely toggling Autoplay in the menu would suppress an honest session's
> card-out save. See research §6b. **Confirmed on cabinet.**

### Additional fixes (pre-existing bugs, found during cabinet testing — both cabinet-verified)

- [x] **Row-lifetime crash** — detour `OptionForm::~OptionForm` (new `optionform_dtor`
  signature, cross-version-verified) to eagerly `clear_side` on options close, +
  empty-guards on the `+0xB8` writers. See `research/option-row-lifetime-crash.md`.
  Confirmed: crash no longer reproduces.
- [x] **2P option-load misrouting** — ddrcode join (`load` savedata+0x48 ↔
  `PlayerWork+0x18`), deferred to SONG_SELECT entry. See
  `research/2p-options-load-side.md`. Confirmed: each side loads correctly; network
  overrides JSON.

---

## Step 1: `score_guard` service — state module + API

**Objective.** Create `src/services/score_guard.rs` with the atomic state and full
public API from design §3.1/§4.1, and register it (`pub mod score_guard;` in
`src/services/mod.rs`). No callers yet — this is the standalone foundation.

**Guidance.**
- Atomics only (`AtomicBool`), no `Mutex`, no heap, no FFI types. Writers `Release`,
  readers `Acquire` (match `autoplay.rs`).
- Implement: `mark_hook_installed`, `is_available`, `set_autoplay_taint(side,on)`,
  `set_quick_fail`, `reset_song_taint`, `reset_session`, `is_stage_suppressed(side)`,
  `is_logout_suppressed(side)`.
- `set_autoplay_taint(_,true)` and `set_quick_fail()` also latch `SESSION_TAINTED`.
  `set_quick_fail` latches both sides; `reset_song_taint` clears only
  `QUICK_FAIL_TAINT`; `reset_session` clears `SESSION_TAINTED[*]`.
- Bounds-guard `side` (`if side < 2`) on every indexed access — no panics/indexing
  across the FFI-reachable read path (CLAUDE.md rule 1).
- Doc-comment each fn with its caller + intent (the module header should state the
  taint model so a future reader doesn't have to reconstruct it).

**Test / validation.** `cargo check` clean. (Pure logic; no runtime surface yet.)
Optionally add a `#[cfg(test)]`-free sanity by inspection — but per repo norms, the
real validation arrives when wired in later steps.

**Integration.** New leaf module under `services/`; nothing depends on it yet. No
behavior change.

**Demo.** `cargo check` passes with the new module present and exporting its API;
`grep` shows `score_guard` registered in `services/mod.rs`.

---

## Step 2: Readiness + session-reset wiring in `custom_options_persistence`

**Objective.** Make the guard's readiness reflect the real ess-hook install, and
reset session taint on card-in — without yet changing any save behavior.

**Guidance.**
- In `custom_options_persistence::init()`, after `resolve_and_hook_ess()` returns
  true (both detours enabled), call `score_guard::mark_hook_installed()`. Do NOT mark
  it on the early-return / failure paths.
- At the top of `load_receiver_trampoline`, call `score_guard::reset_session()`
  (card-in = new session boundary, R8). Keep it before the existing early-returns so
  it always runs on a load.
- Add a one-line `log_info!` when readiness is marked, for cabinet confirmation.

**Test / validation.** `cargo check` clean. Deploy: confirm log shows
`score_guard: save hook installed` exactly when `CustomOptionsPersistence started`
appears; confirm a card swipe still loads options normally (regression — no save
behavior touched).

**Integration.** `score_guard` now has its readiness driven by the real hook;
`autoplay` (Step 8) can later gate on it. No suppression yet.

**Demo.** On the cabinet, the readiness log line appears at boot; card-in/out and
custom-option round-trip behave exactly as before.

---

## Step 3: Diagnostic — log `savekind` + `playside` on every `save_sender`

**Objective.** Before any suppression, positively confirm (R-B) the `savekind` enum
values at `savedata+0x74` and correlate them with observed save moments. This is the
"diagnostic build before trusting a runtime claim" gate.

**Guidance.**
- In `save_sender_trampoline`, after deriving `savedata` and `side`, read
  `savekind = *(savedata + 0x74)` and emit a one-shot-ish `log_info!`:
  `score_guard-diag: save_sender side={} savekind={} (call N)`. (Throttle/counter to
  avoid log spam if needed.)
- No suppression, no behavior change — purely observational. Call the original as
  today.

**Test / validation.** `cargo check` clean. **Deploy + observe:**
- Play one song to a normal finish → note the savekind value at the per-stage save.
- Card out → note the savekind at the logout save.
- Expectation: Stage=2, Logout=3 (First=1 on initial card-in). Record the actual
  values in `research/score-submission-re.md` (update R-B). If they differ, the
  Step 6/7 `match` constants use the observed values.

**Integration.** Reads the field the suppression logic will branch on; proves the
mapping. Still byte-for-byte original save behavior.

**Demo.** Cabinet log shows savekind per save moment; the Stage/Logout enum values
are confirmed and recorded.

---

## Step 4: Trigger taint writers (`autoplay`, `quick_restart_or_fail`)

**Objective.** Make the two trigger mods feed `score_guard`, so taint state becomes
live (still no suppression — readers aren't consulted yet).

**Guidance.**
- `autoplay::autoplay_on_change(side, val)` → add
  `score_guard::set_autoplay_taint(side as usize, val != 0)`.
- `quick_restart_or_fail::trigger_fail()` → add `score_guard::set_quick_fail()`.
- `quick_restart_or_fail::trigger_restart()` → add `score_guard::reset_song_taint()`
  (honest replay must save, R4).
- Keep all calls panic-free; they're plain atomic stores.

**Test / validation.** `cargo check` clean. **Deploy + observe** (add temporary
debug logs in `score_guard` writers, or rely on the existing autoplay/quick-fail
`log_info!`): toggling autoplay logs `Autoplay: side=N ON/OFF`; triple-3 logs
quick-fail fired; triple-1 logs restart. Confirm taint writes happen at the right
moments (no suppression yet, so scores still upload — that's expected this step).

**Integration.** Writers now populate `score_guard`; the reader (trampoline) wiring
lands in Step 6. State is observable but inert.

**Demo.** Cabinet logs show taint being set/reset in lockstep with autoplay toggles
and the two gestures, with scores still saving (suppression not yet active).

---

## Step 5: Taint reset on gameplay entry

**Objective.** Reset per-song taint at the start of each gameplay so a fresh song is
clean (R4) — the reset half of the lifecycle.

**Guidance.**
- `quick_restart_or_fail` already registers a `scene_manager::on_scene_change`
  callback (clears gesture buffers when leaving GAMEPLAY). Extend it: on **entering**
  GAMEPLAY (`next == scene::GAMEPLAY`), call `score_guard::reset_song_taint()`.
- Rationale for placement: this mod already owns a gameplay-scene callback and is one
  of the two trigger mods; no new service callback needed.
- Confirm `reset_song_taint()` clears only `QUICK_FAIL_TAINT` (autoplay is live, so
  no reset needed for it; session-sticky must NOT be cleared here — only on
  card-in).

**Test / validation.** `cargo check` clean. **Deploy + observe:** triple-3 in a song
(quick-fail taint set), then start a new song → log confirms song taint cleared on
GAMEPLAY enter. Session-sticky remains set (will verify its effect in Step 7).

**Integration.** Completes the per-song taint lifecycle (set in Step 4, reset here).
Still no suppression — readers wired next.

**Demo.** Logs show song taint resetting at each gameplay entry while session-sticky
persists across songs.

---

## Step 6: Suppression in the trampoline (per-stage, kind=2) — first end-to-end

**Objective.** The first real suppression: per-stage (kind=2) saves are dropped for
a tainted side. This makes Autoplay/Quick-Fail actually stop the per-song score
upload — the core feature, demoable end to end.

**Guidance.**
- In `save_sender_trampoline`, before calling the original: compute
  `suppress = (savekind == STAGE) && score_guard::is_stage_suppressed(side)` (use the
  enum value confirmed in Step 3). Define `STAGE`/`LOGOUT`/`FIRST` consts.
- If `suppress`: `log_warn!("score_guard: side {} STAGE save SUPPRESSED (autoplay={},
  quick_fail={})", …)` and `return 1;` (pretend-success; do NOT call original; skip
  `<mod_*>` emission). Else: `log_debug!` allowed + unchanged original-call path.
- Suppression runs regardless of `persist_network`/`persist_json` (detour exists when
  either gate on). Keep the existing custom-option emission for the allowed path.
- **Do not yet handle logout** (kind=3) — that's Step 7. This step alone will leave
  the logout re-send u@nsuppressed (acceptable as an intermediate; flagged in demo).

**Test / validation.** `cargo check` clean. **Deploy + observe (R-A is the key
risk):**
- Autoplay ON (P1), play a song → log shows `P1 STAGE save SUPPRESSED`; confirm
  server did NOT receive P1's score; **confirm card-out still completes and a
  subsequent honest play saves** (R-A busy-flag settling). If card-out hangs or the
  next save fails, apply the R-A fallback (let original run on a neutered request, or
  clear the busy flag) and re-test.
- Honest play → `save allowed`, score persists (regression).

**Integration.** Wires the reader to the writers from Steps 4–5; the per-stage core
loop is complete. Logout still TODO.

**Demo.** On the cabinet: an autoplayed song's per-stage score does **not** reach the
server, an honest song's does, and the session continues normally (card-out works).

---

## Step 7: Logout suppression (kind=3, session-sticky) — R8

**Objective.** Suppress a side's card-out logout save if any stage that session was
tainted, closing the logout re-send hole (design R8).

**Guidance.**
- Extend the trampoline `match`: `LOGOUT => score_guard::is_logout_suppressed(side)`.
- Reuse the same suppress branch (log + `return 1`).
- `SESSION_TAINTED` was already latched by the Step-4 writers; `reset_session()`
  (Step 2) clears it on card-in. No new state.

**Test / validation.** `cargo check` clean. **Deploy + observe (R-C):**
- Session = {P1 honest song1, P1 autoplay song2, P1 honest song3} → per-stage: songs
  1 & 3 saved, song 2 suppressed; **card-out: P1 logout SUPPRESSED**; confirm server
  shows songs 1 & 3 (i.e. per-stage saves were authoritative — validates R-C). If
  server is missing songs 1 & 3 after logout suppression, logout was the authoritative
  commit → escalate to the per-stage surgical-drop fallback (hook
  `ReflectSavePlayerData` kind=3) noted in design §8.3 / R-C.
- Mixed session P1 tainted / P2 clean → P1 logout suppressed, P2 logout allowed.

**Integration.** Completes the suppression model (per-stage + logout). Session-sticky
lifecycle now exercised end to end (latch in Step 4, gate here, reset in Step 2).

**Demo.** A session containing any tainted song uploads only the clean songs'
per-stage scores; the tainted side's logout save is suppressed; a fully-clean
co-player is unaffected.

---

## Step 8: Autoplay fail-closed gate; confirm Quick-Fail fail-open

**Objective.** Enforce R6: Autoplay refuses to enable if the guard isn't available;
Quick-Fail keeps working regardless.

**Guidance.**
- In `autoplay::enable()`, at the top, `if !score_guard::is_available() { log_warn!
  ("Autoplay: score-submission guard unavailable — refusing to enable (fail-closed)");
  return; }` before registering judge callbacks / the custom option. Init order
  (lib.rs 4i before step 8) guarantees readiness is settled.
- Ensure `disable()` is still safe to call (no half-registered state) when enable
  bailed — take/None handles already guard this.
- `quick_restart_or_fail`: confirm NO readiness gate (fail-open). Its taint writes are
  already harmless no-ops when the trampoline is absent.
- Consider: should the autoplay custom-option *row* still render when fail-closed?
  Per R6 "Autoplay must not enable" → cleanest is to not register the option at all
  (no row), so the user can't toggle a non-functional autoplay. Document this in the
  enable() warning.

**Test / validation.** `cargo check` clean. **Deploy + observe:**
- Normal boot (hook installs) → Autoplay enables, option row present, suppression
  works (regression of Step 6).
- Simulate hook-unavailable (temporarily force `resolve_and_hook_ess` to fail, or
  point at a bogus ess string) → Autoplay logs refusal, no autoplay option row, and
  autoplay does nothing; Quick-Fail still fails out a song.
- Revert the simulation after the test.

**Integration.** Ties the readiness flag (Step 2) to the trigger mods' enable paths,
completing R6. Feature is functionally complete after this step.

**Demo.** With the save hook present, everything works; with it forced-absent,
Autoplay is inert (no row, no effect) while Quick-Fail still operates — the asymmetric
failure mode is observable.

---

## Step 9: Cabinet acceptance — full matrix + resolve R-A

**Objective.** Run the design's acceptance scenarios 1–6 and risk checks R-A..R-E on
the cabinet; lock in the R-A busy-flag resolution decided during Step 6.

**Guidance.** Execute and record results for:
1. Autoplay P1-only in P1+P2 → P1 suppressed, P2 allowed.
2. Quick-Fail in P1+P2 → both per-stage saves suppressed.
3. Honest play → allowed + custom-options round-trip regression.
4. Autoplay song 2 of 3 → songs 1,3 saved, song 2 + logout suppressed (R8 / R-C).
5. Hook-absent → Autoplay refuses; Quick-Fail operates (R6).
6. Quick restart then honest finish → saved (R-D).
Plus R-E doubles/versus side indexing.

If R-A required the fallback (neutered-request or busy-flag clear), confirm it's the
shipped path and the per-side state machine settles across many consecutive plays
and a full card session.

**Test / validation.** All six scenarios pass on cabinet; logs match expected
SUPPRESSED/allowed decisions; no card-out hang; server-side reflects only clean
scores. Update research/design risk tables with observed outcomes.

**Integration.** End-to-end validation of the whole feature on the live game.

**Demo.** A full play session demonstrating each scenario, with `[DDR-Hook]` logs and
server-side score state confirming correct suppression.

---

## Step 10: Documentation

**Objective.** Reflect the feature in user/agent docs and write the PDD summary.

**Guidance.**
- `README.md`: update the **Autoplay** and **Quick Restart / Fail** mod descriptions
  to note that scores are not submitted when autoplaying / quick-failing (integrity;
  not optional). Note Autoplay won't enable if the score hook is unavailable.
- `AGENTS.md` Custom Instructions / `.agents/summary/components.md`: add
  `services::score_guard` (shared taint state) and note the `save_sender` trampoline
  now also enforces score suppression.
- Write `.agents/planning/20260610-suppress-score-submission/summary.md`
  (artifacts, decisions, status, deviations).

**Test / validation.** Docs build/read cleanly; `cargo check` unaffected. No code
behavior change.

**Integration.** Closes the PDD loop; future agents find `score_guard` documented
where they'd look.

**Demo.** Updated README/AGENTS/summary describe the suppression behavior and the new
shared module.
