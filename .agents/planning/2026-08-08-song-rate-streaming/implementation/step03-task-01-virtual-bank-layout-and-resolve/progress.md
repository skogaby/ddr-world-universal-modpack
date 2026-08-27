# Progress — Step 3 task-01: Virtual Bank Layout, Pre-Data Synthesis, and Region Resolve

Updated: 2026-08-09
Status: Complete (uncommitted — maintainer commits personally)

## Checklist

- [x] T1–T4 written against the stub; failed for the expected reason
      (19 compile errors: missing `plan_virtual_bank`/`VirtualBankLayout`/
      `Region`/`EntryRate` — the absent-API failing state)
- [x] xwb `stream_pre_data` composition (serializer untouched — the two
      private functions and all three serializer entry points unedited)
- [x] `plan_virtual_bank` + `VirtualBankLayout` + additive PlanError variants
      (`EntryRate { index, source }`, `PreData(String)`)
- [x] `Region`/`ResolvedSpan`/`resolve` (stock EOF clamp, region-capped
      spans, spanning reads by iteration)
- [x] T1–T4 green in the fast harness (37/37 — 33 baseline + 4 new);
      serializer suites unmodified + green
- [x] Gate 1: validator green — 127/127 host tests, all report checks PASS,
      validation passed (`logs/validator.log`)
- [x] Gate 2: se-bank ALL CHECKS PASSED (`logs/se-bank.log`)
- [x] Gate 3: windows check 0 warnings (`logs/check-windows.log`)
- [x] Gate 4: fmt clean (whole crate)
- [x] Gate 5: build.sh release DLL OK (`logs/build.log`)
- [x] NO commit (maintainer commits personally)

## TDD cycles

1. Tests first: generalized the shared fixture (`build_bank` → thin wrapper
   over new `build_bank_with_data_lengths`, behavior identical), added
   `layout_payload` / `streamed_bank_bytes` (whole-bank oracle via
   `write_song_bank_streaming`) / `serve_virtual_bank` (resolve-driven
   reassembly) helpers + T1 pre-data equality/reparse, T2 reconstruction
   across chunkings (engine 0x1000-then-64KiB shape, 1/2047/2048/4097/
   u32::MAX), T3 EOF clamp + gap/spanning-region legs, T4 refusal identity
   (28-bit at entry 1 via a 73 MB ceiling fixture; degenerate one-frame
   terminal loop at 175% keeping the stub's `InvalidMappedLoop` identity).
   Failed to compile against the stub as required.
2. Implementation: `xwb::stream_pre_data` (pub composition over the existing
   private `validate_stream_write_layout` + `write_stream_header` — one
   canonical emitter, zero edits to the serializer path) and the
   virtual_bank layer (`VirtualBankLayout`, `plan_virtual_bank`,
   `Region`/`ResolvedSpan`/`resolve`, module doc updated off the stub
   wording). Full suite green on the first complete run (37/37).

## Acceptance criteria evidence

- AC1 pre-data canonical: `virtual_bank_pre_data_matches_streaming_serializer`
  — both entry orders × {75%, 125%}: 2048-byte block == serializer prefix,
  `virtual_size` == oracle length == `serialized_song_bank_len`, completed
  file reparses with the plan's stretched metadata, `main_entry_index`
  correct for both orders.
- AC2 resolve reconstructs: `virtual_bank_resolve_reconstructs_serializer_layout`
  — both orders × {75, 100, 125} × 6 chunkings (incl. the real header-read
  shape and region-spanning requests) byte-match the oracle.
- AC3 EOF clamp + refusals: `virtual_bank_resolve_clamps_at_eof` (at/past/
  straddling the end, zero-length request, header-read span boundary, gap
  region) and `virtual_bank_plan_refusals_carry_entry_identity`
  (`EntryRate { index: 1, DurationOutOfRange }` at 25%, stub-identical
  `InvalidMappedLoop { index: 0 }`).
- AC4 serializer untouched: no edits to `write_song_bank_streaming` /
  `serialize_song_bank` / `write_song_bank` or the private pair; existing
  serializer suites + validator synthetic/corpus sections pass unmodified.
- AC5: five gates green, Windows check 0 warnings.

## Deviations

- **`PlanError` gained two additive variants** (task allows shape freedom;
  behavior binding): `EntryRate { index, source }` — rate refusals from
  `plan_virtual_bank` carry entry identity while `plan_entry` keeps the
  stub's `Rate` identity (R5) — and `PreData(String)` for the emission leg
  (`XwbError` is not `PartialEq`; the leg is structurally unreachable after
  a successful plan, documented in the variant).
- **`main_entry_index` derived from the parser's identity invariant**
  (`usize::from(entries[1].name() == bank.name())` + debug_assert) instead
  of a fallible search — `SongBank` is only constructible via
  `parse_song_bank`, whose `validate_identity` guarantees exactly one main
  entry.
- **Shared fixture generalized**: `build_bank` became a thin wrapper over
  `build_bank_with_data_lengths` so the 28-bit refusal leg can build a
  ceiling-sized entry; all existing call sites unchanged.

Status: Complete (uncommitted — maintainer commits personally)
