# Quick Logout — Progress

Updated: 2026-07-28
Status: **DONE** — all 5 steps complete; cabinet-validated; docs folded back

**NEXT ACTION:** none. (If anything regresses, start from
`docs/quick_logout_research.md` §13 and `design/detailed-design.md` §4/§6.)

Resume protocol: read `implementation/plan.md` (checklist + steps), then
`design/detailed-design.md` (Approved 2026-07-27; §4 has every interface, §7 the
cabinet checklist). Decision history in `idea-honing.md`; RE evidence in
`research/`.

## Done

- PDD Steps 1–6 complete: workspace, orientation, 26-decision register (all
  settled), Ghidra + backend research (R1/R2/R5–R9), approved design.
- Key user overrides to remember: bare triple-9 trigger (no confirm/UI/config);
  summary path only; **sanitise-don't-suppress** tainted logout saves (D21–D26).
- **Step 1 (2026-07-28):** `sequence_finish` signature added to the batch
  `SIGNATURES` array in `core/signatures.rs` (auto-registered by
  `resolve_all`); scene names map + `scene::` constants 32/33/34/35 (incl. the
  34 `EAM_EXIT` display name) + the 29/30 naming comment in `types/scenes.rs`;
  `redirect_repair_available()` (AtomicBool latched at `advance_to_scene` hook
  install) in `services/scene_manager.rs`. Gates clean: cargo check → fmt →
  ./build.sh. Expected Step-5 log line: `[+] sequence_finish` in the boot
  resolve list at an offset near `+0x21DB90`/`+0x21DF70`.
- **Step 2 (2026-07-28):** new `src/services/stage_records.rs` — full §4.3
  decode (game-work global +3, table +16, course field +23 disp8, course
  record +36 imm32, stride +47, base +55) with the §4.3 validation ranges,
  fail-closed `AVAILABLE` latch, accessors (`game_work`/`player_work`/
  `stage_record`/`course_record`/`course_field_offset` + `record_base`/
  `record_stride` for logging). Registered in `services/mod.rs`; `init` wired
  in `lib.rs` as step 4h2, immediately before `custom_options_persistence`.
  `premium_free` refactored onto it: local decode block + 5 layout statics
  removed; INC-patch checks + stage-counter disp8 decode stay local;
  fail-closed outcome preserved (`stage_records` unavailable ⇒ init false);
  "armed" + virginise log lines byte-identical. Gates clean. Expected Step-5
  log lines: `stage_records: layout decoded (records work+0x590, stride 0x2B8,
  course rec +0x2D8, course field +0x70)` and the unchanged PremiumFree
  virginise line during the regression check.
