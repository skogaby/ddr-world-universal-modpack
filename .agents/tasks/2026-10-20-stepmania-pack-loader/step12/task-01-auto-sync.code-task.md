# Task: auto-sync — estimator port, sync sweep, persistence, SSQ rewrite

## Description
Port the `ddr-chart-tools` auto-sync estimator into the DLL as pure `src/core/sync/`, and
add `src/services/stepmania/sync_sweep.rs`: a below-normal-priority background thread that
measures every injected song's materialised SSQ against its audio (once per source
fingerprint), persists the outcome in `data_mods/_cache/stepmania/sync.bin`, and applies
corrections by atomically rewriting the SSQ's tempo seconds column with
`core::ssq::writer::shift_seconds`. Gated by the already-parsed `stepmania_packs.auto_sync`.

## Background
- Source: `~/Projects/ddr-chart-tools` (HEAD a733404) `src/sync/{mod,onset,estimate,events,
  timemap}.rs` (~1,400 lines incl. tests; pure; `rustfft = "6"`). The CLI wiring is in
  `src/job/sync_offset.rs` (`auto_sync_delta`, `decide`) and `src/job/mod.rs` ~L160 (SM→DDR:
  events from the model's charts placed on `TimeMap::from_tempo_pairs(&tempo_pairs, tps)` of
  the SSQ the job will write; audio = the decoded song). Calibration notes:
  `.spec/steering/tech.md`. Constants (`TARGET_OFFSET_MS = −2.11`, `DEFAULT_MAX_CORRECTION_MS
  = 60`, `SEARCH_MARGIN_MS`, rival/split/edge gates, `MIN_EVENTS`, all `ONSET_*`) are a
  calibrated set — copy verbatim, never tune.
- This repo's `src/core/sm/model.rs` already ports the tool's `Rational`/`Beat`/`Bpm`/`Song`/
  `Chart`/`Note`/`NoteKind` (plus extra kinds: lifts, fakes, rolls…). `src/core/audio/
  decode.rs::decode_whole(bytes, info)` returns normalised **stereo i16 at
  `info.target_rate()` without lead-in**. The bank the game plays = `lead_in_frames` of
  silence + that PCM (`SongEntry.audio.lead_in_frames`, `.rate`); the materialised SSQ's
  anchors already include `lead_in_ms` (`tempo_data[0] = −#OFFSET·1000 + lead_in_ms`, ≥ 60
  ms headroom so a correction down to −60 ms never goes negative — design §4.2).
- `src/core/ssq/writer.rs`: `WriteOptions { lead_in_ms, sync_bias_ms, .. }` (writer shifts
  every anchor by `lead_in + sync_bias`), `shift_seconds(ssq, delta_ms) -> Option<Vec<u8>>`.
  `src/core/ssq/timing.rs::TempoConverter::from_ssq` reads `(tick, ms)` anchors.
  `src/core/ssq/ssq_chunk.rs::find_chunk`.
- Index: `src/services/stepmania/index.rs` converts songs (`convert_song`-ish path near the
  `check_ssq` loop ~L640–700; `bundle.files` with `file.index`, `file.charts`), reuses warm
  songs when fingerprint + identity match and artefacts exist; `SongEntry` in `model.rs`
  (`fingerprint`, `audio`, `beat0_ms`, `split_levels`); file names via
  `convert::ssq_file_name(b, index)`; cache paths in `cache.rs`. `FORMAT_VERSION` = 4.
- Scheduler/threads: `src/services/stepmania/scheduler.rs` shows the
  `SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL)` pattern;
  `services::scene_manager::current_scene()`; scene ids in `src/types/scenes.rs`
  (SONG_TO_STAGE_INTERSTITIAL 26, STAGE_INDICATOR 27, GAMEPLAY 28).
- Full-bank build (Step 8): `src/services/stepmania/providers.rs` + `progressive.rs` stream
  decoded blocks into the ADPCM writer.
