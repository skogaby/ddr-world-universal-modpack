# Task: Mod skeleton, bootstrap crawl thread, baseline load

## Description
Turn the module into a registered mod: implement the `Mod` trait
(`PerSongJudgementOffsetsMod`, id `per-song-judgement-offsets`), register it
in `src/lib.rs`, and implement `bootstrap.rs` — the background thread that
crawls the merged musicdb, append-merges missing codes into
`judgement_offsets.csv`, loads the baseline into the store, and hosts the
coalesced CSV writer later steps use for edit upserts.

## Background
Design contract (Components → DLL sections; Error Handling table):
- `required_signatures()` = `["player_option_table", "selectmusic_model"]`
  (both exist and are verified on all four builds — `player_option_table` is
  a derived signature, `selectmusic_model` a scanned one).
- `enable()` is fully gated on `custom_options::row_injection_available()`
  (design requirement 9 / register D20): when false, log one WARN and return
  — no bootstrap thread, no CSV writes, nothing.
- Crawl (design → musicdb crawl): call
  `avs_layeredfs::xml_merger::merge_xmls("gamedata/musicdb.xml", "/data/gamedata/musicdb.xml")`;
  `Some(path)` → read that file; `None` → try
  `mod_paths::find_first_modfile("gamedata/musicdb.xml")` (whole-file
  override) → else `xml_merger::load_xml_from_avs_path("/data/gamedata/musicdb.xml")`.
  Retry briefly (poll a few times over several seconds) if AVS isn't serving
  yet; LayeredFS init precedes mod enable in `lib.rs`, and the game itself
  reads musicdb ~750 ms after boot.
- CSV path: `judgement_offsets.csv` (CWD-relative, like `mod-config.json`).
  Crawl failure ⇒ WARN once, use the existing CSV as-is; CSV missing ⇒ create
  it (header-only if the crawl also failed). Existing rows never modified.
- After the merge, load the baseline into `store::with_store` (`load_baseline`).
- Background CSV writer: an mpsc worker (chart_length pattern —
  `src/services/chart_length.rs`) accepting upsert messages
  `(code, side, Option<i8>)`; coalesces by re-reading/rewriting the whole doc
  per drain batch. Exposed as `bootstrap::queue_csv_upsert(...)` for Step 4.
  Writer failures: WARN once, in-memory state unaffected.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-17-per-song-judgement-offsets/design/detailed-design.md

**Additional References:**
- .agents/planning/2026-08-17-per-song-judgement-offsets/research/musicdb-and-ui.md (§1)

## Technical Requirements
1. `mod.rs`: `PerSongJudgementOffsetsMod` implementing `Mod` (see
   `src/mods/mod_trait.rs`; simple reference: `src/mods/announcer_mute.rs` or
   `music_wheel_song_length.rs` for shape). `init` stores nothing yet (later
   steps add signature reads); `enable` gates on row injection + spawns the
   bootstrap thread exactly once (re-enable safe); `disable` is a no-op for
   the bootstrap (thread is fire-and-forget; writer keeps serving — the mod
   enable flag gates later steps' behavior, matching the design's inert
   rule at the enable() boundary).
2. Register in `src/lib.rs`'s `mods_to_register` list.
3. `bootstrap.rs`:
   - `pub fn start()` — spawns the named thread ("judgement-offsets") once
     (OnceLock guard), panic-contained body (`catch_unwind`).
   - Crawl per the Background section; parse text with
     `musicdb_scan::scan_basenames`; CSV via `csv::parse/append_missing/serialize`;
     aggregated one-shot WARNs from `ParseStats`.
   - Atomic-ish write: write to `judgement_offsets.csv.tmp` then rename over.
   - Baseline into the store; `store.is_armed()` becomes true only on success
     (even a failed crawl still loads whatever CSV exists — armed either way
     as long as the CSV was readable or created).
   - Writer channel + `queue_csv_upsert(code: String, side: usize, value: Option<i8>)`.
4. Logging via the crate's `log_info!/log_warn!` macros only; all thread
   bodies panic-contained; no locks held across file I/O where avoidable
   (clone the doc state or re-read inside the worker).
5. `cargo check --target x86_64-pc-windows-msvc` clean; harness still green;
   `cargo fmt`.

## Dependencies
- task-01-musicdb-scan (scan function).
- Step 1 csv/store layers.

## Implementation Approach
1. Mod skeleton + lib.rs registration first (compiles, logs on enable).
2. bootstrap.rs crawl pipeline; factor the "resolve musicdb text" into a
   helper returning `Option<String>` with the three-way fallback.
3. Writer worker last; wire `queue_csv_upsert`.

## Acceptance Criteria
1. **Registration** — Given the DLL boots with the mod enabled — When enable
   runs with row injection available — Then one INFO logs the bootstrap
   start; with row injection unavailable, one WARN and nothing else happens.
2. **CSV self-creation** — Given no `judgement_offsets.csv` — When the crawl
   completes — Then the file exists with the full basename list and blank
   offsets, and the store is armed. (Cabinet check at deploy.)
3. **Append-only update** — Given an existing CSV with values — When the
   crawl finds new codes — Then only new rows are appended; a diff shows no
   other change. (Host-verifiable via the pure layers; cabinet-confirmed.)
4. **Crawl failure tolerance** — Given AVS/merge resolution fails after
   retries — When bootstrap finishes — Then one WARN, the existing CSV loads
   as baseline, store armed.
5. **Build hygiene** — check/fmt/harness all clean.

## Metadata
- **Complexity**: Medium
- **Labels**: rust, mod-lifecycle, background-thread, layeredfs
- **Required Skills**: Rust, project mod conventions
- **Generated By**: code-task-generator 2026-08-17
- **Source Plan**: .agents/planning/2026-08-17-per-song-judgement-offsets/implementation/plan.md
- **Plan Step**: Step 3
