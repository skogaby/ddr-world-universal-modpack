# Task: `services/stepmania/` + `mods/stepmania_packs/` — end-to-end injection

## Description
The feature's service and mod: index `./stepmania/Songs/<Pack>/<Song>/` on worker threads
from DLL step 0', derive identities, convert and self-check charts, materialise SSQ/XSB and
a thumbnail arc into `data_mods/_cache/stepmania/`, append `<music>` entries to `musicdb.xml`
through the Step 3 member-transform API, serve every per-song asset through the Step 3
resolver registry, build audio banks synchronously on first open (simplest delivery), taint
every injected play, and register the STEPMANIA folder/series through task-01's APIs.

## Background
Steps 1–5 proved the contract and built the pure pipeline (`core/sm`, `core/ssq/{writer,
check}`, `core/audio`, `core/xact/{song_bank,xsb}`) and the LayeredFS seams
(`mod_paths::register_resolver`, `arc_handler::{register_member_transform,
register_arc_fingerprint}`). Cabinet facts: FileManager opens/reads run on ONE non-render
I/O worker thread (E2), so a synchronous build inside the open is permissible (keep it
bounded); the game reads the whole XWB at the wheel-settle preview (scene 25) and again at
stage load; `/lastplay` leaks the committed mcode (Step 7 handles it); stock PNG jacket
members must be stored uncompressed (`core/arc.rs` writes uncompressed — fine). The writer's
split-timing output has no base `<b>.ssq` (only `<b>_N.ssq`), which needs Step 9's
`ssq_path` chain — until then such songs must NOT be declared.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-10-20-stepmania-pack-loader/design/detailed-design.md (§3, §4.7, §4.13 primary taint only, §4.15, §5 all, §6)

**Additional References (if relevant to this task):**
- .agents/planning/2026-10-20-stepmania-pack-loader/progress.md (Deploy & test log + Deviations: E2, jacket rule, writer/audio API contracts)
- src/core/ssq/writer.rs `//!` header (lead_in contract), src/core/audio/lead_in.rs, tools/sm_pack_check/src/main.rs (how the tool strings probe → lead-in → write → banks together; move `placeholder_code` into `identity.rs` and mount it from the tool)
- src/services/avs_layeredfs/{mod_paths,arc_handler}.rs (Step 3 APIs), src/services/avs_layeredfs/shader_synthesis.rs (synchronous work inside a boot-time arc open — precedent)
- src/mods/fast_bootup/cache.rs (`_cache` file conventions, fingerprint-guarded binary caches), src/services/avs_layeredfs/cache_hasher.rs
- src/services/score_guard.rs (taint flags), src/services/stage_records.rs (`side_entered`), src/mods/multiplayer_bot/mod.rs (`is_bot_side`)
- src/mods/config.rs (section structs; operator-only sections are never written), src/mods/mod_trait.rs (`Mod`, `is_active` rule in AGENTS.md)
- docs/stepmania_runtime_packs_feasibility.md §4.2 (psmap limits), §6.4 (identity constraints), §7.3 (entry synthesis), §19 (gotchas)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Layout: `src/services/stepmania/{mod.rs,index.rs,identity.rs,musicdb.rs,cache.rs,providers.rs,taint.rs}` registered in `src/services/mod.rs`; `src/mods/stepmania_packs/mod.rs` registered in `src/mods/mod.rs` and constructed in `src/lib.rs`'s mod list (default ON — not in `DEFAULT_OFF_MODS`). `identity.rs` and `musicdb.rs` pure (`crate::core::*` only) and mounted by `scripts/validate_stepmania.sh` (add legs) and by `tools/sm_pack_check` (replace its placeholder).
2. Config: `StepmaniaPacksConfig { exclude_from_all_music: bool = false, audio_cache_mb: u32 = 1024, auto_sync: bool = true, index_timeout_s: u32 = 30, gimmick_policy: String = "hide_chart" }` as `Option<..>` on the config file struct; absent = defaults; never written.
3. `stepmania::early_init()` in `lib.rs` immediately after `mods::config::init()` and BEFORE `register_startup_arc_hook` / `avs_layeredfs::init()`: if the mod is enabled in config, ensure `./stepmania/Songs/` exists (create; also write `./stepmania/Songs/README.txt` once if absent — short operator note), spawn the scan (`available_parallelism().clamp(2,6)` workers over song folders), register the resolver, the `startup.arc` member transform + fingerprint, and (when the index later reports ≥ 1 song — the transform runs inside the open, so do the folder/series registration from the transform path or from a short wait at `early_init` if the warm index is instant) `folder_expansion::register_runtime_folder` + `series_expansion::register_runtime_series` using the lowest free bit/value and label dir `./data_mods/stepmania_packs/series_labels`.
4. `identity.rs`: design §5.2 exactly (fnv1a64 over case-folded `<pack>/<song>`; basename `z` + 5 base36; mcode `9_000_000 + (h >> 40) % 1_000_000`; probe on in-index collision in sorted-key order; reserved mcodes/basenames excluded).
5. `index.rs`: per song — pick `.ssc` over `.sm`; `core::sm::parse_bytes` (4 MiB cap); audio file = `#MUSIC` else first `.ogg/.mp3/.wav` (cap 64 MiB, duration cap 15 min via probe); `core::audio::probe`; lead-in via `choose_lead_in` from a trial write (as the tool does); `write_song`; **skip the song if the bundle has no `index == 0` file** (split timing — Step 9); `check_ssq`; levels = meter clamped 1..19; title/artist per design §4.7 (translit if present else native — glyph table is Step 12); `#SELECTABLE:NO` → skip; record `SongEntry` (§5.1) with rejects; thumbnail via `core::art` (NEW `src/core/art/{mod,jacket,thumbnail}.rs`: `compose_square(sources)`, `thumbnail_dds` 192² BC1 via `texpresso`, `jacket_arc`/`thumbnail_arc` via `core::arc::ArcArchive`, placeholder image embedded with `include_bytes!` from `data_mods/stepmania_packs/placeholder_jacket.png` — generate a simple 512² PNG). Warm path: `index.bin` keyed by `(path,size,mtime)` fingerprints of simfile+audio+art — unchanged songs reuse cached entries and skip conversion when their artefacts exist; a changed/new song is re-converted; removed songs' artefacts are deleted. `wait_ready(deadline) -> IndexSnapshot`.
6. `musicdb.rs`: `entry_xml(&SongEntry, series, property_bit) -> String` per design §5.4 (types/attributes byte-for-byte like stock, `title_yomi` rule, UTF-8-boundary clip to 127 B, XML escaping) and `transform(xml, entries, series, bit) -> Option<Vec<u8>>`: parse existing mcodes/basenames (regex-free scan is fine), drop + record collisions, insert before `</mdb>`. Fingerprint string = index digest (fnv over sorted `(mcode, basename, slots, levels, title)`).
7. `cache.rs`: layout §5.3; LRU over `xwb/` + `jacket/` by `audio_cache_mb` with `lru.bin` touch order (evict on insert; touch on serve); `_cache/stepmania` wholesale deletable.
8. `providers.rs`: resolver `fn(&str) -> Option<String>` for `mdb_apx/ssq/<b>.ssq`, `sound/win/dance/<b>.{xsb,xwb}`, `arc/jacket/<b>_jk.arc`, `arc/thumbnail/jacket_thumbnails_{ja,ua}_<S>.arc` (cheap prefix test first; `None` for non-injected basenames). XSB/SSQ/thumbnail are materialised already. XWB: if the requested variant's file is complete, return it; else if `!widget_renderer::is_render_thread()` build it NOW synchronously (preview variant when `scene_manager::is_available() && current_scene()==25`, else full), then return; else return a silent variant (`xwb/_silent/<b>.xwb`, generated on demand) and WARN once. Jacket: compose on first request from the song's art sources (same thread rule) else placeholder arc. Mark the full-bank serve as the gameplay-load signal for `taint.rs`.
9. `taint.rs` + `score_guard`: add `set_injected_song_taint()` (global `AtomicBool`, included in `is_stage_suppressed`, cleared by `reset_song_taint` and `reset_session`); on a full-variant bank serve for an injected basename, set it and call `mark_session_tainted(side)` for each side where `stage_records::side_entered(side)==Some(true)` and `!multiplayer_bot::is_bot_side(side)` (if `stage_records` is unavailable, mark both sides). Expose `stepmania::is_injected(mcode)` / `is_injected_basename(&str)` for Step 7.
10. `mods/stepmania_packs/mod.rs`: id `stepmania-packs`, `required_signatures` empty, `is_active()` = root exists ∧ LayeredFS available; `init` logs the index summary; `enable` publishes the splash line via whatever the existing splash uses for per-mod lines (look at `lib.rs` splash code; if no per-mod line API exists, log only and leave the splash for Step 12).
11. Release: `scripts/build_release_archive.sh` ships `stepmania/Songs/README.txt`; confirm the updater's plan never prunes files it never shipped (`updater/src/plan.rs`) so operator packs under `stepmania/Songs/` survive updates; document in the module header.
12. Logging: one INFO summary at index completion (`N songs, P packs, M rejected, cold/warm, ms`), one WARN line per rejected song with reason, one-shot WARNs on fallback branches. Every hook-path body panic-contained; no `unwrap`/`expect` on untrusted data.
13. Gates: `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`, `./build.sh`, `./scripts/validate_stepmania.sh` (with new legs for identity + musicdb), `scripts/validate_sm_pack_check.sh`, path-leak grep.