- Scroll gimmicks: `stepmania::on_gameplay_load` publishes `scroll_remap` tables with
  `entry.beat0_ms` (td0 of `<b>.ssq`); a sync shift moves beat 0 by `delta_ms`.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-10-20-stepmania-pack-loader/design/detailed-design.md`
  (§4.2 lead-in/`shift_seconds`, §4.5 `core/sync/`, §4.7 `sync_sweep.rs`, §5.1 `SyncState`,
  §5.3 cache layout, §6 error table "Auto-sync … Refused", §7 testing)
- Plan: `.agents/planning/2026-10-20-stepmania-pack-loader/implementation/plan.md` — Step 12
- `.agents/planning/2026-10-20-stepmania-pack-loader/idea-honing.md` D20
- `AGENTS.md` (Rust quality rules, purity/harness rules, path-leak rule, NO git commit)

**Additional References:**
- `docs/stepmania_runtime_packs_feasibility.md` §9.1 (lead-in / sync)
- `scripts/validate_stepmania.sh` (how pure modules are mounted; host crate deps)
- `tools/sm_pack_check/` (host CLI; mounts the same pure modules)

**Note:** Read the required documents before implementing.

## Technical Requirements
1. **`src/core/sync/`** (`mod.rs`, `onset.rs`, `estimate.rs`, `events.rs`, `timemap.rs`):
   faithful port. Replace the tool's `AudioBuffer` with an equivalent local struct (or a
   borrowed view `{samples: &[i16], rate, channels}`), keep every constant, refusal, sign
   convention and test. `TimeMap::from_tempo_pairs` is the path we use; `from_song` may be
   ported or dropped (drop its tests with it). `chart_events` takes `core::sm::model` charts
   (taps, hold/roll heads, shocks count; mines do NOT; lifts/fakes: StepMania-unjudged fakes
   must not count — decide per kind against what the SSQ writer emits as a step row, and
   document it). Pure: no `crate::` imports outside `crate::core`, no logging, harness-
   mountable. Add `rustfft = "6"` to the root `Cargo.toml` with minimal features (check that
   `./build.sh` and `./build_win7.sh` still build; avoid features that need std::arch paths
   which break the Win7 `-Z build-std` leg — if `rustfft` SIMD auto-detection breaks it,
   disable default features accordingly).
2. **Measurement input = what the game plays.** For each materialised SSQ file of a song
   (`<b>.ssq`, or each `<b>_N.ssq`): tempo pairs read from the file on disk; tps from its
   tempo chunk; events from the parsed simfile's charts that file carries (re-parse the
   simfile with `core::sm`, select the charts the writer put into that file — reuse the
   writer/index grouping rather than re-deriving it); audio = `decode_whole` of the source
   audio, with event times converted to that PCM's time base by subtracting
   `lead_in_frames / rate` (equivalently: prepend the lead-in silence). The ±1 ms CLI parity
   (req. 7) is the proof this is right.
3. **`sync.bin`** (pure format in a crate-free file, e.g. `services/stepmania/sync_state.rs`,
   hand-rolled LE + magic + version, fully bounds-checked, malformed ⇒ empty): key =
   `<file name>` (`zab12c.ssq` / `zab12c_4.ssq`) → `{source_fingerprint: u64, state:
   Applied{delta_ms: i16 (total vs the as-converted file, 0 = Unchanged)} | Refused{reason
   key}, ssq_td0: i32 (tempo_data[0] of the file after applying)}`. Atomic save (tmp +
   rename). A song is measured once per `SongEntry.fingerprint`; a fingerprint change drops
   its records (the index re-converts the SSQ anyway).
4. **Applying, crash-safe.** Commit order: write shifted bytes to `<file>.tmp` → save
   `sync.bin` with the new record (incl. `ssq_td0`) → rename tmp over the SSQ. Reconcile at
   sweep start: for every `Applied` record whose fingerprint matches, read the on-disk file's
   first anchor; if it differs from `ssq_td0`, rewrite with `shift_seconds(file, ssq_td0 −
   on_disk_td0)` (covers both crash orders and a re-conversion that did not carry the bias).
   Also make the index's conversion path pass `sync_bias_ms` = the stored `Applied` delta when
   the fingerprint matches (so a re-conversion lands already-synced; reconcile then is a
   no-op). Never write any SSQ when `auto_sync` is false (no sweep, no reconcile, no bias).
   No `FORMAT_VERSION` bump unless you change conversion output for `auto_sync: false`.
5. **`sync_sweep.rs`**: one thread, `THREAD_PRIORITY_BELOW_NORMAL`, started once after the
   index is complete AND the boot pass is over (e.g. first scene ≥ WARNING_SPLASH via
   `scene_manager`, or a short delay after `fast_bootup` completion — pick the simplest
   reliable signal and document it). Walks injected songs (`stepmania::injected_entries()`)
   lacking a matching record; pauses (polls ~250 ms) while the scene is 26/27/28; panic-
   isolated (`catch_unwind`); never touches songs with `audio_failed`. Priority bump: when a
   song is committed (scene-26 commit — `taint.rs` / `on_gameplay_load` already see the
   committed mcode), push it to the head of the queue so it is measured right after that
   stage (this is the "on-decide" path; sharing the full-bank decode is OPTIONAL — only do it
   if it is a small, clean change in `providers.rs`; otherwise re-decode in the sweep and say
   so). Log one INFO per song: `StepMania[sync]: '<pack>/<song>' <file> measured
   <m:+.2> ms -> apply <d> ms` / `unchanged` / `refused (<reason>)`, and one summary line per
   pass with counts and elapsed time. Respect AGENTS.md Rust rules (no unwrap/expect/
   indexing panics on the thread; no locks held across long work; nothing on hot paths).
6. **Scroll remap**: where `on_gameplay_load` publishes `entry.beat0_ms`, add the applied
   delta for `<b>.ssq` so `#SPEEDS` timing follows the shifted chart.
