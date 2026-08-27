# Plan — Step 5 fix: Preview-Entry Side Buffer (loading-screen stall at slow rates)

Status: Approved 2026-08-10 (maintainer, in-session — "Approved, that sounds good to me")

## Problem (cabinet-diagnosed, 2026-08-10)

Live Step-5 runs show the loading screen lasting ≈ the FULL stream production
time (25%: 23–25 s observed; log evidence: commit→AssistTick-anchor gap ≈
`max_deferral_latency` at every rate — 23.4 s @25%, 2.6 s @120%). Root cause:
at bank prepare the engine primes a stream context for EVERY wave, including
the never-played-during-gameplay `<code>_s` preview entry, whose data sits at
the END of the virtual file for main-first banks. The producer fills the ring
LINEARLY (entry 0 → gap → entry 1), so that one read defers until the entire
stretched main entry exists; the armed-slot pacing override makes the producer
sprint the whole way; and the ring window then sits at the file tail, so
gameplay's first main-entry reads land behind-window and force a full
regeneration back to offset zero (the constant "3 deferrals" signature — the
main entry is produced TWICE per song).

## Fix (approved design deviation from the one-linear-ring model)

The two entries are independent streams (each stretches from frame 0). Serve
the NON-MAIN entry from a dedicated resident side buffer; the ring covers the
MAIN entry only:

1. `Binding` gains `side_entry` (`1 − main_entry_index`), a full-length
   `side_buffer` (allocated at construction; try_reserve → typed refusal) and
   a monotonic `side_produced` watermark (Release-published; bytes below it
   are never rewritten — no seqlock needed).
2. `Ring::new(capacity, main_offset)`; the producer cursor walks ONLY
   `[main_offset, main_offset + main_len)` — it never traverses the gap or
   the side entry, so the window can never slide past gameplay's read
   position (the spurious regeneration disappears).
3. `check_spans`: side-entry spans are available iff inside `side_produced`
   (NotProduced otherwise; never BehindWindow); main spans keep the ring
   window logic. `copy_spans`: side spans copy from the side buffer.
4. `GeneratorCore::step`: produce the side entry FIRST (bounded chunks into
   `side_append`, completing ready slots each chunk — the engine's prepare
   read completes after its first ~64 KiB ≈ tens of ms), then main-entry
   production exactly as today. Regen targets are main-only by construction;
   the obsolete cross-entry feed invalidation in `rewind_to` is removed.
5. Safety cap: a stretched side entry > 64 MiB refuses the bind
   (`BindRefusal::Plan`-leg; production previews are ~2–8 MB at 25%).
6. The virtual file's BYTES are unchanged — same layout, same oracle
   byte-equality (the replay suites prove it).
7. Also: the missing design-req-28 silence-fill WARN — the maintenance drain
   watches the ACTIVE binding's state and WARNs once per generation when it
   reads SilenceFill.

## Test scenarios

- REGRESSION PIN (the bug): a read at the side entry's start completes while
  `ring_produced() == main_offset` (zero main production) — both physical
  entry orders, driven synchronously through `GeneratorCore`.
- All existing suites (oracle byte-equality both orders × rates, deferral
  exactly-once, behind-window regeneration, silence-fill, retire/reclaim,
  transaction/QR composition) pass under the new serving model.
- Side-entry cap: an oversized side entry refuses → EarlyFailed leg.
- Silence-fill WARN: host-testable predicate is trivial (state read);
  windows drain wrapper stays thin.

## Acceptance

- Fast harness + full validator green; five gates green.
- Maintainer re-test: 25% loading ≈ stock; reclaim lines show deferrals ≈ 0–1
  and max latency in the tens of ms, not seconds.
