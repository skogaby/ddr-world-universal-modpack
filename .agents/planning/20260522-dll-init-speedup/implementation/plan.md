# Implementation Plan: DLL Init Speedup

> Source-of-truth for the design is `../design/detailed-design.md`.
> This plan converts that design into a sequence of demoable
> implementation steps. Each step ends with a deploy + observe
> verification because this codebase has no unit-test harness
> (CLAUDE.md explicitly notes: "validation is done by deploying
> the DLL and observing log output / visual behavior").
>
> **Apply the steps in order.** Each step's success unblocks the
> next; each step is small enough to revert cleanly if its
> deploy-test surfaces a regression.

## Checklist

- [ ] **Step 1**: Add the `Mod::early_apply` trait method + `EarlyContext`
- [ ] **Step 2**: Move `mods::config::init()` earlier in `lib.rs::init`
- [ ] **Step 3**: Implement `SongLimitExpansionMod::early_apply` + flag-based no-op in `init`/`enable`
- [ ] **Step 4**: Reshape `lib.rs::init` to construct mods early + run `early_apply` phase
- [ ] **Step 5**: Reorder slow mods last in `enable_with_config`
- [ ] **Step 6**: Add `custom_options::flush_label_atlas()` API + remove rebuild from `register_label_for`
- [ ] **Step 7**: Wire `flush_label_atlas` call into `lib.rs::init`
- [ ] **Step 8**: Add folder-expansion enable() output cache
- [ ] **Step 9**: Final acceptance test on cabinet with high-song-count `musicdb.xml`

---

## Step 1: Add the `Mod::early_apply` trait method + `EarlyContext`

**Objective**: Expand the `Mod` trait so any mod can opt into a
race-critical pre-service-init pass. Default implementation is a
no-op success — every existing mod compiles unchanged.

**Files touched**:
- `src/mods/mod_trait.rs` — add `EarlyContext` struct, add trait
  method with default body.

**Implementation guidance**:
- `EarlyContext` exposes only `&GameModule` and `&SignatureStore`.
  Mirror `ModContext`'s shape but with a different name to make
  the calling-phase clear.
- Add docstring on `early_apply` referencing the use case
  (race-critical setup before service init) and the constraint
  that derived signatures aren't yet resolved.

**Test requirements**:
- `cargo check --target x86_64-pc-windows-msvc` passes clean.
- No existing mod implementation is forced to change (compile-
  check confirms the default body is sufficient).

**Integration**:
- This step adds dead code intentionally — no caller invokes the
  new method yet. The next steps wire it up.

**Demo**:
- `cargo check` succeeds.
- `git diff src/mods/mod_trait.rs` shows the new trait method and
  context struct.

---

## Step 2: Move `mods::config::init()` earlier in `lib.rs::init`

**Objective**: Load the mod-config.json before the race-critical
phase so `early_apply` can be config-gated.

**Files touched**:
- `src/lib.rs` — move the `mods::config::init();` call from its
  current position (between `resolve_derived` and `avs_layeredfs`)
  to immediately after `resolve_all`. The new order is:
  1. wait_for_game_module
  2. resolve_all
  3. mods::config::init() **(moved here)**
  4. resolve_derived (unchanged)
  5. service inits (unchanged)
  6. mod registration + enable (unchanged)

**Implementation guidance**:
- Verify `mods::config::init` has no dependencies on
  `signatures` or any service. The current implementation reads
  a JSON file from disk; this should be safe to do as the
  second-earliest step.
- Update the inline comment on the `mods::config::init();` line
  to explain the early-load contract ("must precede the
  early_apply phase so mods can be config-gated").

**Test requirements**:
- `cargo check` passes.
- Build the DLL via `./build.sh`.
- Deploy via `./scripts/deploy.sh`. Boot the game once.
- Inspect `log.txt` for the `Config: loaded mod-config.json` line
  appearing **before** the `Resolving derived addresses...` line.
- Confirm all mods register/enable as expected (every mod that
  was enabled before is still enabled).

**Integration**:
- No mods change behavior. This step's purpose is purely
  reordering. Step 4 will call `early_apply_all` between this
  and `resolve_derived`.

**Demo**:
- `log.txt` shows `Config: loaded mod-config.json` immediately
  after the signature-scan summary line, before any service init
  log.

---

## Step 3: Implement `SongLimitExpansionMod::early_apply` + flag-based no-op in `init`/`enable`

**Objective**: Give `SongLimitExpansionMod` a working
`early_apply` body that scans for the 6 patch sites and applies
the buffer-expansion writes immediately, plus internal
`early_applied: bool` so subsequent `init()`/`enable()` skip
duplicated work.

**Files touched**:
- `src/mods/song_limit_expansion.rs` — add `early_applied: bool`
  field, implement `early_apply`, gate `init` and `enable` on
  the flag.

**Implementation guidance**:
- The `early_apply` body is essentially today's `init` (scan +
  verify) followed by today's `enable` (write 0x80 over 0x10):
  one combined pass that scans, verifies, writes, and records
  the `PatchSite { addr, original }` entries so `disable()` can
  still roll back.
- `SongLimitExpansionMod::new()` initializes `early_applied: false`.
- `init()` checks the flag first: if set, log "skipping scan —
  early-patch already applied" and return `true` without re-
  scanning. Otherwise fall through to today's body (this fallback
  exists for the runtime toggle case where the user disables the
  mod via mod-menu and re-enables it later).
