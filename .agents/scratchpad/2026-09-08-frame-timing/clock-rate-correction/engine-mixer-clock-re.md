# XACT 2.10 Mixer / Output Clock — RE record (MOVED)

Updated: 2026-09-09

The durable record now lives at **`docs/audio_clock_research.md`** (engine mixer
and DS backend layout, source-node onset, libavs RDTSC tick calibration,
CrossOver cursor granularity, and the game's vblank-locked visual pipeline).
The proposed design is `design.md` in this directory.

Ghidra note: function definitions were CREATED (no renames) in
`xactengine2_10.dll` for tiny thunks auto-analysis had missed: 0x435A30,
0x4358E0, 0x4358F0, 0x436560, 0x435A40, 0x435CA0, 0x41D150, 0x438570, 0x439190.
