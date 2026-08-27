# Implementation Plan: Preserve Song Pitch sub-option

Status: Approved 2026-08-12

Design: `design/detailed-design.md` (approved 2026-08-12). Steps reference
its sections rather than restating them.

## Checklist

- [x] Step 1: Resampler core (`core/xact/resample.rs`) + host tests
- [x] Step 2: Generator/binding mode seam (`DspState`, flag on `Binding`)
- [x] Step 3: Flag carriage — runtime atomics → lifecycle latch → bind
- [x] Step 4: `ShowWhen::NotEquals`, option row, and textures (end-to-end)
- [x] Step 5: bemani-buddy backend persistence
- [x] Step 6: Validation-script section, docs, and full cabinet pass

---

## Step 1: Resampler core (`core/xact/resample.rs`) + host tests

**Objective:** Land the new DSP module — the frozen reference oracle
`resample_interleaved` and the streaming `ResampleState` — per design
§Components 1 (Q32 piecewise position map, linear interpolation, loop-seam
mapping, O(1) `positioned_at`).

**Guidance:** Pure module, zero game dependencies; reuse `SourcePcm`,
`Produced`, `LoopContext` from `stretch.rs` and the rounding primitives from
`rate.rs`. Wire into `src/core/xact/mod.rs`. This step is front-loaded
because it carries the design's main risk (plan-driven byte-exactness).

**Tests (same step):** Add to `src/core/xact/tests.rs`, mirroring the
stretch suite — reference pitch tracking (`f_out ≈ f_source × S/O`), exact
output length, edge clamps; streaming-vs-reference byte identity across the
percent matrix; chunk-size independence; `positioned_at` suffix identity;
loop-seam continuity. Run via the validation harness's temp-package
`cargo test` (the fast-iteration path in
`scripts/validate_song_playback_speed.sh`).

**Integration:** None yet — new module only; `cargo check` clean.

**Demo:** Host test run showing the resampler's streams byte-identical
across chunkings/seeks and a 75 % sine measured at 0.75× the source
frequency.

## Step 2: Generator/binding mode seam

**Objective:** Make the producer mode-aware per design §Components 2–3
(binding part): `DspState::{Wsola, Resample}` inside `Feed`, mode-aware
`new`/`positioned_at`/`try_capture`, `Binding.preserve_pitch` field, and the
new `prepare_binding` parameter — with every production caller passing
`true` for now (behavior unchanged).

**Guidance:** The WSOLA arm keeps its checkpoint mechanics untouched; the
resample arm seeks directly and never captures checkpoints. `GeneratorCore::new`
reads the binding flag once.

**Tests (same step):** `src/services/song_rate/generator_tests.rs` — a
resample-mode pump vs a whole-buffer oracle (reference resample +
`encode_interleaved`), behind-window regeneration identity in resample mode,
and a guard that preserved mode still matches the existing stretch oracle.

**Integration:** Consumes Step 1's `ResampleState`. Default-true callers
keep the cabinet build byte-identical to today.

**Demo:** Generator host tests green in both modes; `cargo check` clean;
a deploy behaves exactly as current (flag hardwired ON).

## Step 3: Flag carriage — runtime → lifecycle → bind

**Objective:** Thread the per-side desired flag to the per-song latch per
design §Components 3: `DESIRED_PRESERVE_PITCH` atomics + accessors in
`runtime.rs`; `EligibilityInputs.desired_preserve`, `ArmRequest.preserve_pitch`,
`LifecycleState` atomic + getter in `lifecycle.rs`; `wavebank_hook`
passes the lifecycle value into `prepare_binding` (replacing Step 2's
hardwired `true`).

**Guidance:** The flag never influences the eligibility decision — it is
copied from the entered side on arm only. Add the latched flag to the
existing arm INFO log line for cabinet observability.

