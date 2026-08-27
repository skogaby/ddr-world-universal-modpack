# Rough Idea: Perspective Expansion (Distant, Incoming, Space)

Extend the recently shipped `player_perspective` mod (which ported the ITGMania/
StepMania/SimplyLove "Hallway" perspective to DDR World) with the remaining
player perspectives from that family:

- **Distant** — camera tilted so the lane recedes upward/away (overhead-ish tilt,
  opposite vanishing direction from Hallway)
- **Incoming** — tilted the other way; arrows appear to come toward the player
- **Space** — the classic StepMania "Space" skew/tilt combination

Reference implementations live in the ITGMania source (checked out at
`~/Desktop/Projects/itgmania`) and Simply Love SM5 theme
(`~/Desktop/Projects/Simply-Love-SM5`). One of these contains the actual
perspective math (StepMania's `PlayerOptions` / `ArrowEffects` code, and Simply
Love's per-player perspective options UI).

Goal of this PDD pass: a light plan for porting the remaining perspectives onto
the existing per-player perspective infrastructure (PERSPECTIVE enum row,
pass_rewrite C1 mechanism, perspective VS, guideline CPU-side map, cull-window
contribution).
