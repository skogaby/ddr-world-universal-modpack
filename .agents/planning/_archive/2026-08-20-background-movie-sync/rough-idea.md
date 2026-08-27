# Rough Idea: Background Movie Sync

## Origin

While documenting how background movies interact with playback-speed and
training-mode features, the maintainer confirmed there is currently **no**
mechanism keeping the DirectShow background movie in sync with the song:

- Only SONG SPEED ≠ 100 % suppresses the movie (`MovieSuppressor::SongRate`
  in `src/services/movie_policy.rs` — the graph is never built).
- Every other timeline alteration desyncs the movie freely: Training Mode
  (SONG START > 0, FF/RW scrubs, LOOP SONG, restart-from-A) and Quick
  Restart's in-place reset leave the movie free-running.
- The suppression mechanism is a song-open-time decision (BuildGraph);
  mid-song alterations cannot be handled by it at all.

## The idea (as refined by the maintainer)

Keep the background movie **in sync** with the song by seeking it to
specific positions and adjusting its playback rate (DirectShow
`IMediaSeeking::SetPositions` / `SetRate` on the game's own player object).

**Primary deliverable ("Phase 2"): real sync.** A fallback
("Phase 1": stop/tear down the movie on the first timeline alteration)
is only built as the failure rung / contingency if real sync proves
infeasible on the cabinet.

### Option row (maintainer-specified)

- New **options-menu toggle**, registered as a **second child row of SONG
  SPEED** in the song-playback-speed mod — same `ShowWhen::NotEquals`
  visibility as PRESERVE SONG PITCH (shown only while that side's SONG
  SPEED ≠ 100 %).
- **Toggle ON:** the video is kept at non-100 % rates, its playback rate
  and position synced with the audio.
- **Toggle OFF:** video playback is disabled under any altered
  *playback speed* scenario (today's suppression behavior).
- **Regardless of the toggle, at 100 % speed:** the video is enabled and
  is position-synced (seeks) on timeline alterations — quick restart,
  training loops/scrubs/bounds. The option only dictates whether playback
  *speed* adjustment disables background videos.

### Rationale for the toggle

Some users will find slowed-down / sped-up videos unsettling; some will
want them. Both must be servable per-player.
