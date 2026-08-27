# Song Rate Streaming Redesign — Progress

Updated: 2026-08-11
Status: **FEATURE COMPLETE — all 7 plan steps ticked.** Deploy #5 (the
release matrix) PASSED maintainer-run 2026-08-11 and was log-verified
(design req 42 satisfied: 0-deferral streaming at 50 %/175 %, tick
alignment at rate, rate-aware Real Speed exact-math-confirmed, PUS CSV
cells byte-equal to the committed ratios, score containment intact, clean
soak). Docs are current and the README is de-historicized for
open-sourcing. Tree is UNCOMMITTED — maintainer commits personally.
NEXT ACTION (maintainer, administrative): commit the Steps 1–7 tree.
Nothing else remains on this feature.
Resume protocol: read `rough-idea.md` (the pivot constraints), `research/orientation.md`
(keep/remove verdicts), `docs/xact_streaming_research.md` (the RE foundation),
`research/streaming-mechanism.md` (design implications), `idea-honing.md` (register).
Predecessor feature: `.agents/planning/2026-08-05-song-playback-speed/` (its progress.md
holds the pivot record and all cabinet evidence; its design's policy/score/clock/option/
backend sections remain accurate KEEPERS).

## Done

- **Implementation Step 7 task-02 COMPLETE (2026-08-11, code-assist; working
  record: `implementation/step07-task-02-release-matrix-run-sheet-and-handoff/`):**
  the Deploy #5 release-matrix run sheet (10 legs incl. the Step-6 oracles:
  tick alignment at 50 %+100 %, Real Speed velocity both fix states, PUS
  CSV cells; re-confirmation legs marked) composed into this file's Deploy
  & test log; five gates re-run green on the FINAL tree (validator 172/172;
  se-bank passed; 0-warning windows check; fmt clean; release DLL
  8 550 912 B). Handoff: `scripts/deploy.sh`, maintainer-run. Plan Step 7
  deliberately UNTICKED until the matrix passes.
- **Implementation Step 7 task-01 COMPLETE (2026-08-11, code-assist; working
  record: `implementation/step07-task-01-documentation-rewrite-and-sweep/`;
  incl. a maintainer-directed follow-up):** shipped docs current with the
  streaming truth. README: feature row rewritten (streamed, no cache/no
  pause, preview at normal speed, Step-6 mentions), config example +
  cache section removed, Assist Tick capacity sentence (20 min wall /
  5 min chart at 25 %); **follow-up (maintainer, pre-open-sourcing): the
  README carries ZERO historical/retired-key language** — the retired
  config section, the `assist_tick.offset_ms` sentence, and the WebUI
  migration note are all gone (README greps clean for
  retired/earlier-builds/cache_limit_gib/offset_ms; AGENTS.md and the
  planning/docs records deliberately KEEP their historical context —
  agent-facing). AGENTS.md: Song Playback Speed row rewritten (streaming
  modules, two-region serving + preview passthrough, Q31-LAST,
  tick_domain/real_speed/csv_rate_cells, Config: NONE), Assist-tick row
  (1200 s D15 + rate-aware FR-3 via tick_domain), movie-hook instruction
  un-staled (live rate suppression). docs/xact_streaming_research.md §8
  (WSOLA ~2.4× realtime, preview passthrough rationale, parser-rule
  contract + fixture-honesty lesson). docs/song_playback_speed.md:
  supersession banner + §16 Rate-aware Real Speed (the durable
  actor-cluster RE). Design: dated preview-passthrough amendment notes at
  req 12 and the virtual-bank spec (original text intact). Sweep:
  mod-config.json clean; no stale 300 s / "identity-only Step 3"
  remnants. All five gates green (the follow-up postdates them and
  touches only the non-compiled README).
