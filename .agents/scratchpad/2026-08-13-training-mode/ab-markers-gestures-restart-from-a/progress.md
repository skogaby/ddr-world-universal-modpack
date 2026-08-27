# Progress: ab-markers-gestures-restart-from-a

## Checklist
- [x] song_reset::current_raw_music_count accessor (+0x178, sanity-ranged)
- [x] training_mode/bounds.rs (A/B/clear markers, per-side×per-button GestureBuffers, scene clearing, seek-composition quantization, chart-end clamp)
- [x] training_mode/mod.rs wiring (input+scene callbacks, GESTURES_ACTIVE latch, `pub use bounds::active_section_start`) + DDR_TRAINING_TEST_SHIFT_MS REMOVED (disable()'s initial-mapping clear retained — Step 3 uses the API)
- [x] quick_restart_or_fail restart-from-A (consult → `request_reset(a_ms, max(TRAINING_LEAD_MS, restart_delay), Zero, recovery)`; Refused ⇒ WARN + shipped restart-at-0 fall-through)
- [x] Gates: harness 210/210 → cargo check clean (0 warnings) → cargo fmt → ./build.sh clean (`logs/build.log`)
- [x] Cabinet demo (maintainer-run — closes plan Step 2): **PASS 2026-08-13
      (attempt 3)** — 50/100/175 % marker → restart-from-A ×N → clear →
      restart-at-0 all as expected; toast exactly centered. Plan Step 2
      ticked.

## Record
- 2026-08-13: Setup + Explore (GestureBuffer precedent, buttons, scene ids,
  +0x178 raw count, cross-mod pub-fn precedent).
- 2026-08-13: Implementation complete; all local gates green. Release DLL at
  target/x86_64-pc-windows-msvc/release/ddr_world_hook.dll.
- 2026-08-13: **Cabinet demo attempt 1: FAIL — two findings, both fixed.**
  (a) `derive_judge_rebuild_trio`'s `SCAN_LIMIT = 0x60` truncated the E8
  scan (trio at match+0x37/+0x5F/+0x93; rebuild past the window) → seeks
  unavailable all boot → every restart-from-A fell back to 0. Fixed:
  `SCAN_LIMIT = 0xC0` (next unrelated call +0xE0; scan stops at 3 targets).
  (b) Toast left-anchored at x=640 (interim fix: width-estimate
  centering). The fallback ladder, gestures, logs, and t=0 resets all
  behaved per design during the failed attempt.
- 2026-08-13: **Cabinet demo attempt 2: mechanics PASS** — restart-from-A
  and loop-at-A working (trio derivation resolved with the widened
  window). Toast centering only approximate (width estimate). Follow-up:
  decompiled the bmpfont render fn — the line desc has TWO alignment
  fields: `+0xA8` HORIZONTAL per-line (1 = center via width × −0.5, the
  engine's own pre-measured line width) and `+0xAC` VERTICAL block (the
  field `set_alignment` wrote — hence "center did nothing"). Fixed
  `TextWidget::set_alignment` → +0xA8; toast reverted to
  `set_position(640, 630)` + `TextAlignment::Center` — exact for any
  text, no estimation. learnings.md corrected (two-fields entry).
  Gates green; fresh build for the visual re-check.

## Deviations
- **Gesture-feedback toast added (maintainer request, 2026-08-13, pre-demo):**
  `training_mode/toast.rs` — one lazily-created native TextWidget
  (autoplay-watermark lifecycle precedent), bottom-center (640, 630),
  white with black outline, scale 1.2, centered via the native alignment
  field (UNTESTED in-repo — watch placement on the cabinet). Fade curve:
  100 ms in → 250 ms full-brightness hold (the requested ~0.25 s flash) →
  300 ms out, driven by a generation-tokened self-requeueing render-thread
  callback (no locks across the schedule; panic-free). Messages: "Set
  beginning marker" / "Set end marker" / "Cleared markers" (clear toast
  only when markers actually existed — lifecycle clears stay silent;
  `clear_markers` now returns the had-markers bool). `toast::dismiss()` on
  mod disable.
- **Gesture row amendment (maintainer, 2026-08-13, pre-demo):** marker
  gestures moved to the pinpad's MIDDLE row — triple-4 = set A, triple-5 =
  clear, triple-6 = set B (was 7/9/5 per the task text/D3). 7/9 become the
  v2 FF/RW candidates. D3 amended in idea-honing.md; design §8, plan
  Step-2 text, and summary.md updated to match. Code + logs updated;
  gates re-run.
- Lead composition = `max(TRAINING_LEAD_MS, restart_delay_ms)` (the task's
  "+" prose vs the design's minimum-approach reading; max preserves the
  operator's calibrated repositioning window when their delay ≥ 2500 ms,
  and the demo's default-delay case is identical either way).
- Markers quantize through the seek's own wall→grid→content composition
  (falling back to the raw count with no live binding) so the stored value
  equals the seek's landing point — the task's "block-quantized via
  task-02's helper", made rate-correct.

## Cabinet demo (Step-2 close-out — maintainer runs)
Mid-song: triple-4 sets A (INFO logged + "Set beginning marker" toast
fading in/out bottom-center); triple-1 → restart-from-A after the 2.5 s
silent approach — combo/score/gauge reset, claps aligned — at 100 %,
75 %, and 125 % rate alike. triple-5 clears ("Cleared markers" toast,
only when markers existed); triple-6 sets B ("Set end marker" toast,
INFO only otherwise); quick_logout's triple-9 at song select unaffected;
mod disabled ⇒ everything bit-for-bit shipped. Watch the toast's
horizontal centering (the native Center alignment field is exercised
here for the first time — if off-center, it's a one-constant tune).
**NOTE (plan-approved ordering): score containment arrives in Step 5 —
a seek-practiced song's score WILL submit during this demo.**

Status: Complete (uncommitted — maintainer handles git; plan Step 2 ticks
only when the cabinet demo passes)

- 2026-08-13: **Cabinet demo attempt 3: PASS.** Full flow at 50/100/175 %
  (set marker → restart-from-A a couple of times → clear → restart-at-0);
  toast exactly centered via the native +0xA8 alignment. Log: trio derived
  at boot; seeks 22–64 ms stop→anchor; wall-domain shifts numerically
  correct per rate (100 %: 34813 ms → 11993 blocks; 175 %: 44861 → 8831;
  50 %: 28634 → 19730; lead 861 blocks ≈ 2499 ms). Plan Step 2 ticked.
