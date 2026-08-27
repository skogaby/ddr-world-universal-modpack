# Orientation: Background Movie Sync

Findings from the initial survey (2026-08-20), before requirements
clarification. Sources cited per item.

## 1. Current movie handling

`src/services/movie_policy.rs` owns the sole `GenericDetour` on gamemdx
`DShowPlayer::BuildGraph` (AOB `movie_build_graph`, `src/core/signatures.rs`
~line 1241). Contributor model:

- `MovieSuppressor::SongRate` — set tentatively at the scene-26 arm
  (`src/services/song_rate/runtime.rs:116`, arm log at runtime.rs:662 says
  "movie tentatively suppressed"), confirmed/cleared at commit
  (`src/services/song_rate/wavebank_hook.rs:532`). **Full suppression always
  wins over fallback mode** (`MoviePolicy::should_suppress`).
- `MovieSuppressor::NonNativeOs` — suppress mode (never build) or fallback
  mode (build real graph, fake success epilogue on FAILED hr). Set by
  `src/mods/non_native_os_support.rs`.
- Suppression fakes exactly one side effect: player state `+0x8 = 3`
  ("opened"); the `opened` byte `+0x14` stays 0 so the per-frame get-frame
  path early-returns before any COM pointer.
- `CallOutcome::{Passthrough, Suppressed, FallbackFaked}` — a Passthrough
  is the "movie really playing" signal a sync engine can capture on.

**Coverage gap (the motivating observation):** only rate ≠ 100 % suppresses.
Desync scenarios NOT covered: Training Mode (identity binding — explicitly
"no movie suppression", `src/mods/training_mode/mod.rs:9`): SONG START > 0,
FF/RW scrubs (pinpad 7/9), LOOP SONG wrap, restart-from-A; Quick Restart's
in-place reset (`src/services/song_reset/`); restart-delay variant.
Structurally, BuildGraph fires once at gameplay entry — mid-song alterations
cannot be addressed by the suppression flag at all.

## 2. DShowPlayer reverse engineering (already done)

`.agents/planning/20260721-non-native-os-support/research/movie-player-re.md`
maps the player completely (vftable `0x18038a6c8`, object 0xC8 bytes,
20260616 addresses file-relative to 0x180000000):

| Offset | Field |
|---|---|
| +0x08 | state dword (3 = opened) |
| +0x0C | command dword (play/pause/stop bits) |
| +0x10 | seek position float (game-side; vtbl +0x48 writes it) |
| +0x14 | opened byte (1 only after a real graph build; gates get-frame COM path) |
| +0x48 | `IGraphBuilder*` |
| +0x50..+0x68 | QI'd `IMediaControl`/`IMediaEvent`/`IMediaSeeking`/`IBasicAudio`-family pointers — **exact slot assignment not yet pinned** (BuildGraph step 4 QIs them; one Ghidra pass needed) |
| vtbl +0x38 | OpenFile = teardown (`FUN_18023b270`, fully null-guarded) + tail-call to BuildGraph |
| vtbl +0x48 | seek — writes `+0x10` only ("safe" on stub; **real consumer of +0x10 unknown** — worth checking whether the game has its own seek plumbing we can reuse) |

Status machine: `Dx9Movie_Update` (`0x180215F50`) — shared 0x48-byte struct,
STATUS at +0x14 (1 opening, 6 ready, 7 playing, ...); case 1 only advances
when player state == 3 (the movie-ready gate the suppression fake satisfies).

`docs/song_playback_speed.md` §8.4 already anticipated this feature: "A later
native-only enhancement could investigate DirectShow `IMediaSeeking::SetRate`".

## 3. Sync event sources (all already exist)

- **Rate:** `RateSnapshot` committed once per song (before gameplay actors
  start; log order shows commit before the BuildGraph diag). Rate is latched
  per song — **no mid-song rate changes exist**, so one `SetRate` per song
  suffices. Governing side = the single entered side of the arm
  (`src/services/song_rate/lifecycle.rs`; versus/course fail closed to
  identity).
- **Seeks/resets:** `song_reset::on_song_reset(t_ms)` subscriber API
  (`src/services/song_reset/mod.rs:1057`, notified at mod.rs:1050/1612/1893/2152)
  fires on the game thread with the content-domain timestamp for: quick
  restart (t=0 or delayed), training restart-from-A, LOOP SONG wrap, FF/RW
  scrubs (all dispatched through the seek transaction). Movie timeline is
  content-domain ⇒ t_ms maps 1:1 to movie position.
