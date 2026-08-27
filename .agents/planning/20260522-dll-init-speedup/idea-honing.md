# Idea Honing: DLL Init Speedup

> Q&A log for refining requirements. Each Q is asked one at a time and the
> chosen answer is appended below. Earlier rough-idea framing was based on
> the hypothesis that "scanning is the bottleneck." Empirical profiling on
> 2026-05-22 substantially reframed the problem — see research files for
> the full picture.

## Empirical baseline (from diagnostic deploy on 2026-05-22)

Real numbers from a fresh boot of the diagnostic build on the user's
arcade cabinet (game version `MDX:J:F:A:2026042100`):

```
[init-prof] start                                                 0ms
[init-prof] module_load                          +1.0ms        +1ms
[init-prof] resolve_all                       +118.0ms      +119ms
[init-prof] musicdb_probe install              +5.6ms      +124ms
[init-prof] early_song_limit_patch              +59.8ms     +184ms  ← race-critical: PATCH LANDED
[init-prof] musicdb_parser ENTERED                          +748ms  ← parser ran 564ms after patch
[init-prof] resolve_derived                    +729.3ms     +913ms
[init-prof] avs_layeredfs                        +3.1ms     +917ms
[init-prof] widget_renderer                     +88.7ms    +1006ms
[init-prof] custom_options                     +159.1ms    +1172ms
[init-prof] enable/song-limit-expansion          +0.6ms    +1239ms
[init-prof] enable/autoplay                    +172.5ms    +1414ms
[init-prof] enable/folder-expansion          +1283.8ms     +2700ms
[init-prof] enable/webui-options             +2481.6ms     +5184ms
[init-prof] init_complete                      +0.4ms      +5186ms

scan_pattern: 47 calls, total 119ms, slowest 25ms
scan_pattern_all: 29 calls, total 828ms, slowest 47ms ("00 DC 49 00")
```

## Key findings

1. **The musicdb race is solved by reorder alone.** Moving the
   SongLimitExpansion patch to run immediately after `resolve_all`
   (before resolve_derived, services, mod registration) puts the patch
   in place at ~184ms. The parser runs at ~748ms — 564ms of slack.
   No thread suspension required.

2. **`resolve_derived` is the dominant scan-time phase** at 729ms,
   not `resolve_all` (119ms). Centralizing the AOB scanner would
   primarily target `resolve_derived`'s xref-walk and RTTI work, not
   the linear pattern pass.

3. **`scan_pattern_all` is much slower than `scan_pattern`** (828ms vs.
   119ms). The slowest pattern is `00 DC 49 00` — a 4-byte AOB
   starting with `0x00`, defeating the first-byte pre-filter. This
   is where SIMD or Aho-Corasick would help most.

4. **The 4+ second post-init delay is NOT scan-related.** It's two
   specific mod enable() functions:
   - `folder-expansion::enable()` = 1,284ms — reads ARC, extracts IFS,
     generates per-folder geo+AFP+BSI files on disk, generates
     cloned texture atlases. Output is to `./data_mods/custom_folders/`
     — idempotent across boots. **Cacheable.**
   - `webui-options::enable()` = 2,482ms — filesystem-scans
     `data/arc/custom/*` AND `./data_mods/*/data/arc/custom/*` for
     ~14 customize categories. Many syscalls, possibly parallelizable.

5. **Total init is ~5.2 seconds.** Race-critical portion is ~184ms.
   Everything beyond that is UX-quality.

## Questions

### Q1: Feature scope

Given the empirical data, what scope do you want this feature to cover?

**Decision**: Option 3 — race fix + slow-mod-enable speedups (cache
folder-expansion outputs, parallelize webui-options discovery). Skip
the centralized scanner architecture for now. Apply the race fix
*first* before the slow-mod fixes. Open question raised by user:
after the slow-mod fixes, is there an opportunity to *parallelize*
the mod enables themselves (run multiple `enable()` functions
concurrently after all scanning is complete)?

### Q2: Mod-enable parallelization?

Should the design include parallelizing mod `enable()` calls?

**Decision**: Cache first. The design phase covers race fix +
folder-expansion caching + webui-options speedup. Cross-mod
parallelization is deferred to a follow-up phase, evaluated only
after caching's actual impact is measured. Reasoning: caching
folder-expansion (1.3s → ~10ms warm) is a bigger lever than
parallelization (capped at ~2.5s by webui-options' single-mod
duration), and avoids the audit cost of `static mut` and
shared-service-lock conversion until we know we need it.

