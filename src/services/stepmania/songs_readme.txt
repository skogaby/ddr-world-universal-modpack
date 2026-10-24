DDR World Universal Modpack -- StepMania packs
===============================================

Put StepMania packs here, one folder per pack, one folder per song:

  stepmania/Songs/<Pack Name>/<Song Name>/<song>.ssc (or .sm) + audio (+ art)

At boot the modpack indexes this folder, converts every playable chart to the
game's own formats and adds the songs to the music wheel in a STEPMANIA folder
(and a STEPMANIA entry in the VERSION filter). They are kept out of ALL MUSIC
unless "exclude_from_all_music" is set to false in mod-config.json. Scores for
these songs are never submitted to the server.

Notes
- .ssc is preferred over .sm when a song has both.
- Audio: OGG/Vorbis, MP3 or WAV, up to 256 MiB and 60 minutes.
- Levels (#METER) above 19 are shown as 19. Charts with warps, negative BPMs
  or negative stops are hidden; Edit charts, lights charts and double Beginner
  are skipped. Mines are played; lifts and fakes are dropped.
- Charts whose timing differs per difficulty are supported (the game loads a
  separate chart file per level).
- #SPEEDS / #SCROLLS scroll gimmicks are reproduced on screen; judgement is
  never affected. "scroll_gimmicks": false in mod-config.json turns them off.
- Titles: the original #TITLE is shown when the game's fonts can draw every
  character of it (Japanese, Korean, accented Latin, ...); otherwise the
  #TITLETRANSLIT. Sorting and the title filter always use the romanised title.
- Art: #JACKET, else #BACKGROUND, else #BANNER, else a file named *jacket*,
  *bg* or *bn*, else the pack's banner, else a placeholder. Non-square art is
  fitted on a blurred copy of itself.
- Auto-sync: with "auto_sync": true (default) each song is measured once
  against its audio and the result is applied through the game's own
  per-song judgement offset -- nothing in this folder or in the cache is
  edited. A song picked before it was measured is measured while the stage
  loads (a short pause). The sweep runs in the background between songs.
- Converted files live in data_mods/_cache/stepmania/ and are rebuilt when a
  song changes; that folder can be deleted at any time (the next boot rebuilds
  it and re-measures sync).
- The first boot after adding many songs takes longer (conversion runs in the
  background during startup, about 25 ms per song); later boots reuse the
  cache. Very large libraries may finish injecting on the second boot.
- Problems are reported in the game log as "StepMania: rejected '<Pack>/<Song>': <reason>".
  Offline check before copying a pack to the cabinet: tools/sm_pack_check in
  the modpack source.
- Updates of the modpack never touch this folder.