- `enable()` checks the flag: if set, the bytes are already 0x80
  in memory, return without writing.
- `disable()` is unchanged — it always writes the original bytes
  back. (After disable, `early_applied` should remain `true` —
  it just records "we made the original observation"; the sites
  are populated and disable still works.)

**Test requirements**:
- `cargo check` passes.
- This step compiles and the mod's behavior is unchanged because
  no caller invokes `early_apply` yet — `init()` and `enable()`
  still run the full scan-and-write logic via the fallback
  branch.
- Build, deploy, boot. Confirm `log.txt` shows
  `SongLimitExpansion: found 6 patch sites (1MB → 8MB)` and
  `SongLimitExpansion: enabled — XML buffers expanded to 8MB`
  exactly as before.

**Integration**:
- No behavior change yet from the user's perspective. The
  mod's plumbing is now ready for Step 4 to call `early_apply`
  on it.

**Demo**:
- `log.txt` shows song-limit expansion working as today.
- `git diff` shows only `early_applied` field, the new
  `early_apply` method, and the early-return guards in `init`
  and `enable`.

---

## Step 4: Reshape `lib.rs::init` to construct mods early + run `early_apply` phase

**Objective**: This is the load-bearing step that fixes the
musicdb race. Construct the mod instances after `resolve_all`,
run `early_apply` on each one (gated by config), then continue
with the rest of init unchanged. The mods are then `move`d into
the registry via `register()` later in init.

**Files touched**:
- `src/lib.rs` — major reshape of the `init()` function body.
- `src/mods/mod.rs` — re-export `EarlyContext` if needed for
  `lib.rs` to construct it.

**Implementation guidance**:
- After `resolve_all` and `mods::config::init()`:
  ```rust
  let mut mods_to_register: Vec<Box<dyn Mod>> = vec![
      Box::new(mods::song_limit_expansion::SongLimitExpansionMod::new()),
      Box::new(mods::hello_world::HelloWorldMod::new()),
      Box::new(mods::fast_bootup::FastBootupMod::new()),
      Box::new(mods::skip_intros::SkipIntrosMod::new()),
      Box::new(mods::timer_freeze::TimerFreezeMod::new()),
      Box::new(mods::autoplay::AutoplayMod::new()),
      Box::new(mods::series_expansion::SeriesExpansionMod::new()),
      Box::new(mods::folder_expansion::FolderExpansionMod::new()),
      Box::new(mods::note_types_expansion::NoteTypesExpansionMod::new()),
      Box::new(mods::webui_options::WebUiOptionsMod::new()),
  ];
  let early_ctx = EarlyContext {
      game_module: &game_module,
      signatures: &signatures,
  };
  let mod_config = mods::config::get()
      .map(|c| c.mods.clone())
      .unwrap_or_default();
  for m in &mut mods_to_register {
      let id = m.id();
      let should_run = mod_config.get(id).copied().unwrap_or(true);
      if !should_run {
          log_info!("Mod '{}' early_apply skipped (disabled in config)", m.name());
          continue;
      }
      let _ = m.early_apply(&early_ctx);
  }
  ```