### Q3: Race-fix integration shape

How should the early-apply mechanism integrate with the existing
mod system?

**Decision**: Add an optional `early_apply(&mut self, &EarlyContext)
-> bool` method to the `Mod` trait. Default is no-op success. Mods
that need to land before service init implement it.
`SongLimitExpansionMod` becomes the first user. `lib.rs::init`
gains an `early_apply_all()` phase between `resolve_all` and
`resolve_derived`. `EarlyContext` is a stripped-down `ModContext`
that exposes only `game_module` and `signatures` (without derived
addresses, since we run before `resolve_derived`).

Reasoning: extensible (future race-sensitive hooks just opt in by
implementing the method), self-documenting (the trait signals
"this mod has time-critical setup"), and no awkward special case
in `lib.rs` for SongLimitExpansion specifically.

### Q4: early_apply config-gating + phase ordering

When `early_apply` runs, should it respect `mod-config.json`?
And where in the init sequence does the early_apply phase fit?

**Decision (config-gating)**: early_apply respects mod-config.json.
Config is loaded BEFORE the early_apply phase, and disabled mods'
early_apply is skipped. Reasoning: don't apply work the user has
explicitly disabled.

**Implication**: `mods::config::init()` (loading mod-config.json)
must move EARLIER in lib.rs::init — before the early_apply phase
rather than after resolve_derived. Config is just a JSON file load,
should be ~1ms; safe to do early.

**Decision (phase ordering)**: Place the early_apply phase after
`resolve_all`, before `resolve_derived`. Matches the empirically-
proven pattern from the diagnostic deploy. Insertion point:

```
DllMain spawn init thread
  → wait for gamemdx.dll
  → resolve_all()            ~120ms
  → mods::config::init()      ~1ms   ← moved earlier
  → EARLY APPLY phase         ~+60ms ← new
     → for each mod with early_apply:
         if config.is_enabled(mod.id()):
             mod.early_apply(&early_ctx)
     // race window closes here at ~180ms
  → resolve_derived()        ~730ms
  → services init            ~280ms
  → register mods + enable  ~3700ms
  → init complete           ~5200ms total
```

### Q5: folder-expansion caching strategy

How should folder-expansion's enable() output caching work?

**Decision**: Cache key = SHA256(serialized folder config) + mtime
of `data/arc/bm2d/select_music_folder_v3.arc`. Stored as
`.cache_meta.json` alongside generated outputs in
`./data_mods/custom_folders/`. On enable(): compute key, compare to
file. Cache HIT → skip generation. Cache MISS or missing meta →
regenerate and update the meta file.

Reasoning: detects both config changes (user added a custom folder)
AND game updates that ship a new source ARC (game patch alters the
underlying geo/AFP shapes). Detection cost is one stat() + one JSON
read on every boot — cheap. On warm boot saves ~1.27s (~1283ms →
~10ms).

**Schema**:
```json
{
  "version": 1,
  "config_hash": "<SHA256 of serialized FolderConfig>",
  "source_arc_mtime": <unix epoch seconds>
}
```

**Cache invalidation paths**:
- Config change → `config_hash` mismatch
- Game update → `source_arc_mtime` mismatch
- Schema change → `version` mismatch (wipe cache, regenerate)
- Manual: user deletes `.cache_meta.json` or the whole
  `./data_mods/custom_folders/` directory.

### NEW FINDING: webui-options 2.5s is an O(N²) atlas-rebuild bug

While investigating webui-options' 2.5s enable() time, found that
`custom_options::register_option` calls `asset_gen::register_label_for`
which **rebuilds the ENTIRE lang_eng atlas with every option
accumulated so far**, every time. With ~7 registrations, that's 7
progressively-larger atlas rebuilds (~O(N²) work).

The fix is structural, not a speedup:
1. **Defer atlas rebuild** — `register_label_for` just appends to
   `LABEL_REGISTRATIONS` without rebuilding.
2. **Add a `flush_atlas_rebuild()` API** — webui-options calls it
   once after registering all options.
3. Or: **debounce automatically** — rebuild fires after a short
   inactivity window (more complex; probably overkill).

