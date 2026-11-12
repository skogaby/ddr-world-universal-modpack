# Rough idea

A standalone executable the user runs before the game (from `gamestart.bat`, like
the updater, or by double-click) that makes sure every StepMania song under
`stepmania/Songs/` is fully indexed and converted, and that its auto-sync
measurement is taken, before the game is ever booted — so the in-game loader's
`startup.arc` open is always warm and the cabinet never sits on a black screen
converting.

Origin (2026-10-24): a tester symlinked ~14,000 song folders into
`stepmania/Songs/`. The in-game index converted everything to the cache, but the
game never reached a frame in 15 minutes. The in-game guard rails that came out
of that (injection ceiling, 64 MiB XML arena) are already in the DLL; this tool
addresses the remaining experience problem — a huge library's first boot is a
multi-minute silent wait inside the game process, and the sync sweep for
thousands of songs runs for hours in the background of play.

Possible later extension (explicitly a separate feature): compute sync
corrections for the stock songs too.

## Revision (2026-10-24, maintainer)

The tool is not a pre-warmer for the in-game index — it **replaces** it. The DLL
stops indexing entirely: no index thread, no conversion, no `index_timeout_s`, no
background sync sweep. The *only* way the game loads StepMania songs is from a
cache the tool wrote beforehand. Goals: no race conditions between two writers,
no background job during play, a smaller and simpler DLL.
