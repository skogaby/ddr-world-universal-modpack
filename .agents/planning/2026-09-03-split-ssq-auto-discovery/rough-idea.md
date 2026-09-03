# Rough Idea — Split SSQ Auto-Discovery

New top-level mod, "Split SSQ Auto-Discovery" (id `split-ssq-auto-discovery`).

DDR World stores some songs' charts across multiple SSQ files
(`<basename>_<N>.ssq`, N = 1..5 = Beginner..Challenge) because their difficulties
carry different tempo data. The game decides which file holds a given
(basename, difficulty) via a HARDCODED string-compare chain in `build_ssq_path`
(`gamemdx.dll` `0x1801B43F0` on 20260721). The table grows with each game
revision (19 → 27 → 35 entries across 20250805 → 20260721).

Players pinned to an older `gamemdx.dll` who load chart data from newer
revisions get wrong file choices for split songs the old binary doesn't know
about (Expert/Challenge charts missing → boot-time `ME1529 FILE CORRUPTION
ERROR` or empty charts). Replace the hardcoded table with runtime discovery of
`_N` files on disk (stock dir + LayeredFS mod folders), so any split song present
in the installed `musicdb.xml` loads correctly.

Constraints from the maintainer:
- No configuration — a single global on/off toggle in `mods`.
- Preserve the `toho` special case exactly: the play sequences randomize the
  basename to `toho1..toho4` before calling the builder; the resolver must not
  break that.

RE record: `docs/split_ssq_research.md`.