- Then `signatures.resolve_derived()`, services, etc. — unchanged.
- Later in init, where `reg.register(...)` calls live today,
  replace the inline `Box::new(...)` instantiations with
  `for m in mods_to_register { reg.register(m, &ctx); }`.
  Note: this `for` consumes the vec, so it must be the last
  reference to it.
- The mod-menu mod is registered separately as today (after the
  enable-with-config call).

**Test requirements**:
- `cargo check` passes.
- Build, deploy, boot.
- `log.txt` should show:
  - `SongLimitExpansion: found 6 patch sites` and
    `SongLimitExpansion: enabled — XML buffers expanded to 8MB`
    occurring **immediately after** `resolve_all` finishes,
    BEFORE `Resolving derived addresses...`.
  - When the mod registers later via `register()`, `init()` logs
    `SongLimitExpansion: skipping scan — early-patch already
    applied` (or similar — match the wording from Step 3).
  - When `enable_with_config` runs, the song-limit-expansion
    mod's `enable()` either no-ops or doesn't fire (per Step 3
    flag logic).
- All other mods continue to register and enable as before.

**Integration**:
- This is the race fix. After this step, with high-song-count
  musicdb, the cabinet should boot reliably. (User notes they
  don't have a high-song-count musicdb locally; final acceptance
  test in Step 9 uses one on the cabinet.)

**Demo**:
- `log.txt` shows song-limit patches landing in the early phase,
  before service init.
- Stock-songs boot still works (no regressions).
- Code reviewer can trace the `init()` flow and see the
  early_apply phase between resolve_all and resolve_derived.

---

## Step 5: Reorder slow mods last in `enable_with_config`

**Objective**: Make folder-expansion and webui-options enable
*after* all other mods, so fast mods install their hooks
without waiting on slow disk I/O.

**Files touched**:
- `src/mods/mod_trait.rs` — add `LATE_BINDING_MODS` const slice
  and partition logic in `enable_with_config`.

**Implementation guidance**:
- Add at module scope:
  ```rust
  /// Mods whose enable() does substantial late-binding-tolerant
  /// work (filesystem I/O, atlas generation) that doesn't need to
  /// complete before the game's first frame. Enabled after all
  /// other mods to keep faster hooks online sooner.
  const LATE_BINDING_MODS: &[&str] = &[
      "folder-expansion",
      "webui-options",
  ];
  ```
- In `enable_with_config`, partition the `ids` vec into fast vs.
  late by checking against `LATE_BINDING_MODS`. Iterate fast,
  then late, with the existing `mod-menu` skip and `should_enable`
  config check intact.

**Test requirements**:
- `cargo check` passes.
- Build, deploy, boot.
- `log.txt` should show, in `enable_with_config`'s phase:
  - "Mod enabled: SongLimitExpansion" (or similar fast mods)
    appearing before
  - "Mod enabled: FolderExpansion" and
    "Mod enabled: WebUiOptions" at the end.
- Visual smoke test: the song-select scene still has custom
  folders, the options menu still has custom rows.

**Integration**:
- No behavior change to the mods themselves; just reorder.
- Compounds with Steps 6–8: when `webui-options` enables LAST
  and the atlas-flush moves to a single late call, the slow
  work all happens at the tail of init.

**Demo**:
- `log.txt` ordering of `Mod enabled: ...` lines confirms slow
  mods land last.

---

## Step 6: Add `custom_options::flush_label_atlas()` API + remove rebuild from `register_label_for`

**Objective**: Eliminate the O(N²) atlas-rebuild storm in
`register_option`. `register_label_for` becomes append-only;
a new `flush_label_atlas` does the rebuild work once.

