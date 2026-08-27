# Progress — Step 5 fix: Preview Passthrough (loading-screen stall at slow rates)

Updated: 2026-08-11
Status: Complete (uncommitted — maintainer commits personally) — LIVE-CONFIRMED
(deploy #4, 2026-08-11): 25% and 175% both load in ~5 s (normal); 25% played
full-through (517 s wall) with **0 deferrals, 0 µs max latency**, frames
produced == the main entry's planned output EXACTLY once; no refusals, no
stuck reads. The fix chain is closed.

## Deploy #3 regression and its fix (2026-08-11)

The v2 passthrough build played 25% songs at plain 100%: every bind refused
with `HeaderSynth` (fail-open worked as designed — that is WHY it sounded
clean). Root cause: `xwb::validate_stream_write_layout` applied
`validate_generated_layout` — the generated-content rule demanding
`duration == blocks × samples_per_block` EXACTLY — to both header entries.
Stretched streams always satisfy it; a verbatim stock preview essentially
never does (its declared duration sits inside its whole-block payload's
final block). Every host fixture used a block-exact preview
(2,048 = 16 × 128), so the entire suite was blind to the stock shape.
Fix (one line of substance in `xwb.rs`): the stream-layout validator now
uses `adpcm::validate_encoded_layout` — the PARSER's own rule — so the
emission contract accepts exactly what `parse_song_bank` accepts (generated
streams pass the same rule). Fixtures made honest FIRST (TDD: preview
duration 2,000 over a 16-block payload in `core/xact/tests.rs`,
`generator_tests.rs`, `binding_tests.rs`; the validator's synthetic bank
preview declares 8,100 over 64 blocks — note `ceil(duration/spb)` must
equal the block count, which the first attempt at 8,000/64 got wrong and
the parser itself rejected) — 22 host tests reproduced the live refusal,
then went green with the fix.

## History — two iterations, one instrument

- **v1 (side buffer, live-falsified):** the first diagnosis blamed the
  preview entry's POSITION (file tail behind a linear ring). Fix: produce
  the non-main entry first into a resident side buffer. Gate-green but the
  live re-test still stalled 20+ s at 25%.
- **Instrument:** a drain-side STUCK-READ sampler (offset/region/cursors of
  any read pending > 500 ms; kept in `runtime.rs`, dedupe per arm instance).
  One live run named the truth:
  `STUCK READ offset=2906112 region=EntryData{entry:1(main),offset:0}
  side=(412160/2903600)` — the MAIN entry's first packet waiting behind
  side-entry PRODUCTION, at ~114k frames/s ≈ **2.4× realtime**.
- **Real root cause:** WSOLA cost scales ~sample-rate² per second of audio;
  the validator's 1.65M frames/s on 8 kHz fixtures is only ~6× realtime at
  the game's 47 kHz, ~2.4× under CrossOver with the loading screen
  competing. Stretching ~56 s of preview audio during loading is inherently
  10–25 s of DSP — reordering (v1) could never fix it. (The original
  "21× realtime" benchmark reading divided TOTAL frames by the stall time;
  for preview-first banks the stalled read was the main packet, so the
  stall only ever covered the preview's production.)
- **v2 (preview passthrough, maintainer-approved 2026-08-10):** don't
  stretch the preview at all.

## What landed (v2)

- `core/xact/virtual_bank.rs::plan_virtual_bank`: the non-main entry is a
  verbatim PASSTHROUGH plan (stock duration/loops/data_len, identity rate,
  no loop context); only the MAIN entry is rate-planned — the 28-bit
  ceiling now applies to the main entry only (slightly widens support).
- `binding.rs`: the v1 side buffer/watermark/cap machinery DELETED; the
  side entry serves by memcpy from the resident source copy
  (`side_source_offset` computed at construction; spans always available —
  bank prepare completes synchronously). Silence-fill serves the side's
  REAL bytes (static data, no producer involvement). `Binding` is auto-Sync
  again (no direct UnsafeCell).
- `generator.rs`: side production phase deleted — the producer walks the
  MAIN entry only (`[main_data_start, main_data_end)`); regen targets
  main-only.
- `runtime.rs`: STUCK-READ diagnostic kept (drops the side-progress field);
  the req-28 silence-fill WARN kept.
- Oracles updated to the passthrough composition (main stretched + side
  verbatim): `core/xact/tests.rs::transform_bank_oracle` + a
  `EncodedFeed::verbatim` mode, `generator_tests::transform_bank_oracle`,
  and the validator harness `replay_virtual_bank` (its own inline oracle —
  `transform_bank`, the pure-DSP whole-bank oracle for the pitch/SNR/seam
  sections, intentionally still stretches both entries). The 28-bit
  refusal test moved the ceiling to the MAIN entry and pins the side's
  new passthrough behavior.
- Regression pin strengthened: the side entry's prepare-shaped read serves
  SYNCHRONOUSLY (no producer exists at all), byte-equal to the stock
  preview, ring untouched — both physical entry orders.
- New `binding_tests::plan_passes_the_side_entry_through_verbatim`.

## Expected live outcome (maintainer re-test, deploy #3)

- 25% loading ≈ stock at every rate: bank prepare's side read completes
  synchronously; the main entry's first packet needs only ~64 KiB of
  production (< 1 s even at 2.4× realtime).
- Reclaim line: deferrals ≈ 1–2 with max latency well under a second;
  frames produced == the MAIN entry's planned duration only.
- If anything still stalls, the STUCK-READ WARN names it.

## Gates (all green, logs in `logs/` — v2 run)

1. validator — passed; cargo-test phase 161/161; replay legs green under
   the passthrough oracle
2. se-bank-synth — ALL CHECKS PASSED
3. windows check — 0 warnings
4. fmt --check — clean
5. `./build.sh` — release DLL OK (44 s)

## Deviations

- Design req 14's "both entries are stretched" is superseded by the
  approved passthrough (recorded here and in plan.md; the design doc's
  correction belongs to plan Step 7's documentation pass).
- The live throughput margin story in the feature progress.md needs the
  corrected reading: main-entry production ≈ 2.4× realtime under CrossOver
  (not 21×) — still ≥ 1× with margin for gameplay, and the loading path no
  longer depends on production at all.

Status: Complete (uncommitted — maintainer commits personally)
