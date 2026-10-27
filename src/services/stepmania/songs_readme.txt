DDR World Universal Modpack -- StepMania packs
===============================================

Put StepMania packs here, one folder per pack, one folder per song:

  stepmania/Songs/<Pack Name>/<Song Name>/<song>.ssc (or .sm) + audio (+ art)

Then run ddr_world_hook_prep.exe (in the game folder, next to spice64.exe) --
or let gamestart.bat run it on the line before spice64.exe, so every launch
is up to date. The tool converts every playable chart to the game's own
formats, measures each song's sync, and reports what it did; the game itself
never converts, it only loads what the tool prepared. The songs then appear on
the music wheel in a STEPMANIA folder (and a STEPMANIA entry in the VERSION
filter). They are kept out of ALL MUSIC unless "exclude_from_all_music" is set
to false in mod-config.json. Scores for these songs are never submitted to the
server.

Run the tool again after adding, removing or editing a pack. A run with
nothing to do takes about a second; a new 1,000-song library takes about half
a minute to convert and a couple of minutes to measure. It refuses to run
while the game is running.

What the game loads
- There is a per-boot ceiling of 30,000 injected songs (the game's wheel
  thumbnail table cannot hold more): the first N by pack and song folder
  name. The tool tells you how many you have and which packs are left out,
  so you can trim before booting. The modpack's Song Limit Expansion mod
  must stay on: it lifts five of the game's fixed-size limits at boot, and
  with it off nothing is injected.
- Every thumbnail stays in video memory while the game runs (about 18 KB per
  song, so a 14,000-song library takes ~250 MB), and the game's own
  one-time analysis of newly indexed charts runs on the first boot after the
  tool (about a minute per 10,000 songs); every boot after that replays it
  from the modpack's cache and is as fast as a stock boot.
- A song whose files changed since the last tool run is skipped by the game
  (one line in the game log says how many, and to run the tool again).

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
- Auto-sync: with "auto_sync": true (default) the tool measures each song once
  against its audio and the game applies the result through its own per-song
  judgement offset -- nothing in this folder or in the cache is edited. A song
  the tool has not measured (run with --no-sync) plays as authored.
- Converted files live in data_mods/_cache/stepmania/; that folder can be
  deleted at any time (the next tool run rebuilds it). Interrupting the tool
  is safe: it saves its progress as it goes.
- Problems are reported by the tool as "rejected '<Pack>/<Song>': <reason>"
  and in ddr_world_hook_prep.log in the game folder. To vet a pack before
  copying it to the cabinet: ddr_world_hook_prep.exe --dry-run <song or pack
  folder> --out <folder>.
- Updates of the modpack never touch this folder or the cache.