7. **Tests** (pure, in the mounted files and/or `scripts/validate_stepmania.sh` legs):
   ported estimator tests; synthetic leg (click track at known event times + known offset →
   `Apply` with the right delta; refusal paths: too few events, beyond cap, ambiguous/halves
   if constructible); `sync.bin` round trip + malformed input; reconcile arithmetic;
   `shift_seconds` round trip (+d then −d = identity). **CLI parity**: extend
   `tools/sm_pack_check` with an `--auto-sync` flag that prints, per SSQ file,
   `measured_ms`/`correction_ms`/outcome using the same code path as the sweep, and add a leg
   to `scripts/validate_sm_pack_check.sh` that, when `$DDR_WORLD_INSTALL/stepmania/Songs/
   Stamina RPG 5` and the CLI (`../ddr-chart-tools/target/release/ddr-chart-tools`, see how
   existing legs locate it) exist, runs both on ≥ 5 songs and asserts |Δcorrection| ≤ 1 ms
   (SKIP otherwise). Find the CLI's report invocation from its `--help`/`src/cli/` (it is an
   `sm→ddr` conversion with `--auto-sync report`; parse its `correction_ms=` log tail).
8. **Gates**: `cargo check --target x86_64-pc-windows-msvc` clean (no new warnings),
   `cargo fmt` (whole crate, no file args), `./build.sh` clean, `./build_win7.sh` clean,
   `./scripts/validate_stepmania.sh`, `./scripts/validate_sm_pack_check.sh`,
   `./scripts/validate_fast_bootup.sh`, `./scripts/validate_split_ssq.sh` all green; path-leak
   grep (`grep -rnE "/(Users|home)/[^/ ]+/"` over every file you touched/created) clean.

## Acceptance Criteria
1. Given SRPG5 songs, when `sm_pack_check --auto-sync` and the CLI's `--auto-sync report`
   run on them, then their corrections agree within ±1 ms (ITG-synced packs measure ≈ +7…+10
   ms on this pack).
2. Given `auto_sync: true` on the cabinet, when the sweep runs after boot, then each injected
   SSQ is measured once (INFO line), applied corrections rewrite the file atomically and are
   recorded in `sync.bin`, and the next boot measures nothing new.
3. Given `auto_sync: false`, then no SSQ is rewritten and no sweep thread starts.
4. The sweep never runs during scenes 26–28 and runs at below-normal priority.
5. A crash between the `sync.bin` save and the SSQ rename is repaired at the next sweep start.

## Constraints
- Allowed files: `src/core/sync/**` (new), `src/core/mod.rs`, `Cargo.toml`/`Cargo.lock`,
  `src/services/stepmania/{sync_sweep.rs (new), sync_state.rs (new), mod.rs, index.rs,
  cache.rs, model.rs (only if needed), providers.rs (only for the optional decode share),
  taint.rs (only for the commit hook)}`, `src/mods/stepmania_packs/mod.rs` (start the sweep),
  `scripts/validate_stepmania.sh`, `scripts/validate_sm_pack_check.sh`,
  `tools/sm_pack_check/**`. Ask (report back) before touching anything else.
- Do NOT detour anything; do NOT touch `fast_bootup`, `avs_layeredfs`, `score_guard`.
- Never run `git commit`/push. Do not deploy to the game install.

## Metadata
- **Complexity**: High
- **Labels**: auto-sync, stepmania, port
- **Required Skills**: Rust, DSP port fidelity, crash-safe persistence
- **Generated By**: orchestrating agent 2026-10-24
- **Source Plan**: .agents/planning/2026-10-20-stepmania-pack-loader/implementation/plan.md
- **Plan Step**: Step 12: Auto-sync — estimator port, sweep, on-decide measurement