## Dependencies
- Task 01 (folder/series APIs + art) must be in the tree first.

## Implementation Approach
1. Pure pieces first (identity, musicdb, art) with harness legs.
2. Index + cache + materialisation; `early_init` + transform + resolver.
3. Providers (synchronous banks) + taint; mod + config + lib.rs; release bits.

## Acceptance Criteria

1. **Boot with a pack** — Given `Stamina RPG 5` (134 `.sm` songs) in `./stepmania/Songs/`, when the cabinet boots, then the log shows the index summary, `transformed 'data/gamedata/musicdb.xml'`, 0 `ME1529`, and the STEPMANIA folder + VERSION entry exist (task-03 verifies).
2. **Warm boot** — Given a second boot with nothing changed, then the index loads from `index.bin` in well under a second and no conversion runs.
3. **Resolver** — Given an injected basename, then `mod_paths::find_first_modfile("mdb_apx/ssq/<b>.ssq")` returns the cache path (so `chart_length` et al. work).
4. **Taint** — Given an injected song loaded for gameplay, then `score_guard::is_stage_suppressed(side)` is true for the entered side and the session is marked tainted.
5. **Harness** — identity determinism/probing/collision; `entry_xml` schema and byte limits; all previous legs green.

## Metadata
- **Complexity**: High
- **Labels**: service, mod, layeredfs, cache, taint
- **Required Skills**: Rust, in-process hooking discipline, LayeredFS internals
- **Generated By**: code-task-generator 2026-10-22
- **Source Plan**: .agents/planning/2026-10-20-stepmania-pack-loader/implementation/plan.md
- **Plan Step**: Step 6: End-to-end injection — index, identity, musicdb, SSQ/XSB/thumbnails, blocking banks, folder/series, primary taint
