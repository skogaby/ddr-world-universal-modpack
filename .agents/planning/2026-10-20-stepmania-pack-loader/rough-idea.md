# Rough Idea: StepMania Pack Loader

Design a new top-level mod for the modpack that adds compatibility to load StepMania
song packs natively, without converting them to DDR's format (XWB audio, SSQ charts).

- A directory where a user can create folders for song packs, analogous to the `songs`
  directory in a StepMania installation. Users drop in any StepMania packs and the game
  loads them for gameplay, without the songs having to be present or defined in
  `musicdb.xml`.
- Define a custom folder in the game that holds all the StepMania-injected songs. The
  codebase already knows how to do this via the existing Custom Folders mod.
- Since the musicdb and the server will not be aware of these songs, scores must always
  be suppressed from backend server uploads when one of these songs is played.
- For file-format decoding/encoding, `ddr-chart-tools` (sibling checkout,
  `~/Projects/ddr-chart-tools`) is the starting point — it already supports
  bidirectional conversion between DDR and StepMania chart and audio formats.
