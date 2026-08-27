# Rough Idea: Auto-Calibration (StepMania-style)

Implement an "Auto-calibration" feature in DDR World, similar to StepMania's.

- New in-game user option under the "Power User Settings" header. When enabled,
  calibration mode is active during the next song playthrough.
- During the calibration song's gameplay, "Calibrating..." appears as a toast
  text on the bottom through the existing toast system.
- Once the song is over, the calibration setting is automatically flipped back
  to OFF so the next song doesn't recalibrate accidentally.

Mechanism:
- We already have hooks into the judgement systems during gameplay and already
  know the millisecond errors for each step.
- If the player plays with consistent timing but the global cabinet offset is
  off-sync from their hardware setup, steps will consistently skew early or
  late.
- At the end of the song, analyze the full set of timing errors for the steps
  and calculate what the global timing offset "should" be.
- The value to adjust is the `sound offset` value configured globally at the
  cabinet level — we already have a hook in place for manual manipulation of it
  (timing_offsets).
- If the player times steps to the audio during calibration, afterwards timing
  to the audio in subsequent runs should be fully reliable.

UI/persistence:
- Option present in both the in-game options menu and the overlay 000 menu
  according to mod-config.json ordering.
- Value mirrored across both cabinet sides: if one player enables/disables
  calibration mode, the other side's setting flips with it. After the
  calibration song ends, both sides flip to OFF.
