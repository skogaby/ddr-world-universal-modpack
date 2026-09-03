# Orientation — Split SSQ Auto-Discovery

Blind-spot pass over the idea against the repo and the RE record
(`docs/split_ssq_research.md`). Everything below was checked in code, not
recalled.

## What the idea sits on

- **One hook point covers everything.** `build_ssq_path(out[0x100], basename,
  difficulty 0..4)` is called by all four SSQ consumers (boot analysis pass,
  `DancePlaySequence::onSetup`, `MatchingDancePlaySequence::onSetup`,
  `PlayerCourseWork::prepare`) and its output goes straight into the FileManager
  register. Everything downstream — AVS open (LayeredFS-visible), `SsqReader`,
  the shared `services::analyze_hook` boundary (mines, fast_bootup capture), the
  per-song-offsets SSQ-open observer — sees whatever file the builder named.
- **The only DLL-side path builder that does NOT go through the game function** is
  `src/services/chart_length.rs:167` (`mdb_apx/ssq/<code>.ssq`). Out of the
  detour's reach; would need to consume the same resolver explicitly.
- **Mod skeleton precedent:** `src/mods/announcer_mute.rs` — a single
  `GenericDetour` in a `static mut`, `HOOK_INSTALLED` atomic driving `is_active`,
  `required_signatures` = the one AOB, `enable()` installs / `disable()` removes.
  This mod is the same shape minus the option row.
- **Registration:** `src/lib.rs` mod list (line ~132 onward); default-ON unless in
  `DEFAULT_OFF_MODS`. `enable_with_config` runs before the boot screen — the
  fast_bootup mod's `onUpdate` detour proves mod hooks are live before
  `CheckStepDataActor::onInit` runs, so a detour installed in `enable()` is in
  place for the first builder call.
- **LayeredFS mod-folder discovery API:** `src/services/avs_layeredfs/mod_paths.rs`
  — `available_mods()` (ordered mod dirs), `find_first_modfile(norm_rel)` (the
  file LayeredFS would actually serve; `norm_rel` = `mdb_apx/ssq/x.ssq`),
  `find_all_modfile`. Host `std::fs` on repo-relative paths (`data/...`,
  `data_mods/<mod>/...`) is the established off-game-thread pattern
  (`fast_bootup/identity.rs:57`).
- **Chunk-header walker exists:** `src/core/ssq/` (`ssq_chunk`), used by
  `chart_length` and `note_types_expansion`; `scripts/validate_musicdb.py:138`
  has the Python twin (type-3 `param2` set). Level = `param2 >> 8` ∈
  `{04,01,02,03,06}` for B/b/D/E/C; low byte `14`/`18` = single/double.

## Things that change the idea

1. **No musicdb consultation is needed.** The game only ever calls the builder
   for songs already in its music DB (the boot pass iterates the DB; play
   sequences pass DB entries' basenames). "Only if present in musicdb" is
   satisfied structurally by hooking the builder — the resolver just answers the
   question it is asked.
2. **`toho` falls out for free if the resolver is basename-opaque.** The play
   sequences rewrite the basename to `toho1..toho4` BEFORE calling the builder
   (RE doc §8). A directory-scan index keyed on the literal basename string
   returns "no split files" for `tohoN` (none exist) ⇒ unsplit `tohoN.ssq`,
   byte-identical to stock. A musicdb-driven index would have broken this.
3. **The failure mode of a wrong choice is loud**: `ME1529 FILE CORRUPTION
   ERROR` at boot when the DB says a chart exists but the chosen file has no
   notes. This is why the discovery rule must be content-checked (RE §6.1 rule
   A), not filename-only.
4. **Rule A reproduces stock on the installed data** except `sabm` Challenge
   (`_5` instead of `_3`; the two files' Challenge chunks are MD5-identical).
5. **Call volume**: ~7200 synchronous calls during `onInit`. Resolver must be a
   precomputed map; no per-call I/O.
6. **fast_bootup cache**: keyed on the registered path per item; a changed
   path = cache miss for that item = stock analysis + re-capture. No schema
   change, self-heals.
7. **Third-party hex-edited 20250805 DLLs** rewrote this function's prologue
   (`docs/binary_modpack_research.md` §10); the AOB will miss there and the mod
   must skip cleanly.

## Unknowns

- None blocking. The one open behavioral question is whether to also route
  `chart_length.rs` through the resolver (cosmetic: the LENGTH readout for a
  split song is computed from the base file's easy charts today).
