# Rough idea — Multiplayer Bot "Target Score" tier

Captured 2026-09-14 (PDD lite pass; feature on top of the 2026-09-13 Multiplayer Bot).

Add an option tier to the versus bot after the numeric levels 1–10, called
**TARGET SCORE**. Instead of the skill model, the bot replays the GHOST DATA the
game has loaded for the human's current pacemaker target (own PB, rival score,
world/area/machine record — whatever is selected at song select). The result is
a live 2P VERSUS play against a replay of any target score.

Constraints the maintainer stated:

- The ghost data carries NO timing — it is one grade byte per note (Marvelous,
  Perfect, Great, Good, Miss, O.K., N.G.). Each step's judge offset is therefore
  randomised INSIDE the window of the ghost's grade, so the exact judgement is
  reproduced while FAST/SLOW feels natural.
- **S-Marvelous is excluded entirely**: when the S-Marvelous mod is enabled, a
  ghost Marvelous is sampled strictly OUTSIDE the S-Marv window (between the
  S-Marv bound and the Marvelous bound), so the target replay never produces an
  S-Marvelous. (The numeric levels are untouched.)
- **No backend work**: bemani-buddy will not store the bot options. Every bot
  option persists LOCALLY only (the JSON cache), never over the network.
- **No new chip textures**: the level row becomes a discrete "enum-like"
  selector whose values render as TEXT — `Level 1` … `Level 10`, then
  `Target Score`.
