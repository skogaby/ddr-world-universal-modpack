# Rough Idea — S-Marvelous Judgement

Add a new judgement to the game: **S-Marvelous**, with a **±12 ms** judgement
window — a discrete grade above the game's strictest stock grade (Marvelous,
±17 ms).

Maintainer constraints (carried over from the RE session):

- NOT a replacement of an existing judgement; NOT a shift-everything-down
  insertion.
- A discrete, net-new grade with its own graphics, gameplay flash, and results
  presentation.
- New art / AFP layout work is acceptable and in scope.

Starting basis: `docs/s_marvelous_judgement_research.md` — a completed RE deep
dive whose verdict is **Option C (presentation-layer discrete grade)**: the
engine's internal grade space stays untouched (an S-Marvelous IS a Marvelous to
score/EX/gauge/combo/save/ghost), and the discreteness (separate count, flash
art, results row) is implemented at the modpack layer, classified from the same
per-note ms delta the stock classifier used (`|Δ| ≤ 12` ⊂ ±17 by construction).
