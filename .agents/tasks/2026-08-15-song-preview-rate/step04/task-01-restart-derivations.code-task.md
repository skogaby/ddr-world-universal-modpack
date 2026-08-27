# Task: Restart derivations (four preview signatures + loader-chain resolver)

## Description

The restart half's address foundation: four new `SignatureDefinition`s in
`src/core/signatures.rs` (validated exactly-once on all four supported
builds), a derivation that publishes the two vftable identity gates and
the two stock functions, and the preview module's loader-chain resolver
(`TS child → View → AudioPlayer → AudioLoader` with vftable identity
gates + field sanity checks). Nothing calls the restart yet (Step 5);
this step makes `preview::init` resolve and report the restart half's
availability fail-closed.

## Background

Steps 1–3 shipped the wheel-settle preview binding (deploy #1 partial
pass). Step 5's executor needs: `cue_handle_stop(handle)` (stop),
`sound_bank_create_router(file_id)` (re-create through the detoured
`wavebank_create`), and a validated path to the live
`sequence::AudioLoader` (re-arm `handle = −1`, watchdog reads
`failed`/`handle`). The four AOBs were extracted and validated
exactly-once on 20260324/20260421/20260616/20260721 on 2026-08-16 —
byte-level authority (patterns, offsets, per-build match table):
`research/preview-retrigger-re.md` §9.

Key repo facts: derivations live in `SignatureStore::resolve_derived`
(house style: `derive_player_option_table` for base-validated
RIP-decodes, `derive_audio_manager_and_play` for match-count
re-validation); the mod's `init(ctx)` has `ctx.signatures` (the
`real_speed::init` pattern) while `enable()` does not — derivation
resolution must ride `Mod::init`. `scene_manager::
current_transition_sequence()` exists; the `TS+0x58` child walk +
`flags & 0x24` dead-mask is the quick_logout pattern.

## Reference Documentation

**Required:**
- Design: .agents/planning/2026-08-15-song-preview-rate/design/detailed-design.md
  (§Components 5 step 1 — the validated chain; §Components 6 — the four
  derivations table + fail-closed policy; §Error Handling rows 1–2)
- RE: .agents/planning/2026-08-15-song-preview-rate/research/preview-retrigger-re.md
  (§1.1/§1.3 object chain + loader layout, §3 restart steps, §6 inventory,
  **§9 the validated signature matrix — the byte authority for this task**)

**Additional References (if relevant to this task):**
- src/core/signatures.rs `se_play`/`se_play_inner_body`/
  `player_option_ctx_load` entries + `derive_player_option_table` /
  `derive_audio_manager_and_play` (house style for patterns + derivations)
- src/services/song_rate/real_speed.rs `glue::init` (mod-init-time
  derivation stash pattern)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. `src/core/signatures.rs` — four new `SignatureDefinition`s (patterns
   verbatim from RE §9; house-style doc comments explaining what is
   literal and why):
   - `audio_loader_ctor` — yields `audio_loader_vftable` via
     `decode_rip_relative(match+3)`;
   - `selectmusic_view_ctor` — yields `selectmusic_view_vftable` via
     `decode_rip_relative(match+30)` (**the second LEA, `4C 8D 1D` — the
     first LEA at match+23 is an inner interface vftable, not the View's**);
   - `cue_handle_stop` — the match IS the function entry;
   - `sound_bank_create_router` — the match IS the function entry.
2. New `derive_preview_restart(&mut self)` called from `resolve_derived`:
   re-validates each pattern's match count == 1 (`get_all_matches`, the
   audio-family style — `resolve_all` takes first-match silently
   otherwise), RIP-decodes the two vftables, validates both land inside
   the module and that the AudioLoader vftable's slot-0 entry points
   in-module (a function pointer — the one-virtual-slot tick), then
   publishes `audio_loader_vftable` + `selectmusic_view_vftable` as
   derived names. ANY failure ⇒ WARN naming the piece, nothing published
   (the two function names stay resolved-or-missing on their own).
