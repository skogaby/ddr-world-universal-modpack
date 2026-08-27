# Progress: Step 6 — validation-script section + docs

Covered by the feature plan Step 6 (approved 2026-08-12); autonomous run.
The cabinet-checklist half of Step 6 is deliberately left for the manual
test (per the user's "until it's ready for the end to end demo" instruction).

## Done

- [x] `scripts/validate_song_playback_speed.sh`: new `resample` report
  section — `ResampleResult` cells at 50 % and 175 % (reference resample
  through the PLAN's geometry incl. loop context, ADPCM round-trip,
  determinism, exact block-quantized length, latency), checks
  `resample_50`/`resample_175`, `Report.resample` field, tail verifier
  requires the section. Stretch legs untouched (oracle discipline).
- [x] **Bug found & fixed during the first full run:** the script's
  `estimate_frequency` (bounded-lag autocorrelation) folds to subharmonics —
  ratio-invariant (fine for the stretch legs' "pitch unchanged" check) but
  WRONG for the inverted expectation: at 50 % the true fundamental's lag
  (256) exceeds its 200-sample ceiling and it locked onto a subharmonic
  (33 % apparent error). Added `zero_crossing_frequency` (fundamental-true,
  mirrors the in-repo test helper) used by the resample legs only.
- [x] Full validation run GREEN: overall_pass = true; resample legs track
  the exact plan ratio (50 %: 250.03 → 125.01 Hz expected 125.02, err
  0.0062 %; 175 %: → 432.43 Hz expected 432.49, err 0.0123 %). Log:
  `../step01-resampler-core/logs/full-validation.log`.
- [x] README: Song Playback Speed feature row gains the PRESERVE SONG PITCH
  paragraph; `row_order` example + option-id list + conditional-child note
  gain `preserve_pitch` (and `song_speed`, previously missing from the id
  list).
- [x] AGENTS.md: Song Playback Speed table row gains the full
  preserve-pitch sub-section (row, NotEquals variant, resample DSP, flag
  carriage, validation, textures, backend migration 014) + planning-dir
  pointer.

## Remaining (manual)

- The design's 8-point cabinet checklist (visibility, ON/OFF audio, loop
  seam, Quick Restart, persistence both paths, containment, 100 %
  zero-footprint) — requires the cabinet.

## Deviations

- `zero_crossing_frequency` helper added to the script (necessitated by the
  measurement-tool limitation above; the design named the existing
  machinery, which proved unfit for the inverted check — recorded in
  AGENTS.md's feature row).

## Cabinet validation (2026-08-13)

Maintainer-run end-to-end PASS: multiple songs × multiple rates, with and
without pitch preservation — audio as expected in both modes; option
round-tripped. Step 6 ticked in the feature plan; feature complete.