- code-task-generator run for plan Step 7 (breakdown approved by maintainer
  2026-08-11): `.agents/tasks/song-rate-streaming/step07/` —
  `task-01-documentation-rewrite-and-sweep.code-task.md` (README/AGENTS
  rows to the streaming truth, xact_streaming refresh, Real-Speed RE made
  durable in docs/song_playback_speed.md + supersession banner, dated
  design amendment notes for the preview passthrough, cache_limit_gib
  sweep, stale movie-hook instruction fix; Medium) and
  `task-02-release-matrix-run-sheet-and-handoff.code-task.md` (req-42 run
  sheet incl. the Step-6 oracles appended to this file's Deploy & test
  log, final gates, release DLL handoff — completes at handoff, plan
  Step 7 ticks only after the maintainer's matrix passes; Low). Settled at
  approval: design corrections as dated inline amendments; old research
  doc gets a banner + targeted fixes only; Real-Speed RE home =
  docs/song_playback_speed.md dated section; README covers Step-6 features
  as brief in-row mentions.
- **Implementation Step 6 task-03 COMPLETE (2026-08-11, code-assist; working
  record: `implementation/step06-task-03-pus-csv-rate-columns/`) — plan
  Step 6 checkbox TICKED (all three sibling records Complete):** PUS CSV
  rate columns (design req 34). `RateSnapshot::csv_rate_cells()`
  (clock_patch.rs, host-tested): committed non-identity ⇒
  `(requested_percent, "source/output")` — the committed exact GCD-reduced
  ratio as a fraction (exactness over a rounded decimal; recorded choice);
  else the uniform `(100, "1/1")`. `SongIdentity` gained
  `rate: RateSnapshot`, latched inside the EXISTING first-judgment
  song-identity snapshot (post-commit; never read at flush time — the
  publication resets at gameplay exit). Header + every row append the two
  cells (`Song Rate Requested (%)`, `Song Rate Effective`); the three
  pre-existing cells stay byte-identical (labels included — AC-2 governs
  over renaming to "chart ms"). Vectors: identity/committed-100/
  uncommitted-75 uniform; 50 % + 125 % literal fraction pins (the 125 %
  pin proves GCD reduction). All five gates green (validator 172/172 in
  7.39 s; se-bank passed; windows 0 warnings; fmt clean; DLL 45.56 s).
- **Implementation Step 6 task-02 COMPLETE (2026-08-11, code-assist; working
  record: `implementation/step06-task-02-real-speed-effective-rate/`):**
  rate-aware Real Speed (design req 33) — with a HEADLINE premise
  correction: `real_speed_fix.rs` holds no Rust derivation (it is byte
  patches into the native `SetScrollSpeed`), so the KEEPER design's
  raw-recompute mechanism was implemented instead, owned by the
  song_playback_speed mod (never the fix toggle). Fresh RE (Ghidra
  20260721 + 20260616 spot-check; record in the task context.md): Option
  speed-type@+0x8 / target@+0x14 / derived-multiplier@+0x10 / Core
  BPM@+0x88 confirmed; the GamePlayActor latches the multiplier at
  construction into `+0x290/+0x294/+0x29C` and re-writes the arrow/spot
  renderers from those floats EVERY frame — the actor cluster is the
  effective write target and the first judge dispatch the correct window.
  New pure host-mounted `song_rate/real_speed.rs`
  (`rate_adjusted_multiplier`: native-faithful trunc + clamp[25,800] over
  `core × source/output`; None at identity/uncommitted or untrusted inputs
  ⇒ no write ⇒ stock, fail-open) + windows glue (judge pre-subscriber with
  per-side once-per-song latches, GAMEPLAY-entry scene reset, guarded
  option-chain walk, fixed-multiplier sides untouched). 5 new host vectors
  (IEEE-f64 literal pins per rate incl. clamp interaction; purity/toggle
  independence). All five gates green (validator 171/171; se-bank passed;
  windows check 0 warnings; fmt clean; release DLL 45.96 s).
- **Implementation Step 6 task-01 COMPLETE (2026-08-11, code-assist; working
  record: `implementation/step06-task-01-assist-tick-rate-conversion/`):**
  Assist Tick is rate-correct (design req 30) — new pure host-mounted
  `song_rate/tick_domain.rs` (`tick_track_positions` + `restart_skip_ms`:
  identity/uncommitted = the LITERAL legacy arithmetic, bit-identity pinned
  over i32-extreme grids; committed non-identity =
  `clamp_i32(content_to_wall_ms(t + JT − m0) − sound_offset)` /
  `clamp_i32(content_to_wall_ms(mc − m0))` — JT converts with the content
  clock, sound_offset unscaled; c2w Err ⇒ identity fallback). The mod latches
  `clock_patch::snapshot()` into `SongState.rate` at the AwaitAnchor arm and
  threads it through Anchor/Commit/Rewind; the Step-4 scaffold gate is
  REMOVED (`Action::RateGated` + call site + log; zero references remain);
  `is_non_identity_commit()` KEPT as the conversion selector (doc + test
  retitled). `TICK_CAPACITY_MS` 300_000 → **1_200_000** (D15; segment
  ~28.9 MB, still lazy). Vectors: exact conversions at 25/50/75/125/175 via
  `target_for_percent` on a non-block-clean source against an independent
  i128 oracle + literal pins; skips incl. negative; clamp + degenerate-ratio
  fail-soft; proceeds-at-committed-rate. Validator file-presence +2; se-bank
  check 8 re-derived from the constant (now the 1200 s truncation boundary
  pin). All five gates green (validator 166/166 in 7.36 s; se-bank ALL
  CHECKS PASSED; windows check 0 warnings; fmt clean; release DLL 44.7 s).
- code-task-generator run for plan Step 6 (maintainer directed 2026-08-11):
  `.agents/tasks/song-rate-streaming/step06/` —
  `task-01-assist-tick-rate-conversion.code-task.md` (content→wall tick/skip
  conversion at the committed ratio + scaffold-gate removal +
  `TICK_CAPACITY_MS` 300→1200 s; 100% bit-identity pin; High),
  `task-02-real-speed-effective-rate.code-task.md` (Core BPM ×
  effective_rate independent of the fix toggle at non-identity; Medium),
  `task-03-pus-csv-rate-columns.code-task.md` (requested/effective rate CSV
  columns, per-song latch; Low). No inter-task dependencies; implement in
  order. All three consume only `clock_patch::snapshot()` — no
  streaming-engine changes.
- **Step-5 loading-stall fix implemented (2026-08-10, maintainer-approved design
  deviation; working record: `implementation/step05-fix-preview-side-buffer/`):**
  the two-region serving model. Root cause (log-diagnosed): the engine's bank
  prepare primes a stream context for EVERY wave incl. the never-played
  `<code>_s` preview entry at the virtual file's tail; the linear ring produced
  the whole main entry to reach it (armed-override sprint), then gameplay's
  first reads were behind-window → full regeneration (the "3 deferrals, max
  latency == production time" signature; main entry produced twice per song;
  loading = 23–25 s at 25%). Fix: the non-main entry is produced FIRST into a
  resident `side_buffer` (append-only watermark, 64 MiB cap → Plan refusal);
  the ring covers ONLY the main entry's range; regen targets are main-only.
  Plus the missing design-req-28 silence-fill WARN (drain watches the active
  binding's state, once per generation). Regression pin:
  `side_entry_prepare_read_completes_without_main_production` (both entry
  orders; red against the old model). All five gates green (validator 161/161);
  awaiting maintainer re-test of the 25% loading time.
- **Step-5 first live run sheet PASSED (2026-08-10, maintainer-run; logs
  log1.txt/log2.txt/log.txt in the CrossOver install):** boot readiness + row
  registration; 100% literal stock (save allowed); 50% end-to-end (pitch-correct,
  synced, stage save SUPPRESSED); 25%/175% benchmark — production ~21–22×
  realtime at every rate (e.g. 25%: ≈540 s of audio in 25.2 s); Quick Restart
  (same generation, ledger idempotent, 3 clean reclaims); logout sanitiser
  (records virginised ×2); assist-tick scaffold gate fired;
  `DDR_SONG_RATE_FAULT=mid-song-failure` run silent-but-judgeable. The
  ≥1×-realtime design gate PASSES. Finding: loading-screen stall (fixed above).

- **Implementation Step 4 task-04 COMPLETE (2026-08-10, code-assist; working
  record: `implementation/step04-task-04-io-callback-detours-and-readiness/`)
  — plan Step 4 checkbox TICKED (all four sibling records Complete):** the
  XACT IO-callback detour pair + the deliberate readiness flip. New
  `io_callback_hook.rs` (windows-only): `#[repr(C)]` OVERLAPPED mirror
  (Internal @0 = accumulator, offset union @16 = 64-bit read offset),
  pair-or-neither install with rollback from task-01's three derived
  addresses, detour bodies gated on ONE Acquire load of the active binding
  (null ⇒ trampoline) then the stock handle→file_id helper; outcome→ABI:
  Served(n≠0) ⇒ TRUE, Served(0) ⇒ FALSE (stock EOF leg), Pending ⇒ FALSE +
  ERROR_IO_PENDING, Refused ⇒ trampoline (teardown — byte authority returns
  to stock; recorded decision); poll Complete ⇒ report TRUE, Incomplete ⇒
  FALSE + ERROR_IO_INCOMPLETE, NotPending ⇒ stock report-and-zero.
  `binding::integration_available()` now reports the installed state
  (windows) / false (host); `lib.rs` installs the pair between
  `wavebank_hook::init` and the readiness computation. Step-1 tripwires
  INVERTED in place: the readiness test asserts the live linkage
  (binding leg == `integration_available()`; conjunction == the binding leg
  with everything else true), the availability test proves the row registers
  EXACTLY when ready (both directions, modeled enable gate). Validator:
  `identity_runtime` needed no check edits (no identity-base readiness pin
  exists there — verified; `identity_no_dynamic_redirect` survives);
  file-presence list gained the new module. Assist-tick scaffold gate
  (req 32): `RateSnapshot::is_non_identity_commit()` (host-tested predicate)
  + the `Phase::AwaitAnchor` refusal in `assist_tick.rs` (phase → Idle, one
  log line per song, fires after any loader-thread commit lands; 100%/
  uncommitted paths bit-identical). All five gates green (validator 159/159
  in 8.15 s; windows check 0 warnings; fmt clean; release DLL OK 48.6 s).
  **Step 4 demo holds: the complete runtime path is host-tested and
  release-built; NOT deployed — Step 5 is the first live run.**

- **Implementation Step 4 task-03 COMPLETE (2026-08-10, code-assist; working
  record: `implementation/step04-task-03-binding-preflight-and-transaction/`):**
  the real preflight + the reworked create/unregister transaction, host-tested
  end to end. `binding.rs`: real `BindRefusal` variants (mailbox wire codes;
  `IntegrationAbsent` deleted), `SourceView` (host buffers / windows
  FileManager-row glue), the real `prepare_binding` pipeline (fault legs →
  parse → plan → private try_reserve copy → `Binding::new` → producer spawn,
  retire-on-failure), `qualify_bind` + `bind_may_qualify` (one-atomic-load gate;
  Armed+non-dance declines silently; Committed+same-digest = Quick Restart),
  `BindingRegistry` (active `AtomicPtr` + 4-slot retired list + cooldown-gated
  sweep at `Retired ∧ readers==0` + coalescing refusal mailbox);
  `Binding::generation` widened u32→u64. `transaction.rs`: `BindOutcome` + the
  pre-original `bind: FnOnce(i32) -> BindOutcome` inside the existing
  containment (frame-gated; post-original byte-for-byte untouched — the whole
  pre-existing matrix passes with only `|_| BindOutcome::Stock` call-site
  additions); `FaultSelector` gained the five streaming legs (`source-read`,
  `header-synth`, `generator-start`, `bind-refused`, `mid-song-failure` —
  arms kill-after-64-blocks). `wavebank_hook.rs`: cfg-agnostic `bind_for_create`
  (qualify → preflight → claim/expose/attach as the LAST fallible tail →
  mark_exposed/mark_reexposed → publish; FirstBind refusal ⇒ EarlyFailed +
  mailbox; QR refusal ⇒ Committed kept + Q31 `reset_identity` + mailbox),
  `unregister_prelude` (pre-original retire + slot release + ReclaimBinding),
  `retire_after_create` (LateFailed/RecoveryFailed ⇒ retire); windows glue:
  file-table stash (task-01 layout), source/path row resolution, real closure in
  `create_hook`, prelude in `unregister_hook`. `xact_runtime.rs`:
  `finish_quarantine` (late-fail slot recycling). `runtime.rs` drain: event
  consumption (ReleasePending/Quarantined slot freeing), registry sweep with the
  once-per-generation metrics INFO (Step 5's benchmark inputs), coalesced
  refusal WARN. +21 host tests (refusal legs incl. a degenerate-loop Plan
  refusal — the 28-bit leg needs a ~73 MB fixture; commit-order probe with the
  bind in front; late-fail retire with Q31 never published;
  unregister-with-pending clamp cancellation + quiescent reclamation; Quick
  Restart offset-zero re-bind with ledger idempotence; QR-refusal conservative
  clock reset; mid-song fault → real SilenceFill; finish_quarantine;
  fault-parse matrix with `source-read` moved to the positives). Readiness
  untouched (`integration_available()` still constant false; both Step-1
  tripwires ran unmodified; zero validator edits). All five gates green
  (validator 158/158 in 7.83 s cargo-test phase; windows check 0 warnings; fmt
  clean; release DLL OK).

- **Implementation Step 4 task-02 COMPLETE (2026-08-10, code-assist; working
  record: `implementation/step04-task-02-binding-core-and-generator/`):** the
  streaming engine's runtime heart, host-tested end to end. `binding.rs` grew
  the `Binding` runtime core: 16 MiB `Ring` (cursors are ABSOLUTE VIRTUAL FILE
  OFFSETS, one linear producer cursor over entry0+gap+entry1; seqlock `rewinds`
  counter — readers re-validate after copy and fall back to deferral on a torn
  race), 4 SPSC `PendingSlot`s (Free→Arming→Armed→Completing→Complete; the
  accumulator ptr abstracts OVERLAPPED.Internal; exactly-one-completer CAS
  among producer/silence-flip/retire-cancel), epoch guard (`reclaim_eligible` =
  Retired ∧ readers==0), pre-encoded per-entry silent blocks, per-generation
  metrics (frames/wall/deferral count/max latency), fault knob
  (kill-after-N-blocks), and the PURE serve dispatch + poll consume
  (allocation/log/panic-free — task-04's detour bodies verbatim; full-request
  deferral, behind-window regen-target recording, stock
  accumulate-on-serve/report-and-zero-on-poll). New `generator.rs`: the Step-3
  `EncodedFeed` productionized (`Feed` + `positioned_at` restore-or-fresh +
  produce-and-discard regeneration), synchronous `GeneratorCore::step()` (stop
  token → regen → slot completion → pace (capacity/2 ahead of consumed; armed
  slots override) → bounded chunks `min(16 blocks, capacity/4)` — a chunk may
  never out-race the window), `spawn` (`song-rate-generator` thread,
  catch_unwind → SilenceFill → wall metric; idles at EOF for regeneration duty
  until the generation token stops it). +7 host tests in `generator_tests.rs`
  (own fixture/oracle copies): oracle byte-equality through the ring (both
  physical orders × {50,175}, reparse+decode), deferral exactly-once,
  behind-window loop-restart regeneration determinism (shrunken test ring),
  real-thread silence-fill validity (stream parses+decodes), retire
  cancellation + reader quiescence, thread metrics
  (frames==planned durations), core Idle-at-EOF/stop semantics. Validator
  file-presence list gained the two new files. Readiness untouched
  (`integration_available()` still constant false; both Step-1 tripwires ran
  unmodified). All five gates green (validator 137/137 in 7.97 s cargo-test
  phase; windows check 0 warnings; fmt clean; release DLL OK).
- **Implementation Step 4 task-01 COMPLETE (2026-08-10, code-assist; working
  record: `implementation/step04-task-01-xact-io-callback-signatures/`):**
  `song_rate_io_callback_regsite` signature (audio-manager ctor callback
  registration: 0xFA lookAheadTime imm + 3× LEA/MOV, disp32/disp8 wildcarded)
  + `derive_song_rate_io_callbacks` in `src/core/signatures.rs` — re-scans for
  single match, RIP-decodes the readFile (match+21) + getOverlappedResult
  (match+32) detour targets, decodes the handle→file_id stock lookup helper
  (readFile entry+0x21 CALL behind a 34-byte literal-prologue validation;
  Option A settled: call the stock helper — replicates the locked
  sorted-vector walk exactly, one documented double-lookup cost on unbound
  reads), and the audio file-table global (unregister match+18 RIP decode
  behind a `48 8B 05` opcode check). Fail-closed publish-or-remove-all over
  all five names + one WARN; none in the required set (fail-open feature —
  readiness untouched, `integration_available()` still false). Ghidra
  cross-verified on ALL FOUR builds (regsite matches exactly once each;
  20260721 targets equal FUN_1801aa250/FUN_1801aa350; the two owed
  file_table decodes filled: 0616=0x1806f1f50, 0421=0x1806ee068; decode
  arithmetic sanity-checked against both previously known values).
  `docs/xact_streaming_research.md` §6 gained the six-column cross-version
  table + the Option-A record; the owed-verification note is closed. All
  five gates green (windows check 0 warnings).
- code-task-generator run for plan Step 4 (breakdown approved by maintainer
  2026-08-10): `.agents/tasks/song-rate-streaming/step04/` —
  `task-01-xact-io-callback-signatures.code-task.md` (callback-pair AOB +
  file-table/handle-walk derivations + 4-build Ghidra cross-verification,
  Medium), `task-02-binding-core-and-generator.code-task.md` (Binding
  state/ring/pending slots/epoch guard + producer thread + the pure serve
  dispatch, High), `task-03-binding-preflight-and-transaction.code-task.md`
  (real prepare_binding + pre-original bind closure + unregister/drain
  reclamation + Quick Restart + the five streaming fault legs, High),
  `task-04-io-callback-detours-and-readiness.code-task.md` (the detour
  pair as thin glue + readiness/row flip with the Step-1 tripwires
  inverted + assist-tick scaffold gate, Medium). Settled at approval:
  4-task split; the pure serve dispatch lives with the Binding (task-02),
  detours stay thin; task-01 halts if any of the four builds is missing
  from the Ghidra project; scaffold gate rides task-04. Ring reading
  recorded in task-02: base/produced/consumed are ABSOLUTE VIRTUAL DATA
  OFFSETS (one linear producer cursor covers entry 0 + gap + entry 1).
- **Implementation Step 3 task-03 COMPLETE (2026-08-09, code-assist; working
  record: `implementation/step03-task-03-validator-replay-legs/`):** the
  plan Step 3 DEMO — validator `streaming` section gains full synthetic
  replay legs at 50% (main-first fixture) and 175% (preview-first fixture):
  `replay_virtual_bank` in the harness main.rs (compact re-derivation:
  `plan_virtual_bank` → `drive_streaming` + `encode_block` feed →
  resolve-served 0x1000 header read + sequential block-align-rounded 64 KiB
  packets + EOF read) → reassembled, reparsed, both entries decoded and
  matched against the `transform_bank` oracle. New `StreamingReplayResult`
  rows (`replays`) + `streaming_replay_{50,175}` checks, `passed`/
  `overall_pass` conjoined; schema unchanged `song-rate-validation/v1`;
  python gate structurally unchanged and spot-checked (mutated copies
  rejected). All five gates green. **Plan Step 3 checkbox ticked** (all
  three task records `Status: Complete`).
- **Implementation Step 3 task-02 COMPLETE (2026-08-09, code-assist; working
  record: `implementation/step03-task-02-synthetic-engine-replay-harness/`):**
  test-side synthetic engine replay in `src/core/xact/tests.rs` (zero crate
  changes): audio fixture builder (`build_bank_bytes` generalization; main
  entry 32,768 frames / preview 2,048, 8 kHz stereo, full-entry loops),
  `transform_bank_oracle` (the validator composition rebuilt), `EncodedFeed`
  (BlockCachePcm → StretchState → whole-block accumulation → encode_block;
  in-order serves; checkpoint capture; `restore_at_block` = restore +
  discard-to-block-boundary), `serve_read`/`replay_engine_reads` (resolve-
  driven pump: 0x1000 header read spanning pre-data into entry-0 data,
  sequential block-align-rounded 64 KiB packets stream-bounded, defensive
  EOF read). +3 host tests (130/130): feed bytes == oracle payloads
  ({50,175}), full replay matrix ({25,50,100,175} × both entry orders —
  reassembly byte-identical, reparse, decode equality, read-pattern
  fidelity), loop-restart byte equality (50% full-entry zero-discard +
  75% interior nonzero-discard bridge). Phase 7.56 s. All five gates green.
- **Implementation Step 3 task-01 COMPLETE (2026-08-09, code-assist; working
  record: `implementation/step03-task-01-virtual-bank-layout-and-resolve/`):**
  `xwb::stream_pre_data` (pub composition over the UNTOUCHED private
  `validate_stream_write_layout` + `write_stream_header` pair — serializer
  and virtual bank share one canonical emitter, serializer suites pass
  unmodified); `virtual_bank::plan_virtual_bank` → `VirtualBankLayout`
  (both entries in physical order, `main_entry_index` via the parser's
  identity invariant, entry offsets 2048 / next-2048-aligned, pre-data =
  serializer prefix byte-identically, `virtual_size` ==
  `serialized_song_bank_len`) + `resolve(offset, len)` (`Region::{PreData,
  EntryData, Gap, Eof}` + `ResolvedSpan`, stock EOF clamp, spanning reads by
  iteration). Additive `PlanError::EntryRate { index, source }` (rate
  refusals gain entry identity at the whole-bank level; `plan_entry` keeps
  the stub identities) and `PreData(String)` (structurally unreachable
  emission leg). +4 host tests (127/127): pre-data equality/reparse both
  entry orders, resolve reconstruction across chunkings incl. the real
  0x1000-then-64KiB engine shape, EOF/gap/spanning legs, refusal identity
  (28-bit at entry 1, degenerate loop at entry 0). All five gates green.
- code-task-generator run for plan Step 3 (breakdown approved by maintainer
  2026-08-09): `.agents/tasks/song-rate-streaming/step03/` —
  `task-01-virtual-bank-layout-and-resolve.code-task.md` (plan_virtual_bank +
  pre-data synthesis via factored xwb emission + resolve/EOF clamp, Medium),
  `task-02-synthetic-engine-replay-harness.code-task.md` (pull-driven engine
  read-pattern pump + encoded feed + loop-restart via checkpoint, High),
  `task-03-validator-replay-legs.code-task.md` (streaming section 50%/175%
  replay legs — the step demo, Medium). Settled at approval: pre-data
  emission factored out of xwb.rs (serializer suites unmodified as guard);
  replay pump is test-side only with a compact main.rs re-derivation (never
  crate code — Step 4 owns the production producer); design-sketch field
  shapes free, behavior binding.
- **Implementation Step 2 task-03 COMPLETE (2026-08-09, code-assist; working
  record: `implementation/step02-task-03-validator-streaming-section/`):**
  validator `streaming` report section (the plan Step 2 DEMO) — per-rate rows
  across 25/50/75/100/125/175 with byte-equality/counters/chunking/checkpoint
  legs all green, exercised through the REAL planning path
  (`virtual_bank::plan_entry`, production full-entry loop shape) + the REAL
  on-demand view (`adpcm::BlockCachePcm`); informational
  `synthetic_frames_per_second` (≈1.65M at 8 kHz stereo on the dev host;
  outside every pass expression); `overall_pass` conjoins `streaming.passed`;
  python gate gains the streaming leg (rejects missing section / false passed
  / failing check — spot-checked via mutated copies); schema unchanged
  `song-rate-validation/v1`. All five gates green. **Plan Step 2 checkbox
  ticked** (all three task records `Status: Complete`).
- **Implementation Step 2 task-02 COMPLETE (2026-08-09, code-assist; working
  record: `implementation/step02-task-02-resumable-stretch-state/`):**
  `stretch::StretchState` (new/produce/checkpoint/restore + Produced +
  StretchCheckpoint + `StretchError::InvalidCheckpoint`) — pull-driven event
  machine (Identity/FirstCopy/Main/Terminal) emitting only FINALIZED frames
  with the provisional tail retained internally; byte-identical to the
  UNTOUCHED `stretch_interleaved_with` across the full behavior-parity matrix
  (rates × loop shapes × channels, counters included), chunking-independent
  down to one-frame buffers, checkpoint/restore reproduces suffixes
  (5-word checkpoint; phase recomputed; tail rebuilt from the previous
  window's direct-copy region — hence no checkpoints once the terminal region
  begins), bounded source access instrumented per event. +5 host tests
  (123/123). KEY DISCOVERY for Steps 3/4: the reference (and thus the
  streaming machine, by parity) deterministically fails `NoCandidate` at
  25%/50% WITHOUT a full-entry loop; stock banks' whole-entry loops succeed
  at all six rates — the generator must treat NoCandidate as a failure leg.
  All five gates green.
- **Implementation Step 2 task-01 COMPLETE (2026-08-09, code-assist; working
  record: `implementation/step02-task-01-block-codec-and-source-pcm/`):**
  public per-block `adpcm::encode_block`/`decode_block` (thin validating
  wrappers; privates renamed `*_raw`, whole-buffer paths byte-identical);
  `stretch::SourcePcm` trait + `SlicePcm` trivial impl (reference stretcher
  untouched); `adpcm::BlockCachePcm` (64-slot direct-mapped on-demand decode
  view, predictor pre-scan at construction, decode_interleaved-identical
  layout/duration clamp semantics). +4 host tests (118/118 in the validator
  cargo-test phase); all five gates green.
- code-task-generator run for plan Step 2 (breakdown approved by maintainer
  2026-08-09): `.agents/tasks/song-rate-streaming/step02/` —
  `task-01-block-codec-and-source-pcm.code-task.md` (codec block wrappers +
  SourcePcm view, Medium), `task-02-resumable-stretch-state.code-task.md`
  (the StretchState byte-equality core, High),
  `task-03-validator-streaming-section.code-task.md` (the streaming report
  section / step demo, Medium). Settled at approval: 3-task split; throughput
  metric lands now as informational/non-gating; SourcePcm trait consumer-owned
  in stretch.rs (guidance, not mandate).
- **Implementation Step 1 COMPLETE (2026-08-09, code-assist, both tasks; working
  records: `implementation/step01-task-01-retire-cache-model/` and
  `implementation/step01-task-02-binding-era-renames/`):**
  - task-01 (retire the cache model): deleted `cache.rs`, `worker.rs`, `model.rs`,
    `tests.rs` (the ~48-test cache/worker/model+conversion suites), and
    `core/xact/transform.rs` (pure entry-plan/loop-map logic relocated to the new
    `core/xact/virtual_bank.rs` stub with 6 ported-vector tests, incl. the exact
    176/1056 interior-loop vector and the 28-bit refusal). `conversion.rs` →
    `binding.rs`: kept `dance_bank_song_code` + `song_code_digest` (+
    `binding_tests.rs`), added `integration_available()` (constant false until
    Step 4) and the always-refusing `prepare_binding` (→ EarlyFailed). LayeredFS
    seams removed from `file_hooks.rs` (open/lstat redirect + convert seam);
    `runtime.rs` trimmed to scene callback + desired atomics + commit-log poll +
    timeline drain; `RedirectToken` lost all four digest fields; slots lost
    `lease_id`; `MaintenanceEvent` = {kind, slot_index}; FaultSelector lost
    source-read/validation/conversion legs; `cache_limit_gib` dropped from config
    struct AND `mod-config.json` (whole block). `IdentityReadiness` legs are now
    {clock, create, unregister, binding, movie} with the binding leg read from
    `binding::integration_available()`. Validator updated IN PLACE: cache +
    on_demand sections/checks/structs deleted (no schema change); the KEPT
    synthetic/corpus DSP sections run against a new harness-local `transform_bank`
    oracle composed from the surviving primitives (parse → virtual_bank plan →
    decode → stretch → encode → stream-write); admission-tied memory
    fields/thresholds died with the admission.
  - task-02 (binding-era renames + identity assertions): `Preparing`→`Binding`,
    `RedirectReady` deleted (with `begin_exposing`/`mark_redirect_ready`),
    `begin_preparing`→`begin_binding`; machine is now Armed → Binding →
    XactInFlight → Committed/LateFailed, Binding → EarlyFailed (new test),
    Committed → XactInFlight, CAS semantics unchanged.
    `MaintenanceKind::Quarantine`→`ReclaimBinding` (queue mechanics untouched).
    Identity-base assertions added: wavebank_hook_tests proves the readiness
    conjunction is structurally false via the binding leg (every other leg forced
    true), and availability_tests proves no `song_speed` row can register while
    unready (mirrors the mod's enable() gate) — plan Step 4 must flip both
    deliberately. Redirect-era vocabulary swept from surviving comments/docs
    (`RedirectToken` + slot-phase `Exposed`/`Quarantined` retained by design).
  - Gates (both tasks): song-rate validator green (task-01: 111/111; task-02:
    114/114 — +2 assertions, +1 transition test; report carries NO cache/on_demand
    sections, schema unchanged `song-rate-validation/v1`), se-bank-synth ALL
    CHECKS PASSED, windows-target `cargo check` 0 warnings, whole-crate fmt clean,
    `./build.sh` release DLL OK. Step-1 demo holds: identity-only build; zero
    references to caches, leases, deadlines, or admission anywhere in `src/`;
    `integration_ready()` structurally false; SONG SPEED row refuses registration.
  - Plan checklist: Step 1 ticked in `implementation/plan.md`.
- PDD Step 1: workspace created (`rough-idea.md` captured from the 2026-08-08 handoff
  brief; the referenced `streaming-pivot-brief.md` does not exist — the handoff prompt
  is the rough idea, as it anticipated).
- PDD Step 2: orientation pass recorded in `research/orientation.md` — module-level
  keep/remove/rework verdicts for all of `src/services/song_rate/` + `src/core/xact/`,
  the WSOLA streamability analysis (main loop already sliding-window; blockers are API
  shape + terminal anchor region only), and unknowns U1–U7.
- PDD Step 4 (interleaved, Ghidra `DDRWorld_Ghidra`, gamemdx 20260721 +
  xactengine2_10): resolved U1–U5. Durable RE note written:
  `docs/xact_streaming_research.md` (this doubles as the RE note deferred from the old
  plan's Step 8). Headline findings:
  - readFile/getOverlappedResult callback pair registered from the audio-manager ctor
    (`FUN_1801aab60`, lookAheadTime 250 ms; strong AOB anchor: 0xFA imm + 3×LEA).
  - readFile completes synchronously from the FileManager RAM copy;
    `OVERLAPPED.Internal` is the completion accumulator; EOF clamp `min(len, size−off)`.
  - Engine: 64 KiB packets (gamemdx passes packetSize 0x20 sectors), single 0x1000
    header read at offset 0 issued synchronously INSIDE `wavebank_create`, completion
    polled `bWait=0` from exactly one site, `FALSE+ERROR_IO_PENDING` tolerated at issue
    → native back-pressure. Data reads sequential, block-align-rounded, one
    outstanding per stream, loop-start is the only backward jump.
  - NO file-size cross-checks anywhere in the streaming path (engine `GetFileSize`
    only in the untouched WAV path; gamemdx imports it zero times) — a virtual bank
    larger/smaller than the on-disk file is safe.
  - Unregister removes the handle-vector entry and closes the handle synchronously
    after engine Destroy.
  - Design implications distilled in `research/streaming-mechanism.md` (binding by
    file_id pre-original; both callbacks must be detoured as a pair; ring-vs-progressive
    analysis; silence-fill failure shape; source-copy lifetime options).
- PDD Step 3: decision register D1–D14 written to `idea-honing.md` and presented as one
  batch. Maintainer response applied 2026-08-08: D1–D5, D8, D10–D14 accepted; D6
  OVERRIDDEN (full Assist Tick content→wall integration is REQUIRED for delivery — the
  headline use case is slow-rate + assist tick for chart practice; force-disable is
  interim scaffolding only), D7 OVERRIDDEN (no report/schema versioning — update the
  validator in place), D9 OVERRIDDEN (drop `cache_limit_gib` outright, no
  parse-but-ignore, no stale-cache cleanup code). D15 added (tick bank capacity
  1200 s wall) — proposed for settlement at readiness confirmation.

## In flight

- Nothing — Steps 1 and 2 done and uncommitted; awaiting maintainer
  review/commit, then code-task-generator for Step 3.

## Done (continued)

- PDD Step 7 complete: plan approved without changes (maintainer, 2026-08-08);
  `Status: Approved 2026-08-08` recorded in `implementation/plan.md`.
- PDD Step 8 complete: `summary.md` written (artifacts, design digest, next steps,
  standing assumptions: cabinet throughput retired only by plan Step 5; header-parse
  acceptance retired by Step 5's first bound create; tick domain algebra
  oracle-verified at Step 7).
- code-task-generator run for plan Step 1 (breakdown approved by maintainer
  2026-08-08): `.agents/tasks/song-rate-streaming/step01/task-01-retire-cache-model.code-task.md`
  (the atomic removal, High) and
  `task-02-binding-era-renames-and-identity-assertions.code-task.md` (renames +
  identity assertions, Low).

## Done (continued)

- PDD Step 6 complete: design approved without changes (maintainer, 2026-08-08);
  `Status: Approved 2026-08-08` recorded in `design/detailed-design.md`.
- PDD Step 7 draft: `implementation/plan.md` — Step 1 removal to identity-only base,
  Step 2 streaming WSOLA byte-equality core, Step 3 virtual bank + synthetic engine
  replay, Step 4 runtime wiring (detour pair AOB + binding + generator + transaction,
  host-only), Step 5 live bring-up + 25 %/175 % throughput benchmark (FIRST
  deployment; design-invalidating gate), Step 6 dependent features (Assist Tick
  conversion + 1200 s capacity, Real Speed, PUS), Step 7 docs + final matrix.
  Two deployments total (Steps 5 and 7), both maintainer-run.

## Done (continued)

- PDD Step 5: D15 accepted (tick capacity 1200 s wall; maintainer will revisit only
  under real memory pressure — considered unlikely; tick-ring alternatives analyzed
  and rejected: loop-region rewrite violates the proven rewrite-only-after-Stop rule,
  streamed tick bank couples a proven feature to new machinery). `Readiness Confirmed
  2026-08-08` recorded in `idea-honing.md`.
- PDD Step 6 draft: `design/detailed-design.md` — 42 numbered requirements (keepers
  marked), architecture + song-start sequence + lifecycle mermaid diagrams, component
  specs (streaming `StretchState`, `core::xact::virtual_bank`, `io_callback_hook`
  detour pair, `binding.rs` preflight, `generator.rs` producer, trimmed
  transaction/lifecycle/runtime), data models (pending slots, ring, phases),
  threading table, error table, testing strategy (in-place validator update with a
  `streaming` section; live benchmark as first live gate), alternatives, RE appendix.

## Deploy & test log

- 2026-08-10 (deploy #1, Step-5 first live run; maintainer): full run sheet
  PASSED — see the Done entry. Benchmark: ~21–22× realtime production at
  25/50/120/175%. FINDING: loading screen ≈ full production time (23–25 s at
  25%; every reclaim line showed 3 deferrals with max latency == production
  time). Root-caused to the preview entry at the virtual file tail + linear
  ring; fixed by the side-buffer model (gate-green, awaiting deploy #2).
- 2026-08-10 (deploy #2): v1 side-buffer fix DID NOT resolve the stall (still
  20+ s at 25%). The new STUCK-READ instrument named the truth in one run:
  the MAIN entry's first packet waits behind side-entry PRODUCTION at
  ~114k frames/s ≈ 2.4× realtime — WSOLA at the game's 47 kHz is only
  ~2–6× realtime on this hardware (the earlier "21× realtime" reading
  divided total frames by the stall; the stall only ever covered the
  preview). Stretching the preview during loading is inherently 10–25 s.
- 2026-08-10 (deploy #3): v2 passthrough REGRESSED to clean 100% playback —
  every bind refused `HeaderSynth` (fail-open working as designed). Root
  cause: the stream-layout validator applied the generated-content
  whole-block rule to the verbatim stock preview, whose duration sits inside
  its final block; every host fixture was block-exact and blind to it. Fixed
  in `xwb.rs` (emission now uses the PARSER's layout rule); fixtures made
  honest first (22 tests reproduced the refusal on host, then green).
- 2026-08-11 (deploy #4): PASSED — loading ≈ 5 s (normal) at BOTH 25% and
  175%; the 25% song played full-through (517 s wall) with **0 deferrals,
  0 µs max latency** and frames == the main entry's planned output exactly
  once; no `bind refused`, no `STUCK READ`; tick scaffold fired both songs.
  The loading-stall chain is closed; **plan Step 5 demo holds** (engine
  accepts the virtual bank live; sustained ≥1× production proven the strong
  way — the engine never waited once across an 8.5-minute 25% song).
- 2026-08-11 (deploy #4 addendum): Quick Restart at a non-identity rate
  spot-checked live on the FINAL build — rate persisted, audio and arrows
  correct after the restart. Every run-sheet leg now has final-build live
  evidence.
- **2026-08-11 (deploy #5 — RELEASE MATRIX): PASSED (maintainer-run;
  log-verified).** Maintainer verdict: everything good. Log evidence
  (`log.txt`, session 19:14–19:33):
  - Legs 1–2 (slow/fast): generations committed at 175 % (gen 1,
    `rate 517667/295808`) and 50 %; EVERY binding reclaimed with
    **0 deferrals, 0 µs max latency** (e.g. gen 1: 3 253 888 frames /
    79 350 ms wall).
  - Leg 3 (tick alignment): `AssistTick: synthesis done -- ... rate=175%`
    and `rate=50%` lines present (mixed=438/475/819/543, dropped=0);
    alignment confirmed by ear at both rates.
  - Leg 4 (Real Speed): `song_rate/real_speed: side 0 multiplier 150
    (target 450 core 171.00 at 175%)` — exact math cross-checked
    (450·100/(171·1.75) = 150.37 → trunc 150 ✓; second song core 160 →
    160 ✓); velocity oracle confirmed, both fix states; no recompute line
    at 100 %.
  - Leg 5 (PUS CSV): the 50 % export's rows carry `50,5412481/10824960`;
    the 175 % export carries `175,517667/295808` — **byte-equal to the
    committed generation's logged exact ratio**; header + content-domain
    deltas intact.
  - Leg 8 (score containment): `score_guard: ... rate-tainted stage save
    SUPPRESSED (generation=1, stage=0)` and `(generation=2, stage=1)`;
    EAM_EXIT logout path reached; league-strip ordinal resolved.
  - Leg 10 (soak): multiple rate generations back-to-back, all reclaimed,
    deferrals flat at 0; **zero** `bind refused` / `STUCK READ` /
    silence-fill lines in the session.
  - Legs 6/7/9 (Quick Restart, Premium Free, 100 % literal stock):
    maintainer-confirmed (6 and 9 were deploy-#4 re-confirmations).
  **Design req 42 satisfied — plan Step 7 ticked; the feature is CLOSED.**
  The run sheet as issued follows (for the record):
- **2026-08-11 (deploy #5 run sheet, as issued).** The
  req-42 acceptance run on the final tree (Steps 1–6 + Step-7 docs).
  Build: `./scripts/deploy.sh` (release DLL at
  `target/x86_64-pc-windows-msvc/release/ddr_world_hook.dll`; all five
  gates green 2026-08-11). Record each leg's result inline; capture
  `log.txt` from the CrossOver install. Legs:
  1. **Slow song ≤ 50 %** (50 % or 25 %): loading a few seconds; music
     pitch-correct and slow; arrows/judging in sync end-to-end. Evidence:
     the drain's per-generation metrics INFO (0 deferrals expected).
  2. **Fast song > 100 %** (e.g. 150 %): same oracles, sped up.
  3. **Assist-tick alignment at 50 % AND 100 %** (the D6 headline): ASSIST
     TICK on, same chart at both rates — claps land on judgment moments at
     BOTH (at 50 % the claps space out but stay locked to the arrows).
     Evidence: `AssistTick: synthesis done -- ... rate=50%` /
     `... rate=100%` INFO lines; 100 % placement audibly unchanged.
  4. **Real Speed × rate**: a Real-Speed-mode player (speed type 0, e.g.
     target 400) at 50 % sees the SAME on-screen arrow velocity as at
     100 % (multiplier doubles) — check with the Real Speed Fix mod ON,
     then OFF (identical adjustment both ways). Fixed-multiplier mode at
     50 %: untouched (arrows at half wall speed). Evidence:
     `song_rate/real_speed: side {} multiplier {} (target {} core {} at
     50%)` INFO; NO such line at 100 %.
  5. **PUS CSV spot-check**: STEP DATA EXPORT on — a 50 % song's CSV
     (`step_data_exports/`) carries the
     `Song Rate Requested (%),Song Rate Effective` header cells with
     `50,<source/output fraction>` on every row; a 100 % song's CSV shows
     `100,1/1` and is otherwise byte-identical to a pre-Step-6 export.
  6. **Quick Restart at a non-identity rate** *(re-confirmation — passed
     on deploy #4's final build)*: restart mid-song at 50 % — rate
     persists, audio + arrows correct, clean ledger reclaim.
  7. **Premium Free interaction**: a rate song between 100 % songs inside
     a Premium Free session — stage flow uninterrupted, records intact.
  8. **Score containment re-oracle** *(re-confirmation)*: rate stage save
     suppressed (score_guard log line); card-out logout save sanitised;
     backend shows NO competitive record of rate songs and DOES show the
     interleaved 100 % scores.
  9. **100 % literal stock** *(re-confirmation)*: a pure-100 % session has
     zero bind/redirect lines; saves normal.
  10. **Long-session soak**: several rate songs back-to-back (mixed
      rates) — every generation's reclamation INFO shows bindings
      reclaimed (retired list never grows), flat deferral counts, no
      `bind refused` / `STUCK READ` / silence-fill WARNs.
  Matrix passes ⇒ tick plan Step 7's checkbox + close this feature
  (design req 42 satisfied); any failure ⇒ log it here and triage.

## Deviations & open questions

- **Step 2 discovery (matters for Steps 3/4): the WSOLA reference — and the
  streaming machine, by parity — deterministically fails `NoCandidate` at
  25%/50% WITHOUT a full-entry loop context** (near the output end the nominal
  exceeds the last window start by more than the search radius;
  content/size-independent, verified 8 kHz + 48 kHz). Stock banks carry
  whole-entry loops (the shape `plan_entry` produces), which succeed at all
  six rates — but the generator (Step 4) must treat `NoCandidate` as a
  failure/preflight leg, never an impossibility. Full detail:
  `implementation/step02-task-02-resumable-stretch-state/progress.md`.
- Step 2 detail deviations (all recorded in the task records): checkpoint is
  5 words with phase recomputed and NO checkpoints once the terminal region
  begins; allocation-bound reference failures (usize::MAX outputs) excluded
  from parity (whole-buffer-specific); bounded-access lower bound corrected by
  one source-hop (joint-SAD reference window); release-matrix loop shape is
  full-entry per rate (not the sketched interior/none — those fail at 25/50);
  checkpoint-restore exercised per rate (requirement over guidance).
- Step 1 implementation notes (full detail in the two task progress files):
  `MaintenanceKind` was carried through task-01 as `Quarantine` (sole survivor,
  needed by the late-fail push + saturation tests) and renamed to `ReclaimBinding`
  in task-02 exactly as the task split intended. `mark_exposed`/`mark_reexposed`
  and `RedirectToken` keep their names (slot-exposure vocabulary survives; the
  design's Step-4 spec still uses them). `wavebank_hook::identity_conversion_path`
  survives as the pin on the deleted LayeredFS seam (the validator's
  `identity_no_dynamic_redirect` check consumes it).
- Validator-editing gotcha (bit once in Step 1): the harness main.rs heredoc is
  UNQUOTED — backticks in inserted comments execute as command substitution.

- U7 (cabinet DSP margin vs the MacBook's ≈11× realtime) is not measurable during PDD;
  carried as a first-live-step benchmark requirement for the implementation plan.
- Cross-build AOB verification (0324/0421/0616) for the new callback-pair signature is
  owed during implementation (same protocol as the three existing song-rate
  signatures).
- Old plan Steps 7–8 (dependent features, hardening) were never implemented; scope
  decision is D6.

## Key facts for a cold resume

- Modpack branch `playback-speed-adjustment`, tip `ee0368f` (all retired-model work);
  working tree carries uncommitted progress-doc edits (maintainer commits, never us).
- bemani-buddy `mod_song_speed` backend work is uncommitted/staged in that repo and
  survives the redesign unchanged. Never touch it.
- KEEPERS: option mod, lifecycle eligibility, Q31 clock patch (+commit-LAST order),
  score_guard ledger/sanitised logout, rate.rs/adpcm.rs/xwb.rs/digest.rs.
- REMOVED: cache.rs, worker.rs, model.rs cache forms, conversion.rs redirect halves,
  transform.rs (reference only), `cache_limit_gib`, 30 s deadline, 128 MiB admission.
- Settled, do not relitigate: 25..=175 step 5 default 100; streaming-only no
  cache/no fallback; no server-side validation; no latency knob; Step-4 score
  containment semantics.
- Detours never allocate/log; one detour per target; three allocator heaps; the clock
  is wall-driven — never redesign the clock side; 28-bit XWB duration ceiling binds the
  header synth.