- **Step 3 (2026-07-28):** new `src/mods/quick_logout.rs` (registered in
  `mods/mod.rs` + `lib.rs` mod block after quick_restart_or_fail). Gesture:
  per-side triple-9 GestureBuffer (1.5 s window, clone of quick_restart's);
  non-SONG_SELECT presses clear that side's buffer. Four gates: FIRED latch,
  entered-side session gate via `stage_records::player_work` (+0x4 byte;
  degrades to pass with one-time WARN when stage_records down), live TS from
  `current_transition_sequence()`, live child (`*(TS+0x58)` non-null,
  `flags@+0x20 & 0x24 == 0`). Trigger: context log (per-side entered +
  `score_guard::is_logout_suppressed` — old name, migrates in Step 4) →
  `add_redirect_once(30, 32)` → `finish(child, 30₁ᵢₙdₑₓ)` with no lock held
  across the call. Tail diagnostics while FIRED: per-transition `+ms` log,
  EAM_EXIT entry stamp, WARN at THANK_YOU if 34 never seen or dwell < 500 ms;
  FIRED + buffers + stamps reset on SONG_SELECT entry. Enable-gated on
  `scene_manager::is_available() && redirect_repair_available()` +
  input_manager. Gates clean. Expected Step-5 artifacts: trigger-context line,
  timed `25→29→32→33→34→35` tail lines, no WARN on a clean run.
- **Step 4 (2026-07-28):** `score_guard`: `is_logout_suppressed` renamed
  `logout_taint`; `SANITISED[2]` + `mark_logout_sanitised`/
  `was_logout_sanitised` added (cleared in `reset_session`); module docs
  rewritten for the sanitise-don't-suppress model.
  `custom_options_persistence`: Ordinal 164 (`property_node_remove`) resolved
  non-fatally → `LEAGUE_STRIP_AVAILABLE`; EAM_EXIT scene callback
  (`register_logout_sanitiser` + `sanitise_tainted_logout_records`) virginises
  the 5 array records + course record of each tainted side via `stage_records`
  and marks the side sanitised (any accessor failure ⇒ side stays
  un-sanitised ⇒ suppression); `save_sender_trampoline`'s `savekind == 3`
  branch is now the three-way policy (clean→forward /
  sanitised+164→`strip_league_node` on `<data><league>` after the original
  builds the tree→forward / else→pretend-success suppress); `savekind == 2`
  path untouched. `quick_logout` migrated to `logout_taint`. Gates clean.
  Expected Step-5 artifacts: `logout sanitiser: P{n} records virginised
  (tainted session)`, `score_guard: P{n} logout save SANITISED — scores
  stripped, profile forwarded` / `... SUPPRESSED (sanitiser unavailable)`,
  and the boot line `resolved libavs-win64 ordinals 162/163/175/176 (164
  league-strip: true)`.
- **Step 5 (2026-07-28):** single cabinet validation pass run by the
  maintainer — reported clean. **A1 CONFIRMED** (forced EAmExit performs the
  logout save; neither FR4 WARN fired); TOTAL RESULTS rendered and exited (no
  `scene_result` crash, no shutter hang); profile write-back round-trips.
  **R3 answered: the triple-9 gesture DOES fire while the song-select options
  modal is open** — accepted as-is (cosmetic). Docs: README rows ("Quick
  Logout" + "Sanitised logout saves" policy row, config example entry),
  AGENTS.md rows (Quick Logout + reworked score-submission policy row),
  `docs/quick_logout_research.md` §13 (validation outcomes + implemented
  deviations; banner replaced; §9.2 marked superseded), `summary.md` outcome
  header. `mod-config.json` already carried `"quick-logout": true` (added by
  the maintainer during testing).

## In flight

- Nothing — feature complete.

## Deploy & test log

- **2026-07-28** — feature-complete build (Steps 1–4) deployed and validated
  by the maintainer in one session: "everything looked good". Specifics
  captured in the Step 5 entry above (A1 confirmed, no FR4 WARNs, R3 = gesture
  fires in the options modal).

## Deviations & open questions

- **Step 3 micro-deviation:** `FIRED`/`TRIGGER_AT` are armed immediately
  *before* the `finish` call (design §4.7 listed them after) so the re-entrant
  scene hook logs the very first hop (25→29) as part of the FR4 tail. No
  behavioural difference otherwise — every failure path returns before either
  is set, and `finish` cannot fail.
- **A1 resolved (2026-07-28):** confirmed on the cabinet — the forced
  `EAmExitRootSequence` performs the logout save.
- **R3 resolved (2026-07-28):** the gesture fires inside the options modal;
  accepted (the modal is still music selection; the session ends normally).

## Key facts for a cold resume

- Trigger = `add_redirect_once(30, 32)` (0-idx) + `sequence_finish(child,
  30₁ᵢₙdₑₓ)` from the input callback; child = `*(TS + 0x58)`. Indexing footgun:
  `finish` is 1-indexed, everything else 0-indexed.
- Never close the shutter before triggering (TOTAL RESULTS' exit gate soft-locks).
- Sanitiser = `mcode = -1` into 5 array records + course record (`+0x2D8`,
  decode from `stage_record_accessor`+36) at scene-34 entry, tainted sides only;
  league strip = libavs Ordinal 164 (`property_node_remove`) on `<data><league>`;
  fail-closed fallback = today's full suppression.
- Verified addresses: `sequence_finish` @ `0x18021DF70` (20260721) /
  `0x18021DB90` (20260616), single match each.
