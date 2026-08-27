# Progress — task-01 pacing-raise (Step 3)

- [x] `mgr+0x70` open cap raised (default 64) on first hooked onUpdate,
      restored on the actor done-flag and in `disable()` (idempotent, null-guarded)
- [x] One-shot begin/complete INFO with item count + elapsed ms
- [x] Dev-only `DDR_FAST_BOOT_OPEN_CAP` A/B override (not operator config)
- [x] cargo fmt / check (win, 0 warnings) / build.sh clean; harness still 20/20

## Deploy & measurement (2026-08-24, local CrossOver, gamemdx 20260721)
Same-machine A/B, 7305 work items both runs:
- **cap 4  (DDR_FAST_BOOT_OPEN_CAP=4): 6939 ms**
- **cap 64 (default):                 2382 ms  → ~2.9× faster**
Cap restored to 4 post-pass both runs; attract loads ran at stock cap.

## Decision: Appendix B bounded drain NOT needed
The cap raise alone is a large win and the device is not idle-bound at 64
(2382 ms / ~1466 files ≈ 1.6 ms/file). Deferring the bounded drain
indefinitely per the design's measurement gate. (On the slower reference
cabinet — 31 Hz, cap-4 ⇒ ~124 opens/s — the relative win should be larger.)

## Notes
- The 2382 ms is boot-analysis wall time (first onUpdate → done flag). It is
  not "instant" because per-file load latency remains; Step 7's cache removes
  the loads entirely for unchanged charts.
- Local cap-4 (6939 ms) < the reference cabinet's ~15.5 s because this host
  has a faster disk / higher boot-loop fps; the valid comparison is the
  same-machine A/B above.

Status: Complete (uncommitted — maintainer commits manually)
