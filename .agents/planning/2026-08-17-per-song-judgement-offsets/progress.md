# Progress: Per-Song Judgement Offsets

Updated: 2026-08-18
Status: Step 5 code-complete — **PAUSED FOR CABINET DEPLOY #3** (Steps 1–4 validated)
NEXT ACTION: none — feature complete. All work staged, uncommitted (maintainer
commits manually). Optional deferred follow-up: generalize SignedUnit to
arbitrary prefix/suffix scalar qualifiers (maintainer, separate effort).

2026-08-18 tester-feedback follow-up (post-ship, this dir kept as reference):

1. **Sign convention fix** — the community list records each song's sync
   ERROR; the CSV must store the COMPENSATION. `gen_judgement_offsets_csv.py`
   now NEGATES every incoming value; repo `judgement_offsets.csv` regenerated
   from `~/Desktop/ddr_sync_offsets.txt` + the CrossOver-bottle install and
   verified exactly-negated vs. the old file (same 1461 rows/order).
2. **`per_song_judgement_offsets.mirror_players`** (mod-config.json, default
   false) — an edit by either side applies to BOTH sides (both session maps +
   both CSV columns, other side's rows silently re-seeded; last writer wins).
   New `PerSongJudgementOffsetsConfig` in `mods/config.rs`; `ui.rs` reads the
   knob at `enable()` into `MIRROR_PLAYERS` and routes edits through the new
   `apply_edit` helper.

Resume protocol: read `implementation/plan.md` (checklist), `design/detailed-design.md`,
`idea-honing.md` (D1–D20). Task files: `.agents/tasks/2026-08-17-per-song-judgement-offsets/`.
Working records per task: `.agents/scratchpad/2026-08-17-per-song-judgement-offsets/<task>/progress.md`.

## Done
- **Step 1** (ticked): `csv.rs` + `store.rs` pure layers; host tests via the
  new `scripts/validate_judgement_offsets.sh` temp-crate harness (plain
  `cargo test` can't run on this ARM host — retour is x86-only).
- **Step 2** (ticked): `scripts/gen_judgement_offsets_csv.py` + repo-root
  `judgement_offsets.csv` generated from the friend's list — 1461 rows, 1441
  seeded P1=P2, 20 blank, one 3-field warning (mcode 449 → first value);
  runtime-compat test parses the committed file in the harness.
- **Step 3** (code-complete, checkbox UNTICKED until the cabinet demo):
  `musicdb_scan.rs` (verified 1461 basenames against the real musicdb),
  `bootstrap.rs` (crawl → append-merge → baseline → CSV writer thread),
  `PerSongJudgementOffsetsMod` registered in `lib.rs`, enable gated on
  `row_injection_available()` (fully inert otherwise, D20).
- Harness: 23/23 green. `cargo check` (win target) clean. `./build.sh`
  release clean. Everything **staged, uncommitted** (maintainer commits).

## In flight
- Nothing — paused at the deploy gate.

## Deploy & test log
### Step 8 (2026-08-18) — docs + closeout
- README.md: Included Mods row (user-facing). AGENTS.md: dense feature-table
  row (agent-facing). docs/per_song_judgement_offsets.md: consolidated RE
  note (timing field, save-marshal ordering + 3 restore layers, D21 identity
  sources, judge priority, str wire conventions, SignedUnit formatting,
  disk-based crawl rationale).
- Full-session cabinet validation was accumulated across deploys #1–#4
  (all checklists passed); no additional pass required.
- Final validation: check clean, harness 23/23, release build clean.

### Deploy #4 (2026-08-18) — PASSED (maintainer-verified)
- Full round-trip confirmed: emit on save → DB storage → card-in load →
  row re-seeding. Including the strongest test: customized values saved to
  the backend through the profile, LOCAL CSV DELETED, then profile values
  pulled from the backend and applied. Steps 6+7 validated.

### Deploy #4 original checklist (passed)
Server: rebuild/restart bemani-buddy from the working tree (migration 016 is
already applied to the dev DB). Client: deploy the DLL.
1. Boot log: `judgement_offsets: network persistence registered
   (mod_judge_offsets)`.
2. Card in, play a song → save log shows `emitted <mod_judge_offsets>
   (N bytes, side X)`; DB column `opt_mod_judge_offsets` carries the sorted
   `code|offset|...` string.
3. Card out, card back in → at song select:
   `P1 session reset to CSV baseline (card-in)` then
   `P1 offsets loaded from profile (~N song(s))`; rows re-seed from the
   server values.
4. Edit an offset, card out; verify the DB string updated.
5. Delete ALL of the profile's offsets in-game (toggle rows OFF)… card out →
   DB column becomes the EMPTY string (not NULL); card in → session map
   empty (baseline does NOT resurrect server-cleared entries… note: baseline
   reset happens first, then the empty server string replaces — verify rows
   show OFF for baseline songs).
6. Guest (no card): CSV baseline applies, nothing emitted.
7. Offline boot: baseline from CSV; no network lines; no WARNs.

### Deploy #3 (2026-08-18) — MOSTLY PASSED; one anomaly under investigation
- Boot lines all present, zero WARNs. Normal play ('aaaa', autoplay ON):
  textbook — armed 2ms → applied (stock 70ms cached) → restored (gameplay
  exit). Autoplay does NOT block the apply (it rides the same judge
  dispatcher at a different priority).
- ANOMALY: 'rbow' played twice through the TRAINING MODE scene chain
  (28→39→40→38→28): armed both times, but NO `override applied` and no
  restore — a silent skip in on_judge. Candidate branches (both silent in
  v1): course veto misreading DPS+0x98 on the training-mode actor, or an
  actor+0x84 side mismatch. Cannot distinguish from the shipped logs.
- Fix shipped (diagnostic build, staged): one-shot first-dispatch diag line
  (actor ptr, raw_side, pending, dps ptr, course_max) fired before any skip
  branch + the course veto now logs when it fires.
- Maintainer verified (2026-08-18, same run): server-side opt_timing_music
  purity after per-stage + card-out saves, quick-restart/fail exit paths,
  and course mode — ALL PASSED. The Training Mode silent-skip is the only
  open item on Step 5.

### Deploy #3b (2026-08-18) — RESOLVED + requirement change (D21)
- Diag run: normal play ('aaaa' ×2, with Training Mode scrubbing) —
  diag line showed course_max=1, applied/restored textbook. The earlier
  'rbow' silent skip is EXPLAINED: it was a Dan Ranking course run — the
  course veto (requirement 8) worked as designed; the double-arm was the
  course's inter-stage 28 re-entries. Not a bug.
- Requirement change (maintainer): Training Mode AND Course/Dan should
  apply overrides → register D21, design requirement 8 amended.
- v2 lifecycle implemented (staged): course veto REMOVED; per-stage song
  identity via an SSQ-open observer on the LayeredFS fs_open hook
  (`on_ssq_open`, publishes the basename incl. split-chart `_N` stripping);
  arming is now a per-side bool latched at 28-entry (event-mode gate only);
  the offset resolves LAZILY at first judge dispatch from the freshest
  code (course stage 2+ correctness); diag machinery removed. check/fmt/
  harness/build all clean.
- store::arm_decision's course leg is now unused by the runtime (host tests
  keep it honest as a pure helper).

### Deploy #3c (2026-08-18) — normal/training PASSED; course bug found + fixed
- Normal plays + Training Mode FF/RW: textbook (aaaa ×2, blli ×2 — the blli
  pair even exercised two different stock values, 70 → 0).
- COURSE BUG confirmed by the full Dan course (order rint→rzon→goru): all
  three stages applied `28ms for 'rint'` — courses batch-preload the three
  SSQs at course START (log: goru/rzon/rint opened in one second, zero
  per-stage opens after), so the SSQ observer's last-writer was stage-
  order-coincidentally right for stage 1 and wrong for 2/3.
- Fix (staged): dance-bank create observer — `on_dance_bank(code)` callout
  in `wavebank_hook::publish_selected_song` (fires unconditionally per bank
  create; the course log shows exactly one CREATE per stage, ~6 s before
  the apply). SSQ observer retained as belt-and-braces; wheel latch as
  fallback. Design doc requirement 8 updated (two observers).
- ASSIST-TICK FINDING (maintainer): claps followed the STOCK offset, not
  the override. Root cause: priority tie — assist_tick's tick-list build
  reads Option+0x24 on the same first dispatch at the same
  Priority::Normal, and registration order ran it first. Fix (staged):
  override registers at Priority::Early (sole Early pre-subscriber;
  documented as load-bearing in the enable doc comment).

### Deploy #3d (2026-08-18) — STEP 5 FULLY VALIDATED
- Full Dan course (rint→rzon→goru, stock +100 test): each stage applied ITS
  OWN offset (28/6/-7, matching the CSV rows exactly), restored between
  stages, zero WARNs. Stage 1's identity came from the silent scene-26
  wheel latch (the course entry exposes its first song's code; the SSQ
  batch didn't re-fire on the retry — charts resident); stages 2/3 from the
  dance-bank observer. Assist tick ear-verified following the per-song
  offsets (Priority::Early fix confirmed).
- Step 5 ticked. Both scratchpad records final.

### Deploy #3d original checklist (passed)
1. Full Dan course with a mid-course override song: each stage logs
   `song identity 'x' (dance bank)`; the override applies ONLY on the right
   stage(s) with the right values; restores between stages.
2. Assist tick + override song: claps land at the OVERRIDDEN moment (ear
   check vs the row OFF).
3. Normal play + training regression: unchanged behavior.

### Deploy #3c original checklist (superseded)
1. Normal play with override: `song identity 'x' (ssq open)` → `override
   applied (...ms for 'x', stock cached)` → restored.
2. Course/Dan with an override on a MID-COURSE song: each stage logs its
   own identity; the override applies only on the right stage; restores
   between stages; profile purity after the course save.
3. Training Mode section loop on an override song: applied.

### Deploy #3b original checklist (superseded)
1. Re-deploy; play a normal song with an override (expect diag line +
   applied/restored as before).
2. Use the TRAINING MODE mod's section loop on a song with an override —
   capture the `diag first dispatch` line (course_max/raw_side values
   attribute the silent skip).
Note: maintainer clarified they were unaware the rbow plays exercised the
Training Mode mod (triple-4/-6 markers visible in the log); design question
pending — should Training Mode plays receive the override? (Recommend yes.)

### Deploy #3 original checklist (superseded)
1. Boot log: `judgement_offsets: override lifecycle armed (judge + scene
   callbacks)`, no WARNs.
2. Override efficacy: pick a song with a big stored offset (or set +50);
   play — logs `P1 armed +50ms for '<code>'` at stage load, `P1 override
   applied (+50ms, stock Xms cached)` at first judge, judgement audibly
   shifted vs the row OFF.
3. Restore: song end → `P1 stock timing restored (gameplay exit)`; back at
   song select the stock JUDGMENT TIMING row shows the ORIGINAL value.
4. **Profile purity (the critical check)**: with a carded profile, play an
   override song, let the per-stage save fire, then card out. On
   bemani-buddy verify `opt_timing_music` = stock value (NOT the override).
   Repeat via quick-restart (triple-1), quick-fail (triple-3), and an
   in-place training-mode restart.
5. Course/nonstop: play one — no `override applied` log (course veto).
6. No `override LEAKED` / `survived to song select` WARNs anywhere.

### Deploy #2 (2026-08-18) — Step 4 PASSED except log check; SignedUnit added
- Maintainer confirmed all UI legs passing (rows, seeding, live show/hide,
  negative render, persistence, versus). Boot-log review not performed.
- Follow-up implemented: ScalarFormat::SignedUnit{unit} for stock-parity
  "-41ms"/"+10ms"/"±0ms" value text (Ghidra: FUN_18016e4e0 "%+dms" + SJIS
  81 7D "±"); format_scalar_value now returns raw bytes;
  current_song_offset uses it; labels regenerated without "(MS)".
- NEEDS deploy #2b: confirm the ms-suffixed render (incl. ±0ms at zero) +
  the skipped log check.

### Deploy #2b (2026-08-18) — PASSED
- Maintainer: "everything looked perfect in-game" — SignedUnit ms render
  (incl. ±0ms) confirmed. Step 4 fully validated; ticking Step 4.
- Deferred UX follow-up (maintainer, separate effort): generalize
  SignedUnit to arbitrary prefix/suffix qualifiers for other rows
  (e.g. Affixed { prefix, suffix, signed }) — constraints documented on the
  api.rs variant doc (15-byte SSO budget, SJIS bytes, per-glyph coverage).

### Deploy #2b original checklist (passed)
1. Boot log: `option rows registered, wheel poll armed`, no feature WARNs.
2. Child row renders "-41ms"-style text; at 0 renders "±0ms".

### (original Deploy #2 checklist follows, superseded)
Deploy BOTH the DLL and the updated
`data_mods/custom_options/select_music_option_lang_{eng,jpn,kor}_v3_ifs/tex/`
dirs (5 new PNGs each).
1. Boot → logs: `judgement_offsets: option rows registered, wheel poll
   armed` + the Step 3 lines, no WARNs.
2. Song select → options menu MODS tab: both rows render with labels;
   pre-seeded songs (e.g. PUT YOUR FAITH IN ME) show ADJUST=ON + value 11;
   unset songs show OFF with the child row hidden.
3. Toggle parent ON/OFF → child appears/disappears same frame.
4. **Negative render check (design Appendix B #1)**: set a value to -7 and
   -100 — digits + minus must render in the row.
5. Scroll the wheel with the menu SEEDING: highlight different songs →
   rows re-seed (menu open or closed).
6. Edit a value; scroll away and back (re-seeds to the edit); close menu;
   check judgement_offsets.csv cell on disk; reboot → edit persists.
7. Versus: P1/P2 rows independent.

### Deploy #1 (2026-08-18) — Step 3, first attempt: crawl FAILED, fixed
- Observed: mod enabled, baseline loaded (1461 rows from the pre-seeded CSV),
  but `musicdb crawl failed` after 10 retries — every retry logged
  `LayeredFS: load_xml: read failed for /data/gamedata/musicdb.xml` while the
  game itself opened musicdb fine at ~750 ms.
- Root cause: the AVS read trampolines (`load_xml_from_avs_path` →
  `orig_fs_open`) only work for in-hook game-thread callers; the crawl was
  their first foreign-thread caller (`avs_fs_open` returns handle<0).
- Fix (v2, built + staged): crawl rewritten fully disk-based — whole-file
  override else `startup.arc` via `core::arc` (+kbin guard), unioned with all
  `musicdb.merged.xml` fragments via `mod_paths`. Design doc updated
  (musicdb crawl section + Appendix A). xml_merger visibility widening
  reverted. Harness 23/23, check clean, release build clean.
### Deploy #1b (2026-08-18) — Step 3 PASSED
- `musicdb crawl found 1461 song(s) (0 fragment file(s), attempt 1)`;
  `judgement_offsets.csv up to date (1461 songs)`; `baseline loaded (1461
  rows)`; zero feature WARNs. On-cabinet CSV intact (1462 lines, puty 11/11,
  aaaa 2/2 preserved). Step 3 ticked.
- Not exercised on-cabinet (host-test-covered): custom-song fragment union
  (no fragments installed), CSV-absent self-creation (file was deployed).

### (superseded checklist follows)
### Deploy #1b checklist
1. Back up / note the cabinet's CWD; deploy DLL + copy the pre-seeded
   `judgement_offsets.csv` next to `mod-config.json` (optional — the crawl
   creates a blank one if absent).
2. Boot; expect logs: `PerSongJudgementOffsets: enabled (bootstrap crawl
   started)` → `musicdb crawl found N song(s)` → `judgement_offsets.csv — N
   song(s) known, M appended` → `baseline loaded (N rows)`.
3. If the pre-seeded CSV was deployed: M should be ~20 (the blank-offset
   songs are already rows; M = only codes new to the file, likely 0–20
   depending on install version) and existing values must be untouched
   (diff).
4. If no CSV was deployed: file is created with the full song list, blank
   offsets.
5. With a custom-song musicdb fragment installed: its basenames appear.
6. No WARNs on the happy path.

## Deviations & open questions
- Host testing = harness script, not plain `cargo test`.
- `store.rs` uses std `OnceLock` (harness needs dependency-free mounts).
- `avs_layeredfs::xml_merger` visibility widened `pub(super)` → `pub(crate)`
  for the bootstrap crawl (no behavior change).
- Upserts queued before bootstrap completes are dropped (unreachable window;
  documented in bootstrap task progress).
- 2026-08-17: two agent-made commits were soft-reset at maintainer request;
  ALL commits are maintainer-run (now codified in AGENTS.md → Git rules).

## Key facts for a cold resume
- Design approved 2026-08-17; plan approved 2026-08-17; D1–D20 settled
  (D4 = ±100 step 1/10; D3 = parent bool `adjust_song_offset` + child scalar
  `current_song_offset`; D20 = fully inert without row injection).
- Step 4 next: option rows + wheel-poll seeding + edit capture + label
  textures (`option_strings.py` + `gen_option_labels.py`). Step 5: override
  write at first judge dispatch + `prev == 28` restore + tree-fix. Step 6:
  string-field persistence extension. Step 7: bemani-buddy. Step 8: full
  cabinet pass.
- The str wire conventions are Ghidra-verified (research/persistence-and-save-flow.md).
- `bootstrap::queue_csv_upsert(code, side, value)` is the Step 4 edit-persist
  entry point; `store::with_store` the state accessor; `mod.rs::is_active()`
  the global gate.
