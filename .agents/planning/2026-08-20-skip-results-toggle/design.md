# Skip Results on Fast Exit — Toggle (light PDD)

**Date:** 2026-08-20 · **Status:** designed, implementing
**Mod touched:** `quick-restart-or-fail` · **New option id:** `skip_results_fast_exit`

## Idea

Players want the choice to still see the stage results screen (score breakdown up
to the drop-out point) when they quick-fail with 3 during gameplay, instead of the
instant cut to song select. New player-facing bool row on the injected MODS tab:
**"SKIP RESULTS ON FAST EXIT"**, default **ON** (= today's instant-cut behavior).

## Decisions (settled with maintainer 2026-08-20)

| # | Decision |
|---|----------|
| D1 | Governing side = **the side that pressed 3** (`InputEvent.player`); cabinet-wide effect follows the presser's preference. |
| D2 | OFF path = **natural fail flow** (`fail_song(None)`): 0.25 s fade + FAILED banner + results screen + natural tail. Direct-to-results `finish(DPS, 0x1E)` was RE-investigated and **rejected** — see research notes below. |
| D3 | `PersistMode::Full` (wire `mod_skip_results_fast_exit`); bemani-buddy migration in scope. |
| D4 | Textures generated via `option_strings.py` + `gen_option_labels.py`, run locally. |
| D5 | Scope: quick FAIL only. Quick restart (press 1) and quick logout (triple-9) unaffected. |
| D6 | Score taint (`score_guard::set_quick_fail`) applies in BOTH modes — the play is incomplete either way; the results *display* reads live state and is unaffected by save suppression. |

## RE findings — why direct-to-results was rejected

(Recorded in full in `docs/quick_restart_fail_speedup_research.md` §10.)

- The per-stage record that ResultSequence displays is only written by the
  **result commit** — a vfunc at GamePlayActor vtable **+0x28** (20260721:
  `FUN_18005d970`, vtable @ `0x180360d68`; 20260526: `FUN_18005d180`, vtable @
  `0x18035fd68`). It runs during the natural song-end machinery, NOT before.
  The song-select commit *zeroes* the record at selection, so a mid-song
  `finish(DPS, 0x1E)` would show an all-zero results screen — defeating the
  feature's purpose.
- Replicating the natural machinery by hand means: per-actor commit calls, the
  stage bump (`GameWork+0xC` INC @ `0x180058c29`, guarded `+0x59`/`+0x5a`), the
  msg `0x1053` broadcast, per-actor `+0x210` vfunc calls, the song stop
  (`FUN_1801aa7c0`), and an `MDX1529` error-handler call when total judges == 0
  (quick fail before the first judged note). Every divergence needs cabinet
  validation; the known failure mode of this machinery is a hard limbo.
- The natural-death path (`force_game_over`) triggers ALL of it for free and is
  cabinet-proven (it is today's predicate-fail fallback and the stock gauge-death
  flow).

## Design

### DLL (`src/mods/quick_restart_or_fail.rs`)

- `static SKIP_RESULTS: [AtomicBool; 2]`, default **true** (= ON).
- `enable()`: register `RegisterSpec::bool_toggle("skip_results_fast_exit")
  .default_value(1).on_change(skip_results_on_change)` when
  `custom_options::is_available()` (announcer_mute pattern; registration failure
  logs one WARN, gesture behavior falls back to skip = default ON).
- `on_input_event` passes `event.player` → `trigger_fail(side)`.
- `trigger_fail(side)`: after `set_quick_fail()`, if `!SKIP_RESULTS[side]` →
  `fail_song(None, "quick-fail (show results)")` and return. Otherwise the
  existing ladder (session predicate → fast finish → redirect fallback) runs
  unchanged.
- `disable()`: nothing to reset — the input callback is removed, so the atomics
  are unreachable; the row keeps its persisted value.

### Textures

`scripts/option_strings.py`: `skip_results_fast_exit` in ITEM_STRINGS + two
`PreviewSpec`s (off/on, WIDE) with en/ja/ko copy. Regenerate all three language
sets with `scripts/gen_option_labels.py`.

### Backend (sibling `bemani-buddy` repo)

New nullable TEXT/INT column `opt_mod_skip_results_fast_exit` + migration
(pattern: migrations 013/014/016), stored VERBATIM; wire field
`mod_skip_results_fast_exit`.

## Validation

- `cargo check` / `cargo fmt` / `./build.sh` clean.
- Cabinet: toggle OFF → press 3 mid-song → fade + FAILED banner + results screen
  shows partial score; natural return to song select; no per-stage save for the
  tainted play. Toggle ON → today's instant cut. Versus: presser's setting wins.
