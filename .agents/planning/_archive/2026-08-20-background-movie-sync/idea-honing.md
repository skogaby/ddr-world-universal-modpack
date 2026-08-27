# Idea Honing: Background Movie Sync

Decision register. Statuses: Proposed / Accepted / Overridden / Assumed / Open.

Register approved wholesale by maintainer 2026-08-20 (D9 was Open pending research; resolved same day by `research/movie-sync-re.md` §4).

| ID | Decision | Why it matters | Recommendation | Status |
|----|----------|----------------|----------------|--------|
| D1 | Toggle semantics | Defines the feature's user-visible contract | Child bool row governs ONLY the rate≠100 % movie behavior: ON = build graph + sync rate & position; OFF = suppress (today). At 100 % the movie always plays and is position-synced on timeline alterations, toggle-independent | Accepted (maintainer-specified) |
| D2 | 100 % position-sync is an always-on service capability | Determines whether restarts/training seeks sync movies even with the song-playback-speed mod disabled | Yes — service-level, active whenever the movie hook is live and a movie is playing; independent of the speed mod | Accepted 2026-08-20 |
| D3 | Toggle default value | First-run behavior for every player | OFF (non-100 % keeps suppressing = status quo; synced-rate video is opt-in) | Accepted 2026-08-20 |
| D4 | Row id + label | Wire field, texture names, UI wording | id `sync_movie`, label "SYNC BACKGROUND VIDEO", OFF/ON previews | Accepted 2026-08-20 |
| D5 | Persistence | Server schema + profile follow | `PersistMode::Full` → wire `mod_sync_movie` → bemani-buddy `opt_mod_sync_movie` (migration 018), stored verbatim | Accepted 2026-08-20 |
| D6 | Governing side | Whose toggle wins (movie is cabinet-wide) | The committed rate arm's side (arms are single-entered-side; versus/course force identity, toggle moot) | Accepted 2026-08-20 |
| D7 | Failure ladder | Fail-open shape; where "Phase 1" lives | Rate-sync failure (no IMediaSeeking / SetRate fails) ⇒ tear down movie (vtbl +0x38) = suppressed-equivalent, one WARN. 100 % seek failure ⇒ leave movie desynced (today's behavior), one WARN | Accepted 2026-08-20 |
| D8 | Platform bar | What blocks ship | Windows cabinet = must work; CrossOver fallback mode = best-effort fail-open (validated but Wine quirks don't block); CrossOver suppress mode = feature inert by construction | Accepted 2026-08-20 |
| D9 | Movie shorter than song / seek past end | Seek clamping & completion semantics | Mirror the player's own loop flag (`+0x16`, read at capture): loop set ⇒ positions map modulo duration; loop clear ⇒ clamp (stock stops at EC_COMPLETE anyway). Research-resolved: the game loops movies natively by absolute seek-to-0 on EC_COMPLETE when request flag bit 0 is set (`research/movie-sync-re.md` §4) | Accepted 2026-08-20 (research-resolved) |
| D10 | Restart-delay sync point | When to sync during a delayed restart | Issue the position sync at the re-anchor moment (the idempotent second 0x1044), not at reset time — avoids pause/run choreography | Accepted 2026-08-20 |
| D11 | Mid-song rate changes | SetRate call count | None exist (rate latched per song) — exactly one SetRate per song at start | Assumed |
| D12 | Code placement | Module layout | Sync engine as a movie_policy sub-capability (or sibling `services/movie_sync.rs`) consuming the captured player ptr; the option row lives in `src/mods/song_playback_speed.rs` | Assumed |
| D13 | Interaction with NonNativeOs suppress mode | Wine boxes without movie support | Suppress mode always wins — toggle ON cannot force a movie there | Assumed |

---

## D1 — Toggle semantics

Maintainer-specified (2026-08-20): "If the toggle is enabled, the video is
kept and its playback rate and position are kept in sync with the audio. If
the option is disabled, then video playback is disabled under any altered
*playback speed* scenarios. Regardless of the toggle, if the playback speed
is 100 %, then the video should be enabled and we simply seek to the correct
position. The option mainly just dictates whether playback speed adjustment
should disable background videos." Row is the 2nd child of SONG SPEED with
PRESERVE SONG PITCH's `ShowWhen::NotEquals` visibility.

## D2 — Always-on 100 % position sync

Quick Restart and Training Mode exist independently of the
song-playback-speed mod. The maintainer's "regardless of the toggle" implies
position sync is unconditional; recommending it also be independent of the
speed mod's enable state (service capability, like score_guard).

## D3 — Default OFF

Rationale: some users find rate-shifted video unsettling (maintainer's own
motivation for the toggle); OFF preserves today's suppression behavior for
everyone who never opens the menu. Rejected alternative: default ON
(showcases the feature but changes behavior under players' feet).

## D7 — Failure ladder

The teardown rung IS the former "Phase 1" (stop-on-alteration) mechanism —
it gets built as the failure path of Phase 2 rather than as a separate
deliverable, satisfying the "Phase 1 only as fallback" directive. At 100 %
a desynced background after a failed seek is milder than killing the video
mid-song; keep today's behavior there.

## D10 — Restart-delay sync point

The in-place reset future-dates the timing anchor (music count runs
negative during the countdown). Seeking the movie at reset time would leave
it playing during the countdown (or need pause/run choreography). The
prepared-cue re-anchor is an existing, idempotent, exactly-at-audio-start
moment; syncing there keeps the movie logic stateless. Cost: the movie shows
stale/free-running frames during the countdown window (≤10 s, cosmetic).

---

Readiness Confirmed 2026-08-20 — maintainer approved proceeding to detailed
design with all 13 decisions Accepted/Assumed and research complete
(`research/orientation.md`, `research/movie-sync-re.md`).

---

## D14 — Wine rate handling (added 2026-08-21; SUPERSEDED 2026-08-24)

| ID | Decision | Why it matters | Resolution | Status |
|----|----------|----------------|------------|--------|
| D14 | Rate sync platform scope | Wine cannot rate-adjust a movie | Under Wine, SONG SPEED ≠ 100 % suppresses the movie REGARDLESS of the SYNC BACKGROUND VIDEO toggle; real `SetRate` (readback-verified) is Windows-only. Position sync (seeks/scrubs/restarts at 100 %) stays on both platforms | Accepted 2026-08-21 (maintainer-specified); **SUPERSEDED 2026-08-24 (maintainer-approved)** |

Context: Wine builtin quartz's `SetRate` is a silent no-op (three-way
live-confirmed); native quartz was tried (load blocker solved via the
`ntdll_state_shim` IAT patch) and hard-locked the game in its VMR×wined3d
path — experiment abandoned, bottle reverted. Seek-based drift correction
was proposed and REJECTED by the maintainer ("extremely visually stuttery,
I'd rather not pursue that path"). Consequence for D7's ladder: on Wine the
arm/commit seam keeps the SongRate suppressor set regardless of the latched
toggle (the toggle is effectively Windows-only); the readback-verified
stop rung remains as Windows' failure path.

**Supersession (2026-08-24):** D14's rationale was SetRate-specific, and
`SetRate` turned out to be dead on REAL WINDOWS too (the game's own WMV
chain refuses it — Windows tests #1–#3). The replacement mechanism (FR-3
second amendment: movie_sync's scaled reference-clock proxy, adaptive
in-place/escalated `SetSyncSource` install) is platform-uniform and was
cabinet-validated on Windows (test #5) AND CrossOver (trial #2, incl.
off-rate scrubs). The toggle now works on both platforms; under Wine,
movies require `non_native_os_support.movie_mode="fallback"` to exist at
all. D14's dead ends stay authoritative: never retry native quartz, never
seek-based drift correction.