Per-call cost (cabinet observed): ~360ms. With one rebuild instead
of seven: saves ~2.1s. Webui-options enable() drops from ~2.5s to
~400ms.

Cabinet asset directory sizes confirm this isn't a "many files"
problem — total customize asset directories: 7 dirs, ~878 .arc
files, but only 7 of those map to registered options (one per
non-empty CategoryDef in webui-options). The dir-scan part of
`discovery::discover_all` is fast; the slow part is the atlas
rebuild storm during `register_option` calls.

### Q6: webui-options atlas-rebuild fix shape

Given the O(N²) atlas-rebuild finding, how should we fix it?

**Decision**: Add `custom_options::flush_label_atlas()` API.
`register_label_for` becomes append-only (no rebuild). The flush
fires **once, automatically, in `lib.rs::init` after all mods are
enabled** — not per-mod. This avoids forcing every mod that
registers a single option to remember to flush, and avoids the
compounding problem when future mods register options too.

```
// lib.rs::init flow:
//   ...
//   register all mods + enable_with_config
//   register mod menu + enable
//   custom_options::flush_label_atlas()   ← here, ONCE
//   spawn splash-screen thread
```

**Callers audited**: `autoplay.rs:218` and
`webui_options/mod.rs:197`. Both register multiple options in a
loop. With the single-flush-at-end design, neither has to change —
they just keep calling `register_option` like today.

**Reason this lives in `lib.rs` rather than `mod_trait`**:
`mod_trait` shouldn't know about a specific service. `lib.rs` is
the orchestration layer that already knows about every service
(`avs_layeredfs`, `widget_renderer`, `judge_hook`, etc.); the
flush is just one more orchestration step.

### Q7: Profiling instrumentation in production

Should diagnostic profiling ship?

**Decision**: Drop the diagnostic. Remove `profiling.rs` and all
its call-sites in `lib.rs`/`scanner.rs`/`song_limit_expansion.rs`/
`mod_trait.rs`. Diagnostic served its purpose; production stays
clean. Stash@{0} has the instrumentation if we ever need to
re-measure.

### Q8: Success criteria

What does "done" look like for this feature?

**All four criteria must hold**:

1. **Race fix** — high-song-count `musicdb.xml` boots without
   crashing across multiple consecutive boots. Race window
   ≥ 200ms (currently ~564ms).
2. **folder-expansion warm-boot enable() < 50ms** — second-and-
   later boots with unchanged config. Currently 1283ms cold.
3. **webui-options enable() < 600ms** — after atlas-rebuild fix.
   Currently 2482ms.
4. **No mod-level regressions** — SongLimit loads songs,
   folder-expansion custom folders show, webui-options options
   appear in options menu. Smoke-test on cabinet.

Total init time on warm boot is implicitly bounded by the above
(rough math: 184ms early-phase + 729ms resolve_derived + ~280ms
services + ~50ms folder warm + ~600ms webui + ~200ms other mod
enables ≈ 2.0s; vs. 5.2s today). Not separately tracked; the
component-level criteria capture it.

### Q9: Late-binding-tolerant slow-mod ordering

Folder-expansion and webui-options are not race-critical — their
work needs to complete only before the player navigates to a
scene that uses their features (song-select for folder-expansion,
options-menu for webui-options), not before the game boots.

**Decision**: Reorder these mods to run last in
`enable_with_config`'s enable loop. No background thread —
keeps the change simple and synchronous. The combined effect of
reordering + caching + atlas-flush is enough.

Implementation: introduce a small `LATE_BINDING_MODS` set inside
`ModRegistry::enable_with_config`. Mods whose id is in that set
are enabled after all other mods (in their original relative
order). Initial members:
- `folder-expansion`
- `webui-options`

Reasoning:
- Reorder is a single small change to `enable_with_config`.
- Background-threading would let "init complete" log fire much
  earlier but adds shared-state-audit cost across mods.
- The reorder ensures fast mods (autoplay, fast-bootup,
  skip-intros, etc.) install their hooks before the slow mods
  begin their disk I/O, so their hooks are live within ~200 ms
  of mod-registration start. Today they wait behind webui-options'
  2.5s.

**Expected impact**: webui-options enable() drops ~2.5s → ~400ms.



