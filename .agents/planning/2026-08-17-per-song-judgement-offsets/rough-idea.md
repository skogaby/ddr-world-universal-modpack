# Rough Idea: Per-Song Judgement Offsets

New top-level mod: **Per-Song Judgement Offsets**.

Players can set their judgement offset per-song, and per-player / per-profile.

## Background

The game's stock options set already includes a **JUDGEMENT OFFSET** option that
lets the user +/- the time at which the game judges the notes for the song.
However, that setting is global — less than ideal because not all songs are
synced perfectly between the chart and the audio. Many players effectively
maintain a private list of special offset values per song and manually adjust
the stock option between songs according to their lists.

## Desired behavior

- Add a new option called **CURRENT SONG OFFSET** which, if set, *overrides*
  the player's stock JUDGEMENT OFFSET value with the value set in the new
  option.
- Crucially, it should apply **only for the currently selected song on the song
  wheel** (we already have visibility into the highlighted song from the
  music-wheel-song-length mod).
- When the user navigates to a different song:
  - load whatever value they had saved for that song, or
  - if none was set, the new option shows OFF and the player's stock
    JUDGEMENT OFFSET value applies.

## Persistence

- **Local:** a dedicated `judgement_offsets.csv` living alongside
  `mod-config.json` (NOT inside mod-config.json). Three columns:
  `song code, p1_offset, p2_offset`. The offset columns can be empty; when
  empty, the player's default judgement offset applies.
- **Server:** persist the offset values to the backend server too, similar to
  how other custom options have local JSON persistence that can be overridden
  by server-side values when the server supports the option. Encode as a single
  long string of the form `song_code|offset|code2|offset2|...`; the server
  dumps the string back to the client, which decodes it and sets the values in
  memory as needed.

## Scope notes

- Backend changes are in scope: the maintainer's own backend server codebase is
  the sibling `bemani-buddy` project (recent commits there show the pattern for
  adding new profile fields).
- Ghidra is available for RE if needed.