3. `preview.rs` restart-derivation stash + availability:
   - `init_restart(signatures: &SignatureStore) -> bool` (windows):
     stashes 4 `AtomicPtr`s (view vftable, loader vftable, stop fn,
     router fn); all-or-nothing — any missing ⇒ false, pointers stay
     null;
   - `restart_available() -> bool`;
   - the mod calls `init_restart` from `Mod::init` (beside
     `real_speed::init`) and reports at `enable()`: INFO when the restart
     half is available, one WARN naming it degraded otherwise (Step-3
     preview bindings continue either way — design R9/R11).
4. Loader-chain resolver (windows, `preview.rs`): `resolve_loader() ->
   Option<LoaderChain>` walking `scene==SONG_SELECT → TS →
   *(TS+0x58)` child (null + `flags&0x24` dead checks) → `View =
   *(child+0xB8)` with `*View == view_vftable` identity gate → `loader =
   *(View+0xC8+0x08)` with `*loader == loader_vftable` identity gate →
   snapshot `{loader, handle(+0x10), failed(+0x14), mode(+0x15),
   slot(+0x18), xwb_id(+0x08), xsb_id(+0x0C)}`. Struct offsets are
   compile-time constants gated by the vftable identities (design §Data
   Models).
5. Pure sanity predicate (host-tested, cfg-shared): `loader_sane(&
   LoaderSnapshot) -> bool` — `slot == 5 && mode == 1 && xwb_id >= 0 &&
   xsb_id >= 0` (the cue `_s`-suffix check stays in Step 5's executor
   where the cue string is read). Host tests cover accept + each reject.
6. Diagnostic (plan Step-4 demo, observable on the Step-5 deploy's log):
   the maintenance drain emits a ONE-SHOT INFO when a preview binding is
   live and `resolve_loader()` succeeds (chain proof), and a one-shot
   WARN when a preview binding is live but the chain fails while
   `restart_available()` — latched, never per-frame.
7. Both cfg targets compile; validator harness gains any new pure-module
   files; existing suites unchanged.

## Dependencies

- Steps 1–3 (preview binding end-to-end) — complete on the tree.
- RE §9 signature matrix — validated 2026-08-16 (this session).

## Implementation Approach

1. Signatures + `derive_preview_restart` (compile-checked; the four-build
   match evidence is already recorded — no live scan needed here).
2. `preview.rs`: stash/availability + `resolve_loader` + the pure
   predicate with host tests red→green.
3. Drain diagnostic + mod wiring (`Mod::init` + enable-time report).
4. Full gates: validator, windows check, whole-crate fmt, build.sh.
5. No cabinet deploy this step — the demo rides deploy #2 (Step 5).

## Acceptance Criteria

1. **Four-build patterns land verbatim**
   - Given the RE §9 patterns
   - When added as `SignatureDefinition`s
   - Then each pattern string byte-matches §9 (the validated form) and the
     doc comments name the literal layout facts they pin

2. **Fail-closed derivation**
   - Given any of: pattern missing, match count ≠ 1, RIP target outside
     the module, loader vftable slot-0 outside the module
   - When `derive_preview_restart` runs
   - Then nothing is published for the failed piece and one WARN names it

3. **All-or-nothing restart availability**
   - Given any of the four stash pointers unresolved
   - When `init_restart` runs
   - Then it returns false, `restart_available()` is false, and the
     enable-time report warns exactly once — while Step-3 preview binding
     behavior is unchanged

4. **Sanity predicate matrix**
   - Given snapshots varying slot/mode/file-id validity
   - When `loader_sane` runs (host tests)
   - Then exactly the `slot==5, mode==1, both ids ≥ 0` shape passes

5. **Gates**
   - validator green (≥ 232 + new tests), `cargo check --target
     x86_64-pc-windows-msvc` clean, whole-crate fmt, `./build.sh` clean

## Metadata

- **Complexity**: Medium
- **Labels**: song-rate, preview, signatures, derivation, RE
- **Required Skills**: Rust, AOB/derivation house style, the preview-pipeline RE
- **Generated By**: code-task-generator 2026-08-16
- **Source Plan**: .agents/planning/2026-08-15-song-preview-rate/implementation/plan.md
- **Plan Step**: Step 4: Restart derivations (signatures + loader-chain resolution)