**Files touched**:
- `src/services/custom_options/asset_gen.rs` — remove the
  `rebuild_lang_eng_atlas` call from `register_label_for`,
  add a new `pub fn flush_label_atlas() -> bool`.
- `src/services/custom_options/mod.rs` — re-export
  `flush_label_atlas` so callers use
  `custom_options::flush_label_atlas()`.

**Implementation guidance**:
- The new `flush_label_atlas` body is the same XML-load + call to
  `rebuild_lang_eng_atlas` that today's `register_label_for`
  does.
- After this step, the atlas is **never** rebuilt automatically
  during option registration. If no caller calls
  `flush_label_atlas`, custom-option labels won't show. Step 7
  wires the call in.
- Confirm by `cargo check` and inspecting the diff that the only
  call site for `rebuild_lang_eng_atlas` from inside
  `register_label_for` is removed; the function itself stays
  private.

**Test requirements**:
- `cargo check` passes.
- This step deliberately leaves the system in a transient broken
  state (atlas labels don't render until Step 7 lands the flush
  call). Don't deploy at this checkpoint; combine with Step 7
  for the deploy verification.

**Integration**:
- Sets up Step 7's wiring.

**Demo**:
- Code review shows `register_label_for` has only the dedup
  push-to-Vec body; no rebuild calls.
- `flush_label_atlas` exists and is re-exported from the
  `custom_options` module.

---

## Step 7: Wire `flush_label_atlas` call into `lib.rs::init`

**Objective**: Land exactly one atlas-rebuild after every mod's
enable has finished, restoring custom-option label rendering.

**Files touched**:
- `src/lib.rs` — add `custom_options::flush_label_atlas();` after
  the mod-menu register/enable, before the splash-screen thread
  spawn.

**Implementation guidance**:
- Insertion point:
  ```rust
  // Existing: register + enable mod-menu...

  // ★ NEW: one-shot atlas flush after every option-registering
  // mod has run its enable.
  custom_options::flush_label_atlas();

  // Existing: splash screen thread spawn...
  ```
- The function returns a bool; ignore it (logging happens inside).
- Steps 6 + 7 land together as one logical change for testing.

**Test requirements**:
- `cargo check` passes.
- Build, deploy, boot.
- `log.txt` should show **exactly one**
  `custom_options/asset_gen: rebuilt lang_eng atlas ...` log line,
  appearing **after** `Mod enabled: WebUiOptions`. Today's log
  shows multiple rebuilds during webui-options enable; the new
  log should show one rebuild after the slow mods finish.
- Visual smoke test: open the options menu in the game; verify
  that custom-option rows display their labels correctly.
- Cabinet timing comparison: webui-options' enable time should
  drop dramatically (per design success criterion: < 600ms).
  Without the diagnostic re-applied, you'll observe this as
  the game reaching the splash screen visibly faster.

**Integration**:
- Closes the atlas-storm regression introduced in Step 6.
- Compounds with Step 5's reorder: late mods now run quickly
  (no atlas work during their enable) and the rebuild happens
  once at the end.

**Demo**:
- `log.txt` shows exactly one atlas-rebuild log line, near the
  end of init.
- Game's options menu shows custom-option labels correctly.

---

## Step 8: Add folder-expansion enable() output cache

**Objective**: Skip the ~1.3s on-disk asset regeneration in
`folder-expansion::enable` when the cached outputs are still
valid for the current config + source ARC mtime.

**Files touched**:
- `src/mods/folder_expansion.rs` — add `CacheMeta` type, helper
  functions (`compute_cache_key`, `cache_is_valid`,
  `write_cache_meta`, `cache_meta_path`), and gate the asset-
  regeneration block on the cache check.
- `src/mods/folder_expansion.rs` (or wherever `FolderConfig` is
  defined) — add a helper for canonical serialization that
  produces stable bytes for hashing.
- `Cargo.toml` — add `sha2 = { version = "0.10", default-features = false }`
  if not already present (audit during implementation).

**Implementation guidance**:
- `CacheMeta` has three fields: `version: u32`, `config_hash:
  String` (SHA-256 hex), `source_arc_mtime: u64` (Unix epoch
  seconds).
