# Progress: Step 4 — ShowWhen::NotEquals, option row, textures

Covered by the feature plan Step 4 (approved 2026-08-12); autonomous run.

## Done

- [x] `ShowWhen::NotEquals` — all four touch points:
  api.rs (enum + doc), registry.rs (parent validation covers both parented
  variants), rows.rs (`is_show_when_satisfied` NotEquals arm, fail-open on
  unresolvable parent like Equals), rows.rs (`update_children_visibility`
  child-detection matches both variants → same-frame remask).
- [x] `song_playback_speed.rs`: `preserve_pitch` bool row — default ON,
  `NotEquals { song_speed, 100 }`, `load_clamp_bool` load transform,
  `on_preserve_pitch_change` → `set_desired_preserve_pitch`; registered
  immediately after the parent (Duplicate tolerated; failure degrades to
  pitch-preserved with one WARN — never refuses the mod); per-side re-seed
  on enable; `set_option_available` for the child in enable/disable;
  disable resets the flag atomics to preserved.
- [x] `scripts/gen_option_labels.py`: LABELS + two PREVIEWS entries (copy
  per design FR-7, opening with the user-specified sentence); fixed the
  pre-existing duplicate `arrow_opacity` entry + "Sone" typo section
  header. Script run: no overflow warnings; tracked PNGs unchanged
  (deterministic regen); 3 new PNGs created.
- [x] Textures visually verified (label glyphs + both preview panels).
- [x] 151/151 green; `cargo check --target x86_64-pc-windows-msvc` clean.

## Notes

- End-to-end chain now complete in code: row press → atomics → scene-26
  latch → binding → DspState. Cabinet demo (plan Step 4 demo) pending the
  final deploy — deferred to the Step 6 manual test per the autonomous-run
  instruction.

## Deviations

- Child registration failure logs WARN and continues (design's fail-open
  NFR-2) instead of refusing the whole mod — the parent row remains fully
  functional pitch-preserved.
