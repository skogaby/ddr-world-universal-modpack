# Task: Spike fixture generator (E1/E3/E5/E9)

## Description
Build a throwaway host script that turns one StepMania song folder into a LayeredFS overlay
mod folder which injects that song into DDR World by hand: converted SSQ/XSB/XWB at the stock
asset paths, a jacket arc, a thumbnail arc for an existing custom series, and a
`startup_arc` overlay `musicdb.xml` carrying a hand-written `<music>` entry. Flags generate
N clone entries (E3 arena headroom) and omit art (E5). Also answer E9 offline.

## Background
Step 1 of the plan proves the visibility contract before any DLL code exists: an entry with
`limited_ary 0`, real `diffLv`, a 7-digit mcode and a 6-character basename must appear on the
wheel, preview, play and reach results online and offline. The game reads `musicdb.xml` out
of `startup.arc`; LayeredFS already overlays `data_mods/<mod>/arc/startup_arc/data/gamedata/
musicdb.xml` onto that arc (full-file overlay — the `musicdb.merged.xml` fragment path does
NOT reach the game's DB). The sibling `ddr-chart-tools` CLI converts SM5 → DDR with
`--song-code`. `scripts/arc_tool.py pack` builds ARC v1 containers; `scripts/unpack_arc.py`
extracts the stock musicdb. Series 30 (`WORLD RUBY`) is already configured in
`mod-config.json`, so the game requests `jacket_thumbnails_<ja|ua>_30.arc` at boot.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-10-20-stepmania-pack-loader/design/detailed-design.md (§2.3 constraints, §5.4 entry schema)

**Additional References (if relevant to this task):**
- docs/stepmania_runtime_packs_feasibility.md §4.2 (psmap/limits), §4.5 (visibility), §4.6 (jacket/thumbnail naming), §7.3 (entry synthesis), §16 (E1/E3/E5/E9)
- .agents/planning/2026-10-20-stepmania-pack-loader/research/ddr-chart-tools.md (CLI, song-code rule)
- .agents/learnings/learnings.md 2026-09-03 entries (startup.arc overlay is the only route)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Script lives at `.agents/planning/2026-10-20-stepmania-pack-loader/prototypes/make_spike.py`; Python 3 stdlib + Pillow (already used by `scripts/gen_*`); never enters `src/` or `scripts/`.
2. Inputs: song folder, output mod folder (default `$DDR_WORLD_INSTALL/data_mods/_spike_sm`), `--code` (default `zab12c`), `--mcode` (default 9000001), `--series` (default 30), `--clones N`, `--no-art`, `--title/--artist` overrides, path to the `ddr-chart-tools` binary (env `DDR_CHART_TOOLS`).
3. Conversion via the CLI (`--from-format SM5 --to-format DDR --song-code <code>`); outputs placed at `mdb_apx/ssq/<code>.ssq`, `sound/win/dance/<code>.xsb`, `sound/win/dance/<code>.xwb` under the mod folder.
4. Jacket: 512×512 PNG from the song's jacket/banner/background (first found) → `arc/jacket/<code>_jk.arc` with member `data/jacket/<code>_jk.png`. Thumbnail: 192×192 DXT1 DDS → `arc/thumbnail/jacket_thumbnails_ja_<series>.arc` AND `_ua_` with member `data/jacket/thumbnail/<code>_tn.dds`. If Pillow cannot write DXT1, write an uncompressed A8R8G8B8 DDS and note it (stock thumbnails are DXT1 — record whether uncompressed loads).
5. musicdb: extract stock `data/gamedata/musicdb.xml` from `$DDR_WORLD_INSTALL/data/arc/startup.arc`; if the install's LayeredFS already has a `startup_arc` overlay musicdb, start from that instead; append the entry (and clones) before `</mdb>`; write to `arc/startup_arc/data/gamedata/musicdb.xml`. Entry per design §5.4: `limited_ary 0` where a chart exists else `-1`; `diffLv` from the SM meters clamped 1..19 else 0; no `<region>`; `title`/`artist` ≤ 127 bytes.
6. Clones: unique mcodes (`base + i`) and basenames (6-char, `z` + base36 counter), all pointing at the same assets by symlink/copy of SSQ/XSB/XWB under the clone names (so the boot analysis sees a chart for each).
7. E9: grep every `texturelist.xml` reachable from the install's select-music IFS/arcs for `level_20` and print the finding.
8. Print a one-screen summary of what to observe on the cabinet (E1/E3/E5) and the exact log lines to grep.

## Dependencies
- `ddr-chart-tools` release binary present (sibling checkout, `target/release/ddr-chart-tools`).
- A StepMania song folder supplied by the maintainer.
- Pillow for image work (inferred from `scripts/gen_option_labels.py` usage).

## Implementation Approach
1. Parse args; locate the install via `DDR_WORLD_INSTALL` (no hard-coded fallback).
2. Run the CLI into a temp dir; move outputs to the stock paths under the mod folder.
3. Parse the simfile just enough for title/artist/meters per chart (reuse nothing from the DLL; a 60-line MSD reader is fine for a spike).
4. Build the jacket PNG and the thumbnail DDS; pack arcs with `scripts/arc_tool.py pack` (import it or shell out).
5. Build the musicdb overlay; validate the entry's byte lengths; write.
6. Emit clones if requested; emit the E9 report; print the observation checklist.

## Acceptance Criteria

1. **Overlay produced**
   - Given a song folder with an `.ssc`/`.sm` and OGG audio
   - When `make_spike.py <song> --code zab12c` runs
   - Then the mod folder holds the SSQ/XSB/XWB at stock paths, `zab12c_jk.arc`, both thumbnail arcs, and a `musicdb.xml` whose last `<music>` has mcode 9000001, basename `zab12c`, `limited_ary` 0 for exactly the converted charts' slots and `-1` elsewhere, and `diffLv` 1..19 on those slots and 0 elsewhere

2. **Clones**
   - Given `--clones 500`
   - When the script runs
   - Then 500 additional entries with unique mcodes/basenames are appended and their SSQ/XSB/XWB exist under the clone names

3. **No-art**
   - Given `--no-art`
   - When the script runs
   - Then no jacket/thumbnail arcs are written and the summary says E5 is being tested

4. **E9**
   - When the script runs
   - Then it prints whether any `level_20` texture exists in the select-music texturelists

5. **No machine paths in artifacts**
   - Then the script prints paths `~`-relative and nothing under the planning directory contains `/Users/` or `/home/`

## Metadata
- **Complexity**: Medium
- **Labels**: spike, fixtures, layeredfs, musicdb
- **Required Skills**: Python, DDR asset formats (ARC/DDS), musicdb schema
- **Generated By**: code-task-generator 2026-10-21
- **Source Plan**: .agents/planning/2026-10-20-stepmania-pack-loader/implementation/plan.md
- **Plan Step**: Step 1: Spike E1/E3/E5/E9 — hand-built injected song via overlay
