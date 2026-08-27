# Implementation Plan: Real-Time Rate Preview at Song Select

Status: Approved 2026-08-15
Design: `.agents/planning/2026-08-15-song-preview-rate/design/detailed-design.md` (Approved 2026-08-15)

## Checklist

- [x] Step 1: Target-entry parameterization (planner + binding)
- [x] Step 2: Preview registry slot + io miss-path routing
- [x] Step 3: Preview qualification + create-detour branch (cabinet deploy #1 — wheel-settle stretched previews)
- [x] Step 4: Restart derivations (signatures + loader-chain resolution)
- [x] Step 5: Restart executor + debounce + wiring (cabinet deploy #2 — full matrix)
- [x] Step 6: Documentation + regression close-out

## Steps

### Step 1: Target-entry parameterization (planner + binding)

**Objective:** `core/xact/virtual_bank.rs` and `binding.rs` accept
`StretchTarget::{Main, Side}`; `Side` produces the inverse plan (stretched
`_s` entry, verbatim main served from the private source copy); `Main`
remains byte-identical to shipped behavior.

**Guidance:** Design §Components 1–2. Land the regression pin FIRST: a test
that snapshots today's `plan_virtual_bank` + `prepare_binding` outputs on
the existing fixtures, then refactor under it. Ring coverage/regeneration
ranges follow the target entry; keep the side-buffer production path
gameplay-only. Fixtures for the Side plan must have non-block-exact
durations (the "honest fixtures" rule).

**Tests (same step):** planner Side-plan layout/duration/loop-mapping
tests; Main-plan byte-identity pin; binding Side-target ring coverage;
verbatim-main serving from the source copy; serve-dispatch byte-identity
vs. a whole-buffer oracle in both DSP modes at 50 % and 175 %.

**Integration:** pure layers only; no engine-facing change. Gameplay
callers updated to pass `StretchTarget::Main` explicitly.

**Demo:** `cargo test` green, including the byte-identity pin proving
gameplay plans are untouched; `./scripts/validate_song_playback_speed.sh`
still passes.

### Step 2: Preview registry slot + io miss-path routing

**Objective:** `BindingRegistry` carries an independent preview slot
(`publish_preview` / `with_preview` / `retire_preview`), `retire_by_file`
covers both slots, retired preview bindings flow through the existing
sweep, and `io_callback_hook::bound_verdict` checks the preview slot after
an active-slot miss.

**Guidance:** Design §Components 2–3. Mirror the active slot's atomic
pattern; add the preview refusal mailbox; preview identity from a separate
`AtomicU64` counter (R15).

**Tests (same step):** publish/replace/retire; both-slot `retire_by_file`;
routing order (active first, preview on miss, trampoline on both-miss);
sweep reclamation; refusal-mailbox coalescing.

**Integration:** consumes Step 1's target-aware `prepare_binding` in the
tests. Hot-path cost audit: unchanged when no bindings exist (one Acquire).

**Demo:** `cargo test` green; a host test drives a full
publish→serve→unregister-retire→sweep cycle for a preview binding.

### Step 3: Preview qualification + create-detour branch (cabinet deploy #1)

**Objective:** The core end-to-end behavior: while the controlling side
desires ≠ 100 %, every wheel-settle preview create binds and the preview
plays stretched in the selected DSP mode. This is the
design-invalidating-risk step (scene-25 binding + `_s`-entry streaming) —
front-loaded before any restart machinery.

**Guidance:** Design §Components 4–5 (the `qualify` pure function and
`preview.rs` policy skeleton: `feature_active` conjunction — WITHOUT the
restart derivations, which are Step 4 — scene-exit force-retire callback,
drain reporting). Wire the detour branch behind the gameplay path's stock
outcome; add `file_table_state`; wire `song_playback_speed::enable/disable`
to `set_feature_active`. No restart, no debounce yet: `request_refresh` is
a stub.

**Tests (same step):** the `qualify` matrix (scene × entered × desired ×
path shape incl. `custom_bgm_%04d` exclusion); feature-conjunction gating;
scene-retire behavior (pure state-machine part).

**Integration:** first consumer of Steps 1–2 in the real detour.

**Demo (cabinet deploy #1):** with SONG SPEED persisted at 75 %: browse the
wheel — every preview plays slowed and pitch-preserved (sub-second start);
preserve-pitch OFF (persisted) gives record-player previews; confirm a song
→ gameplay identical to shipped behavior (matrix C5); 100 % boot shows zero
footprint (C1); versus shows stock previews (C6); 25 %/175 % latency spot
check (C7). Value edits do NOT yet restart the playing preview (expected —
the next settle picks them up).

### Step 4: Restart derivations (signatures + loader-chain resolution)

**Objective:** The four preview-scoped signatures
(`selectmusic_view_ctor`, `audio_loader_ctor`, `cue_handle_stop`,
`sound_bank_create_router`) resolve on all four supported builds and the
loader-chain resolver (`TS child → View → AudioPlayer → AudioLoader` with
vftable identity gates + field sanity checks) reads a live loader.

**Guidance:** Design §Components 5–6 and the RE inventory in
`research/preview-retrigger-re.md` §6. Verify AOB uniqueness on all four
builds in Ghidra before committing patterns. Derivation failure keeps
`feature_active` true for the Step-3 half but marks the restart half
unavailable (one WARN naming the missing piece).

**Tests (same step):** derivation decode tests against the signature-store
harness conventions (pattern→address shape); loader-chain resolver
validation logic (pure parts: field sanity predicates, identity-gate
refusal paths).

**Integration:** `preview::init` gains the derivations; nothing calls the
restart yet.

**Demo:** boot log on the cabinet (or the Step-5 deploy's log) shows all
four derivations resolved + the restart half reporting available; a
diagnostic one-shot INFO proves the loader chain resolves while a preview
plays.

### Step 5: Restart executor + debounce + wiring (cabinet deploy #2)

**Objective:** Live edits restart the playing preview: 150 ms debounce,
supersession check against the selected-song publication generation,
game-thread executor running the stop→unregister→re-create→re-arm
sequence, scene-gated, fail-open at every precondition.

**Guidance:** Design §Components 5, 7 (executor steps 0–5; `RefreshCell`;
`on_change` stamping; panic containment). Executor registered on the input
manager's per-frame poll; one relaxed load when idle.

**Tests (same step):** debounce cell semantics (stamp/coalesce/fire/clear,
scene-gate suppression, supersession suppression); executor precondition
matrix (pure predicate parts); restart-sequence ordering under a mocked
call recorder (host-side seam consistent with how the transaction suites
mock stock calls).

**Integration:** completes the feature; `request_refresh` un-stubbed.

**Demo (cabinet deploy #2):** full matrix C1–C9 from the design's Testing
Strategy — notably: C2 (edit → single restart ~150 ms after the last tick),
C3 (DSP-mode switch restarts), C4 (return to 100 % restores the stock
preview), C8 (rapid scroll coalesces to one restart), C9 (fast-confirm race
declines cleanly).

### Step 6: Documentation + regression close-out

**Objective:** The feature is documented per repo conventions and the full
regression surface is re-validated.

**Guidance:** Publish the RE findings as `docs/` research notes (the
preview pipeline/AudioLoader material from
`research/preview-retrigger-re.md`, addresses file-relative per the
steering conventions); add the AGENTS.md feature-row entry (mechanism,
signatures, fail-open behavior, no-config note) and the README feature
blurb; finalize `progress.md`; update
`scripts/validate_song_playback_speed.sh` if a preview report section was
added in Steps 1–2.

**Tests (same step):** readiness gates — `cargo check` clean, whole-crate
`cargo fmt`, `cargo test` green, `./build.sh` clean, validator script
green.

**Integration:** none (docs + gates).

**Demo:** one final cabinet regression pass: a full ordinary session
(100 % song, 75 % song, card-out) confirming gameplay rate, score
containment, and logout sanitisation behave exactly as shipped, with
previews rate-bound throughout.
