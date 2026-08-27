# Research: song-rate engine integration points for preview bindings

Date: 2026-08-15. Code-level survey of what the preview binding needs from
the existing streaming engine (`src/services/song_rate/`, `src/core/xact/`).

## 1. What is reusable unchanged

- **DSP**: `core/xact/stretch.rs` (WSOLA) and `core/xact/resample.rs`
  operate on a decoded PCM view of ONE entry — entry-agnostic; the
  `DspState::{Wsola, Resample}` seam in `generator.rs` selects per
  `Binding.preserve_pitch`. The `_s` entry is stereo MS-ADPCM on the same
  sample grid as the main entry (same file, same parser).
- **Serving**: the ring (absolute virtual offsets), the SPSC pending slots,
  the serve/poll dispatch, and `io_callback_hook`'s OVERLAPPED protocol are
  offset-range-driven — nothing in them assumes WHICH entry the ranges
  cover.
- **Loop mapping**: `virtual_bank.rs::map_loop` (half-up boundary mapping,
  one-frame clamp) already handles looped entries generically — if `_s`
  entries carry loop regions, the stretched plan maps them for free (the
  engine's loop-aware stream context then loops the stretched preview).
- **Detour composition**: the create router (`FUN_1801aa520`) calls the
  PATCHED `wavebank_create` entry, so re-creates made by the re-trigger
  pass through the existing create detour — the preview bind slots in as a
  new qualification branch, no new hooks.

## 2. What must be generalized or added

### 2.1 Planner: target-entry parameterization (`core/xact/virtual_bank.rs`)

`plan_virtual_bank` hardcodes `main_entry_index` as the stretch target and
the non-main entry as verbatim passthrough. The preview plan is the exact
inverse: target = the non-main (`_s`) entry stretched; main entry verbatim.
Mechanically: lift the target choice into a parameter
(`StretchTarget::{Main, Side}`), keeping the identity rule (main = entry
named like the bank) for classification. The verbatim main region serves
directly from the binding's private source copy (contiguous bytes, offset
arithmetic — no separate side buffer needed; today's append-only
`side_buffer` exists because the verbatim region had to be produced; for
the preview plan the verbatim bytes already sit in the source copy).

### 2.2 Binding: target-entry ranges (`binding.rs`)

`prepare_binding` wires ring coverage + regeneration targets to the main
entry's range. Needs the same `StretchTarget` parameter: ring covers the
TARGET entry's virtual range; regeneration targets follow. The private
source copy (preflight memcpy of the whole resident XWB) stays — it is the
generator thread's lifetime guarantee against FileManager row reuse. Cost
at song select: one ~8–30 MiB memcpy per wheel-settle create while armed
(≤ ~2.5 settles/s thanks to the game's own 0.4 s request debounce) —
measure on cabinet.

### 2.3 Registry: a preview slot (`binding.rs` + `io_callback_hook.rs`)

`BindingRegistry` holds ONE active slot consumed by `bound_verdict` (one
Acquire on the unbound hot path). Add an independent `preview` slot with
`publish_preview` / `with_preview` / retire coverage:
- `bound_verdict` miss path checks the preview slot (second Acquire only
  when the active slot missed).
- `retire_by_file` covers BOTH slots — the existing unregister prelude then
  retires preview bindings on natural teardown (wheel move, song confirm,
  scene exit) with no new code at those sites.
- Retired-list sweep (`registry().sweep` via the runtime drain) reclaims
  preview bindings identically (generator thread stop, ring free).

### 2.4 Create detour: preview qualification (`wavebank_hook.rs`)

After the gameplay `bind_may_qualify`/`qualify_bind` path declines (scene
25 never gameplay-qualifies — arming happens at 26), a preview branch:
- feature latched on (mod active + integration ready),
- current scene == SONG_SELECT (scene_manager),
- `dance_bank_song_code(path).is_some()` (excludes `custom_bgm_%04d` — the
  other slot-5 creates — and every named bank),
- controlling side per D3: exactly one side entered (`stage_records::
  side_entered`, the same source `classify_scene26` uses; both/none/
  unreadable ⇒ decline), desired ≠ 100, supported rate,
- ⇒ `prepare_binding(.., StretchTarget::Side, ..)` + `publish_preview`.
No lifecycle phases, no XactSlots, no Q31, no score ledger, no movie
policy — refusals fail open to a stock create with a drain-reported WARN.
No pre-arm state: qualification reads the desired atomics at create time,
for wheel-settle creates and re-trigger re-creates alike.

### 2.5 Re-trigger executor thread choice

The re-trigger calls game APIs (cue stop, unregister, create, loader field
writes) ⇒ **game thread only**. The runtime drain is a background thread —
NOT usable as executor. `input_manager::poll` (game thread, every frame) is
the natural executor: one relaxed atomic load per frame when idle; when the
debounced deadline (150 ms after the last option tick, D4) passes and scene
== 25, run the §3 sequence from `research/preview-retrigger-re.md` inline
(few-ms one-frame cost at a menu scene — same class as the gameplay bind
inside the loading screen's create).

### 2.6 Change-callback plumbing (`mods/song_playback_speed.rs`)

`on_song_speed_change` / `on_preserve_pitch_change` additionally stamp a
monotonic "preview refresh requested at T" cell (two atomic stores —
callback-contract-legal). The 150 ms debounce compares against it.

## 3. Interactions audited

- **Gameplay arm**: preview binds only at scene 25; gameplay qualification
  runs FIRST in the detour and scene-26 arming is untouched. On song
  confirm the preview bank unregisters (cabinet-proven, research §4) ⇒
  preview binding retires via the existing prelude BEFORE the gameplay
  create. Belt-and-braces: the runtime scene callback force-retires the
  preview slot on any transition leaving SONG_SELECT.
- **Training mode**: its identity-passthrough arm and scrub machinery are
  gameplay-generation-scoped; preview bindings never touch the lifecycle,
  so no interaction. `selected_song` publication continues on every create
  (armed or not) — unchanged.
- **Quick logout / redirects**: leave scene 25 ⇒ scene-callback retire ⇒
  stock teardown.
- **Score guard**: previews are not gameplay; no taint, no ledger — the
  gameplay path's containment is untouched (the preview binding never
  publishes Q31 or commits a transaction).
- **Movie policy**: not involved (no background movie at song select).
- **`fast-confirm` race**: player confirms the song while a re-trigger is
  pending ⇒ the executor's scene gate (must still be 25) + loader identity
  checks decline; the pending flag clears on scene exit.
