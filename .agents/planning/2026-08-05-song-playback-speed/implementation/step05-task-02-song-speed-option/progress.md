# Progress: Step 5 Task 2 — Add the Player-Facing SONG SPEED Option

## Checklist

- [x] Slice A: `lifecycle::snap_rate_percent` (clamp 25..=175 + half-up snap to 5) —
      host red/green; snapped output proven always in-domain
- [x] Slice B: framework availability — `RegisteredOption.available` (default true,
      mutated under the STATE lock), `FrameworkState::set_available`,
      `custom_options::set_option_available(id, bool)` (unknown id WARN no-op),
      strict `row_injection_available()` (`is_available && rows::is_scalar_ready &&
      builder_hook::is_ready && FILTER_READY` latch), builder-hook per-open snapshot
      filter (silent non-injection; open forms untouched by construction; handles/
      ordering invariants preserved — tuples carry true registry indices)
- [x] Slice B tests: `custom_options/availability_tests.rs` against a LOCAL
      `FrameworkState` (register→default-available→hide→still-registered/valued→
      re-show same handle; unknown-id no-op) + harness mini-module (api + registry
      only) + validator file-list entries; `#[cfg(test)]` declaration in
      custom_options/mod.rs for consistency
- [x] Slice C: `src/mods/song_playback_speed.rs` (id `song-playback-speed`) —
      scalar row `song_speed` (25..=175, step 5, coarse 10, Integer, default 100,
      `PersistMode::Full`, load transform = snap, on_change = snap →
      `song_rate::runtime::set_desired_percent` — one atomic store, no I/O/game API);
      enable gates on `row_injection_available() && integration_ready()` (self-
      disable + WARN otherwise; `is_active` reports it); Duplicate-on-reenable =
      success + registry-value reseed; disable = row unavailable + desired → 100
      (active attempt untouched — lifecycle owns it). New
      `song_rate::runtime::integration_ready()` (BOOT_READY && live score-guard
      full sanitization). Registered in mods/mod.rs + lib.rs instance list (before
      the atlas flush).
- [x] Slice D: `("song_speed", "SONG SPEED")` in gen_option_labels.py → generated
      `seop_item_song_speed.png` (only the new PNG changed — rendering is
      deterministic); mod-config.json gains `"song-playback-speed": true` and
      `song_speed` in the `custom_options.row_order` example
- [x] Validate: five gates green (156 host tests: 153 + snap + 2 availability);
      `cargo check` windows target 0 errors 0 warnings; release build OK
- [x] Records updated; NO commit; NO deployment

## Record

- TDD red: validator failed on `no field available` / `no method set_available`
  (compile-red for the framework slice) after the availability tests + harness
  mini-module landed first; snap tests written before `snap_rate_percent` existed.
- Green: 156 passed. Post-fmt re-verified.
- The persist_transform inverse contract holds: identity save ∘ snap load is the
  identity for every value the option can hold via the UI; the snap only rewrites
  out-of-domain legacy/hand-edited values.

## Deviations

- Host-test scope: atlas-flush ordering, live form-rebuild visibility flips, and
  live P1/P2 isolation are cargo-check + task-04 cabinet legs — mods/* and the
  windows-heavy framework glue are not harness-compiled (consistent with every
  prior mod). The host-testable kernels (normalization, registry availability
  semantics) are covered; the eligibility matrix was already green from task 01.
- `row_injection_available()` deliberately excludes label-asset presence: a
  missing PNG degrades to a blank label by framework design (non-fatal), and the
  committed `seop_item_song_speed.png` makes it moot.
- `mod-config.json`'s `song_playback_speed.cache_limit_gib` example is left to
  task 04's config-surface pass (the key is optional; absence = default 10 GiB).

## Status

Complete pending maintainer commit. Next: task 03 (bemani-buddy `mod_song_speed`
backend — JSON model + codegen + migration + sqlx cache, in the sibling repo).
