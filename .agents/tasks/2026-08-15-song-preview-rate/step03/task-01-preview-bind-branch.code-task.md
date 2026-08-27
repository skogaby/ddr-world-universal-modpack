# Task: Preview qualification + create-detour branch (wheel-settle rate previews)

## Description

The feature's core end-to-end behavior: while the controlling side desires
a non-100 % SONG SPEED, every song-select preview-bank create binds a
`StretchTarget::Side` virtual bank and the preview plays at rate in the
selected DSP mode. Adds the `preview` policy module (pure qualification,
feature gate, scene-exit retire), the create-detour preview branch, drain
reporting, and the `song-playback-speed` mod wiring. Ends in cabinet
deploy #1.

## Background

Step 1 gave the planner/binding `StretchTarget::Side`; Step 2 gave the
registry the preview slot and the io detours the two-slot routing. This
step publishes preview bindings: inside the existing create detour's bind
closure (pre-original — the header read must already serve the virtual
bank), after the gameplay path resolves to Stock, a preview branch
qualifies and publishes. The gameplay transaction NEVER sees the preview
bind (outcome stays Stock: no slot expose, no Q31, no score/movie/
lifecycle involvement — design R8).

Key repo facts: `scene::SONG_SELECT == 25`; `scene_manager::
current_scene()` is an atomic read; `stage_records::side_entered(side)`
exists; `runtime::desired_percent/desired_preserve_pitch` are the option
rows' atomics; the maintenance drain spawns lazily on the first gameplay
arm — preview binds must also ensure it (sweep/reporting); the validator
harness mounts song_rate files explicitly and must gain `preview.rs` (+
tests).

## Reference Documentation

**Required:**
- Design: .agents/planning/2026-08-15-song-preview-rate/design/detailed-design.md
  (§Components 4–5, §Architecture Flow 1, §Error Handling, §Testing C1–C7)

**Additional References (if relevant to this task):**
- .agents/planning/2026-08-15-song-preview-rate/research/engine-integration.md §2.4–§2.5, §3

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. NEW `src/services/song_rate/preview.rs`:
   - pure `qualify(inputs) -> Option<PreviewBindRequest>` (host-tested):
     declines on feature-off, scene ≠ SONG_SELECT, non-dance path, any
     unreadable entered flag (fail closed), zero or two entered sides
     (versus ⇒ stock, D3), desired == 100, unsupported percent;
   - `set_feature_active(bool)` / `feature_active()` (mod-driven gate);
   - `init()`: registers the scene-change callback once (leaving
     SONG_SELECT ⇒ `registry().retire_preview()`); no signatures needed
     in this step;
   - `request_refresh()` stub (Step 5 fills it);
   - windows glue `maybe_bind_preview(file_id)` for the detour branch:
     gates (feature, scene) → path/source resolution (existing
     wavebank_hook accessors) → `qualify` → `prepare_binding(...,
     StretchTarget::Side)` with `next_preview_generation()` →
     `publish_preview` → note a publish event for the drain; refusals →
     `note_preview_refusal`; ensures the maintenance drain is running.
     Detour-legal: allocation OK (pre-original, game thread, song
     select), logging forbidden.
2. `wavebank_hook.rs`: the bind closure calls
   `preview::maybe_bind_preview(id)` when the gameplay path produced
   `BindOutcome::Stock`; adds `file_table_state(file_id)` (row `+0x20`
   dword) for Step 5.
3. `runtime.rs`: `ensure_maintenance_drain()` (public wrapper over the
   idempotent spawn); drain additions — one INFO per new preview binding
   (latched by preview generation: file id, rate, DSP mode) and the
   preview refusal mailbox WARN (coalesced).
4. `mods/song_playback_speed.rs`: `enable()` calls `preview::init` +
   `set_feature_active(true)` (log availability); `disable()` calls
   `set_feature_active(false)` + force-retire. No new option rows, no
   config (D11).
5. Zero footprint at 100 %: with both sides at identity, the detour's
   preview branch exits on the qualification's desired check with no
   allocation, no logging, no binding.
6. Validator harness (`scripts/validate_song_playback_speed.sh`): mount
   `preview.rs` + `preview_tests.rs`; file-check list updated.
7. Both cfg targets compile; existing suites unchanged.

## Dependencies

- Steps 1–2 (StretchTarget + preview registry slot) — complete on the tree.

## Implementation Approach

1. Write `preview_tests.rs` first (the qualify matrix + feature-gate
   semantics), red against a stub, then implement `qualify`.
2. Windows glue + detour branch + drain + mod wiring.
3. Harness updates; full gates (validator, windows check, fmt, build.sh).
4. Hand to the maintainer for cabinet deploy #1 (matrix C1/C2-partial/
   C3/C5/C6/C7 — no live re-trigger yet; edits apply at the next wheel
   settle).

## Acceptance Criteria

1. **Qualification matrix**
   - Given every combination of scene, path shape, entered flags, and
     desired values the design names
   - When `qualify` runs
   - Then exactly the single-entered-side non-100 supported-rate
     song-select dance-bank case produces a request (side, snapped
     percent, preserve flag); everything else declines

2. **Detour isolation**
   - Given a preview bind published by the branch
   - When the gameplay transaction machinery inspects the create
   - Then the outcome is Stock (no slot expose, no Q31 publication, no
     lifecycle phase change, no score/movie effect)

3. **Scene-exit defense**
   - Given a live preview binding
   - When the scene leaves SONG_SELECT
   - Then the preview slot is force-retired

4. **Zero footprint at identity**
   - Given both sides desiring 100 %
   - When preview creates flow through the detour
   - Then no binding, no refusal note, no log line results

5. **Cabinet demo (deploy #1)**
   - Given SONG SPEED persisted at 75 % (single side)
   - When browsing the wheel at song select
   - Then previews play slowed (pitch-preserved; record-player with
     PRESERVE OFF), wheel moves keep working, confirming a song plays
     gameplay exactly as shipped, versus and 100 % show stock previews

## Metadata

- **Complexity**: High
- **Labels**: song-rate, preview, detour, qualification, cabinet
- **Required Skills**: Rust, the song-rate create-detour architecture, detour-context law
- **Generated By**: code-task-generator 2026-08-15
- **Source Plan**: .agents/planning/2026-08-15-song-preview-rate/implementation/plan.md
- **Plan Step**: Step 3: Preview qualification + create-detour branch (cabinet deploy #1 — wheel-settle stretched previews)
