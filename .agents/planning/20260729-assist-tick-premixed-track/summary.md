# Summary — Assist Tick: Pre-Mixed Tick Track

## What this is

Replaces the shipped Assist Tick mod's per-tick `se_play` trigger (which stacks frame-boundary
and XACT packet-grid quantizers, causing audible jitter in 8th/16th bursts) with a **pre-mixed
whole-song tick track**: one continuous mono waveform, every clap mixed at its exact sample
position, played once as a single cue through the game's own XACT engine. Clap spacing becomes
sample-exact; the clap is timed to the **judgement moment** (`t_i + JUDGMENT_TIMING −
SOUND_OFFSET − m0`), derived entirely from game state — no operator tuning knob.

## Artifacts

- `rough-idea.md` — the problem + proposal.
- `idea-honing.md` — decision register D1–D11 (all Accepted), readiness confirmed 2026-07-29.
- `research/orientation.md` — Step-2 blind-spot pass + RE spike summary.
- `research/ra-rb-timing-chain.md` — the offset chain: JUDGMENT TIMING is judge-compare-time
  (not inherited); `mc = tick − sound_offset`; DISPLAY/RENDER/INPUT are display-count-only.
- `research/rc-rd-re-lifecycle-synthesis.md` — zero within-song drift (co-rendered voices);
  rewrite-in-place + immediate-stop lifecycle; 300 s/~7.3 MB capacity; synthesis cost + port
  scope.
- `design/detailed-design.md` — Approved 2026-07-29. Three components (`game_audio` additions,
  new `se_bank_synth`, reworked `mods/assist_tick`), data models, error handling, testing, RE
  appendix, rejected alternatives.
- `implementation/plan.md` — Approved 2026-07-29. Five risk-ordered steps, each demoable.

## Next steps

1. Run the `code-task-generator` sop against `implementation/plan.md` to produce task files (one
   step at a time — later steps' tasks benefit from what earlier steps learn, especially Step 2's
   live confirmation of rewrite-in-place).
2. Run `code-assist` on each task in order.
3. Maintain `progress.md` in this directory as the live resume point (repo convention).

## Assumptions to watch during implementation

- **JUDGMENT TIMING sign** (Step 4): implemented as one named constant; validated by the ±100 ms
  listening test — flip if backwards.
- **Rewrite-in-place vs a race** (Step 2): if the stop→reap→rewrite window proves racy live,
  switch to the double-buffer fallback before Step 3.
- **ADPCM SNR 17.4 dB** adequate for a clap (carried from the shipped feature).
- **Reserved contingency**: a config-only trim key (no UI) if a residual fresh-start constant
  shows up on a track voice (≈38 ms of per-trigger asymmetry was measured on the CrossOver setup
  for the *old* per-cue path).
