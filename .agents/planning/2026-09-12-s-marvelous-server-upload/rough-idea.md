# Rough Idea — S-Marvelous server-side awareness (score-upload `s_marv` node)

Captured 2026-09-12 from the maintainer.

## Context

The shipped S-Marvelous Judgement mod (`src/mods/s_marvelous/`, planning
`.agents/planning/2026-08-29-s-marvelous-judgement/`) is strictly a
presentation-layer classification: the engine's grade space is untouched, so the
stage record's Marvelous counter, the score save payload, and the ghost streams
are all bit-identical to stock. A backend server therefore sees every
S-Marvelous hit as an ordinary Marvelous and cannot distinguish them (the
original design's D21/FR8 explicitly scoped server-side surfaces OUT for that
reason).

## The addition

Make it possible for a backend that supports custom network shapes (the
maintainer's own bemani-buddy, in particular) to be aware of the ACTUAL
S-Marvelous data, so it can be reflected on a web UI later.

Shape as pictured by the maintainer:

- The data that is sent today stays exactly as-is (stock-compatible packet —
  a stock server keeps working, and the stock Marvelous count still includes
  the S-Marvelous hits).
- Add a dedicated `s_marv` node to the score-upload packet, in the same spirit
  as the `custom_options` node the DLL already adds to profile saves.
- Inside `s_marv`, carry duplicates of everything in the normal packet that
  would need to be S-Marvelous-aware / recomputed. At minimum:
  - the per-grade judgement counts (or at least Marvelous and S-Marvelous,
    since every other grade is unchanged), and
  - the ghost data, using a NEW designator character for S-Marvelous steps in
    the ghost stream.
- The FAST/SLOW breakdown, if it is part of the stock upload — with a tier
  above Marvelous the FAST/SLOW rule moves (stock exempts Marvelous; the mod
  exempts S-Marvelous and counts loose Marvelous as FAST/SLOW), so those
  counts need an S-Marv-aware duplicate too (2026-09-12 addendum).
- Rule of thumb (maintainer, 2026-09-12): `s_marv` should include EVERYTHING
  in a stock score upload that would need to be recalculated with
  S-Marvelous awareness — inspect the score save packet to enumerate them.

## Scope (revised 2026-09-12)

- DLL side: the `s_marv` node emission + the wire contract.
- **Backend side is IN scope**: the maintainer's bemani-buddy backend
  (sibling checkout at `../bemani-buddy`) needs the matching changes — parse
  and persist the `s_marv` node (migration, model, handler), and whatever
  minimal plumbing lets a web UI read it later. Its git history is a useful
  reference for how earlier `mod_*` fields were added.
- **Echo-back (added 2026-09-12):** since the backend can now hold S-MFC
  flags, the client should learn about them on score loads and badge the
  song at song select accordingly (new texture(s) in the modpack). The
  maintainer's first sketch was "overwrite `clearkind` with the S-Marv
  clearkind on load"; the pacemaker ghost can stay stock (score-driven).
- **bemani-buddy codegen hygiene (added 2026-09-12):** the maintainer learned
  that `models/ddr_world/playdata_3.json` is stale vs the hand-edited
  generated `playdata_3.rs`; fixing that and re-generating to prove it is
  part of this work.
- Ghost designator: **`'8'`** (2026-09-12; supersedes `'S'`).

## Explicitly not in scope

- Changing what the stock fields contain (a stock server must keep working).
- The web UI presentation itself (later; this pass only makes the data
  available to it).