**Tests (same step):** Host test on `classify_scene26` asserting the
entered side's flag lands in `ArmRequest` (both sides, both values) and that
identity outcomes ignore it.

**Integration:** Atomics default to 1 (preserved) and nothing writes them
yet, so behavior is still unchanged; Quick Restart re-binds inherit the
latched value for free.

**Demo:** Deploy + play a rate-adjusted song: the arm log line shows
`preserve_pitch=true` latched; gameplay identical to today.

## Step 4: `ShowWhen::NotEquals`, option row, and textures (end-to-end)

**Objective:** The user-visible feature per design §Components 4–6: the
framework variant (4 touch points), the `preserve_pitch` bool row registered
after `song_speed` (default ON, `NotEquals { song_speed, 100 }`, load clamp,
`on_change` → Step 3's atomics, Duplicate re-seed, availability mirroring),
and the generated textures.

**Guidance:** Add the LABELS entry and both PREVIEWS entries to
`scripts/gen_option_labels.py` (copy per design FR-7), drop the duplicate
`arrow_opacity` PREVIEWS entry, run the script, and commit the three PNGs
under `data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/`.
Watch the script's overflow warnings for the preview copy.

**Tests (same step):** No host surface — this is UI/hook code; validation
is the cabinet demo below plus `cargo check`/`cargo fmt`/`./build.sh` gates.

**Integration:** First step where the atomics get written — completes the
end-to-end chain built in Steps 1–3.

**Demo (core end-to-end):** On the cabinet — the row is hidden at 100 %,
appears live when SONG SPEED moves off 100 (per-side), shows label/ribbons/
previews; playing at 75 % with OFF sounds record-player pitched-down with
arrows, judging, and assist ticks in sync; ON sounds like today; loading
with OFF is no slower.

## Step 5: bemani-buddy backend persistence

**Objective:** Server-side round-trip per design §Components 7, stacked on
the in-flight 012/013 working-tree changes (do not renumber or touch them):
model JSON (both `<option>` shapes) → codegen → migration 014 → DB model →
MySQL DAO → playdata handler (load / new-player / save) → sqlx cache
refresh.

**Guidance:** Follow the uncommitted `mod_song_speed` diff line-for-line;
never hand-edit the `@generated` protocol file. Leave any `cargo fmt` churn
in the working tree (maintainer folds it into one commit).

**Tests (same step):** Handler tests per the established five-test pattern
(present/absent/malformed parse; None-skipped / Some-echoed on load;
`load_option_all_none()` helper gains the field). `cargo build` +
`cargo test` green.

**Integration:** The DLL already emits/consumes `mod_preserve_pitch`
(PersistMode::Full is automatic) — this step makes the network round-trip
real. The JSON offline cache already worked from Step 4.

**Demo:** Toggle OFF, card out, card in — the row seeds back OFF from the
server; with the server offline the JSON cache restores it instead.

## Step 6: Validation-script section, docs, and full cabinet pass

**Objective:** System-level validation and documentation per design
§Testing Strategy: the additive `resample` report section in
`scripts/validate_song_playback_speed.sh` (inverted pitch expectation, SNR,
throughput; tail verifier updated to expect it), README updates (feature
table + option docs + `row_order` example gains `preserve_pitch`), and the
AGENTS.md song-playback-speed entry.

**Guidance:** Keep the existing WSOLA sections untouched (oracle
discipline). Finish with the repo's readiness gates: `cargo check` →
`cargo fmt` (whole crate) → `./build.sh`.

**Tests (same step):** Full validation-script run green including the new
section; the design's 8-point cabinet checklist executed end-to-end
(visibility, ON/OFF audio, loop seam, Quick Restart, persistence both paths,
containment log check, 100 % zero-footprint log check).

**Integration:** Final integration step — no orphaned code; all prior
steps' behavior re-verified together on the cabinet.

**Demo:** Validation report with the `resample` section passing; cabinet
checklist fully green; docs describe the shipped behavior.
