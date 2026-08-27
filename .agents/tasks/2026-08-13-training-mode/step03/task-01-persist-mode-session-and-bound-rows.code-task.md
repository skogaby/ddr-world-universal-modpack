# Task: PersistMode::Session + SKIP FIRST / OMIT LAST rows

## Description
Add the `PersistMode::Session` framework variant to `custom_options` —
registered normally, excluded from BOTH the network fields (save and
load) and the JSON cache (write and prime), and reset to the row's
`default_value` on card-in — then register Training Mode's two
section-bound scalar rows with it: `training_skip_first`
("SKIP FIRST (s)") and `training_omit_last` ("OMIT LAST (s)"), 0–599 s,
step 5, coarse 30, default 0 (design §4.1's row table). Label textures
via the existing `scripts/gen_option_labels.py` pipeline.

## Background
Section bounds are deliberately session-scoped (design §4.1): they are a
practice-session tool, not a profile preference — a player carding in
next week must NOT inherit last week's skip. The existing modes don't
fit: `Full` persists everywhere, `SaveOnly` still EMITS a network field,
`None` resets on every card swipe but was the historical `persist: false`
(check its exact reset point — if `None` already resets at card-in and
emits/loads/caches nothing, `Session` may be a semantic alias; more
likely `None` rows also skip some registration-side bookkeeping the
session rows need. Establish the actual difference during Explore and, if
`None` already IS the needed behavior, say so and confirm with the
maintainer before adding a redundant variant). The `ShowWhen::NotEquals`
addition is the model for a small framework-variant change: enumerate the
touch points (api.rs enum + every `PersistMode` match in registry.rs /
mod.rs / persistence paths) and keep each arm's intent documented.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-13-training-mode/design/detailed-design.md (§4.1 row table + session-persistence paragraph, §5 config note: "the session rows serialize nothing")

**Additional References (if relevant to this task):**
- src/services/custom_options/api.rs (`PersistMode`, scalar-row registration surface)
- src/mods/song_playback_speed.rs (the scalar-row registration model: gating on `set_option_available` + `row_injection_available`, `load_transform` precedent)
- scripts/gen_option_labels.py (texture pipeline; `seop_item_<id>.png` naming)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `PersistMode::Session` in `api.rs` with doc comments stating the
   contract: no network save field, no network load, no JSON cache
   write/prime, value reset to `default_value` at card-in (the same
   profile-load lifecycle point where `Full` values land — one hook,
   registry-side; maintainer-approved recommendation).
2. Every existing `match` over `PersistMode` gains an explicit `Session`
   arm (no `_` catch-alls that silently misfile the new mode).
3. Two scalar rows registered from `training_mode` (enable path, gated
   like SONG SPEED on `custom_options::set_option_available` +
   `row_injection_available()`): ids `training_skip_first` /
   `training_omit_last`, labels "SKIP FIRST (s)" / "OMIT LAST (s)",
   range 0–599, fine step 5, coarse step 10× class (coarse 30), default
   0, `PersistMode::Session`, per-player. Expose the per-side values to
   `bounds.rs` (atomics or accessor — task-02 consumes).
4. Label textures generated via `scripts/gen_option_labels.py`
   (`seop_item_training_skip_first` / `seop_item_training_omit_last`) and
   committed under the repo's existing option-label asset location; note
   in the demo instructions that the PNGs must be deployed to the
   cabinet's `data_mods/` (maintainer-side step).
5. Host tests (harness-mounted custom_options registry kernel): Session
   rows emit no wire field, are skipped by the JSON cache prime, and
   reset to default at the card-in hook; existing Full/SaveOnly/None
   behavior unregressed.

## Dependencies
- None new (Steps 1–2 shipped; pure framework + registration work).

## Implementation Approach
1. Explore: map every `PersistMode` consumer (registry, persistence,
   mod.rs cache) and the card-in/profile-load hook; settle the
   Session-vs-None distinction question first.
2. TDD the framework variant against the harness registry kernel.
3. Register the rows + generate textures; wire the value accessors.

## Acceptance Criteria

1. **Session rows serialize nothing**
   - Given a registered `PersistMode::Session` scalar row with a non-default value
   - When the network save fields and the JSON cache are produced
   - Then the row contributes no wire field and no cache entry, and the network load never touches it
2. **Card-in resets**
   - Given a Session row holding a non-default value
   - When the card-in/profile-load hook fires
   - Then the row's value is back at `default_value` (per side)
3. **Rows registered and adjustable**
   - Given the mod enabled on a cabinet with the label textures deployed
   - When the MODS tab is opened
   - Then SKIP FIRST (s) / OMIT LAST (s) render with 0–599 range, fine 5 / coarse 30 stepping, default 0
4. **Existing modes unregressed**
   - Given the existing suites
   - When the harness runs
   - Then every Full/SaveOnly/None test passes unchanged

## Metadata
- **Complexity**: Medium
- **Labels**: custom-options, framework, training-mode, host-tested
- **Required Skills**: Rust, the custom_options registry/persistence architecture
- **Generated By**: code-task-generator 2026-08-13
- **Source Plan**: .agents/planning/2026-08-13-training-mode/implementation/plan.md
- **Plan Step**: Step 3: Bound rows, session persistence, silent skip-first start