- `compute_cache_key`: serialize `FolderConfig` to canonical
  bytes (sort any HashMap-backed collections), SHA-256 the bytes,
  stat the source ARC for mtime. Return `Option<CacheMeta>` so
  failures degrade to "regenerate."
- `cache_is_valid(want)`: read `.cache_meta.json`, parse, compare
  field-by-field. Any failure → return `false`.
- `write_cache_meta`: serialize as pretty JSON, write
  best-effort. Log a warning on write failure but continue.
- Gate **only** the asset-generation block (the section that runs
  `generate_custom_assets(config)` and
  `mod_paths::init_mod_paths()`). The hook installs and ctor
  patches **always** run — they patch live game memory, which
  isn't persisted.

**Test requirements**:
- `cargo check` passes.
- Build, deploy. Boot **twice**:
  1. First boot ("cold"): no `.cache_meta.json` exists. Expected
     log: `FolderExpansion: cache MISS, regenerating assets`.
     Asset files appear in `./data_mods/custom_folders/...`.
     `.cache_meta.json` is written.
  2. Second boot ("warm"): `.cache_meta.json` exists and matches.
     Expected log: `FolderExpansion: cache HIT, skipping asset
     regeneration`. Asset files are unchanged. enable() should
     complete much faster.
- Visual smoke test: custom folders still appear in song-select
  on both boots.
- Edit `mod-config.json` to change a custom folder definition,
  reboot. Expected log: `cache MISS, regenerating` (config_hash
  mismatch).
- Touch the source ARC (or wait for a game update), reboot.
  Expected log: `cache MISS, regenerating` (mtime mismatch).

**Integration**:
- Final piece of the design. After this step, all four success
  criteria from R5 should be measurable on the cabinet.

**Demo**:
- Two consecutive boots show MISS then HIT in `log.txt`.
- Custom folders appear in song-select on both boots, with no
  visual difference.

---

## Step 9: Final acceptance test on cabinet with high-song-count `musicdb.xml`

**Objective**: Validate the four success criteria from R5
against the production build on a cabinet with the modpack's
real >2000-song `musicdb.xml`.

**Files touched**: None — this is a deploy-and-observe step.

**Implementation guidance**:
- Restore the user's high-song-count `musicdb.xml` to the
  cabinet's data directory.
- Boot the game ≥ 5 consecutive times. Each boot must reach the
  song-select scene without crashing.
- For one of those boots, re-apply the diagnostic `[init-prof]`
  instrumentation (from `git stash@{0}`) and capture the log to
  measure:
  - early_apply landed at < 200 ms? (Should be ~180ms based on
    Step 4's deploy.)
  - folder-expansion warm enable() < 50 ms?
  - webui-options enable() < 600 ms?
- Cabinet smoke test:
  - Song-select: custom folders appear, can be selected, songs
    inside them load.
  - Options menu: custom options appear with labels, can be
    changed, persist correctly.
  - SongLimit: enter a stage with a song from the high-count
    musicdb that's beyond the original 2200-song limit. Verify
    it plays.

**Test requirements**:
- All four criteria from R5 satisfied.
- No mod-level regressions.
- No new error-level log lines compared to current production.

**Integration**:
- Closes the feature. After this step, the diagnostic
  instrumentation is dropped (per Q7) and the production build
  is the final state.

**Demo**:
- Five clean boots with the high-count musicdb.
- Captured log file showing the timing measurements within
  spec.
- Cabinet smoke-test photos / video showing each affected mod
  working correctly.

---

## Notes on rollback

If any step's deploy reveals a regression that can't be
diagnosed quickly, revert to the previous commit and resume
debugging on a topic branch. The steps are sequenced to be
independently revertable — each one leaves the codebase in a
shippable state once its deploy verification passes.

The most-fragile step is **Step 4** (lib.rs reshape) — it
touches the most code and is the load-bearing step for the
race fix. If it breaks anything, the workaround is to revert
just `src/lib.rs` to the previous commit; the new
`Mod::early_apply` trait method (Step 1) and
`SongLimitExpansionMod::early_apply` body (Step 3) are dead
code without it but remain compilable.