- **Restart delay:** the reset future-dates the timing anchor (music count
  runs negative); an idempotent second `0x1044` re-anchor fires when the cue
  is prepared — a natural second sync point.
- **Training SONG START > 0:** bounds resolved at gameplay entry
  (`TrainingMode: row-derived bounds resolved` log); ordering vs BuildGraph
  within the same second — needs verification if an open-time initial seek
  is wanted.

## 4. The child-row pattern to copy (PRESERVE SONG PITCH)

`src/mods/song_playback_speed.rs:48-51` and 160-200:

- `RegisterSpec::bool_toggle(OPT_PRESERVE_PITCH)` registered immediately
  after the parent, `.show_when(ShowWhen::NotEquals { parent_id:
  OPT_SONG_SPEED, value: IDENTITY_PERCENT })`.
- Non-fatal registration failure (warn and continue).
- Re-enable re-seeds per-side atomics from registry values (Duplicate does
  not re-fire `on_change`).
- Persistence: `PersistMode::Full` → wire `mod_preserve_pitch` → bemani-buddy
  `opt_mod_preserve_pitch` (migration 014). Latest migration in that repo is
  017 (`opt_mod_skip_results_fast_exit`).
- Textures: `seop_item_preserve_pitch` + `seop_image_preserve_pitch_{off,on}`
  via `scripts/gen_option_labels.py` + `scripts/option_strings.py` (en/ja/ko).

## 5. Platform context

- **Windows cabinet (primary):** movies always play; quartz + stock WMV
  (VC-1). SetRate/SetPositions support on this graph is the main live-test
  risk.
- **CrossOver fallback mode:** movies play via native WM runtime (VC-1,
  `docs/native_wm_runtime_bottle_setup.md`) or winegstreamer (H.264
  conversions). Requires spice2x `-audiohookdisable` (the 2026-08-20 crash
  diagnosed in this session was fallback mode without the flag — quartz's
  devenum audio-renderer enumeration through spice2x's wrapped WASAPI objects
  AVs in Wine's builtin winmm). Wine quartz seek/rate behavior unproven.
- **CrossOver suppress mode:** no movie is ever built — the whole feature is
  inert there by construction.

## 6. Known unknowns (research items)

1. **IMediaSeeking slot** within +0x50..+0x68 — one Ghidra pass on
   BuildGraph's QI sequence.
2. **Movie completion handling** — does the game loop movies via
   `IMediaEvent` EC_COMPLETE, or hold/stop? Determines seek-past-end and
   long-song mapping (clamp vs modulo). Ghidra: the IMediaEvent consumer /
   `Dx9Movie_Update`.
3. **Do stock movies carry audio streams?** If yes, the graph includes an
   audio renderer whose SetRate support differs from video-only graphs.
   Cheap local check: ffprobe on `data/mdb_apx/movie/*.wmv` in the CrossOver
   bottle install.
4. **SetRate / SetPositions actual behavior** on the real graphs (Windows
   quartz + WM ASF Reader; Wine quartz) — keyframe snap granularity, rate
   limits, mid-run seek robustness. Only provable live; front-load in the
   plan as a diagnostic probe step.
5. **Whether the game's own vtbl +0x48 seek (+0x10 float) is wired** to
   anything — if the game has dormant seek plumbing, reusing it may be
   simpler than raw COM calls.
6. **Decode headroom at 175 %** (VC-1 software decode under Wine; cabinet
   GPU-assisted decode) — validate live.

## 7. Proposed approach shape (pre-register)

Extend the movie machinery with a sync capability: capture the live player
pointer on `CallOutcome::Passthrough`, pin `IMediaSeeking`, issue one
`SetRate(effective_rate)` after commit when the new toggle is ON (instead of
suppressing), subscribe to `on_song_reset` for position sync (always, at any
rate), fail-open at every rung: rate-sync failure ⇒ tear down the movie
(vtbl +0x38 teardown = the "Phase 1" mechanism, built as the failure rung);
100 % seek failure ⇒ leave the movie desynced (today's behavior) with one
WARN.
