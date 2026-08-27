# Orientation: Song Playback Speed

## Idea Understanding

The feature lets a player choose a fixed playback rate for the next gameplay
song. Audio, chart progression, rendering, judgment, and dependent mod features
must remain synchronized. Any non-stock rate is assisted play and must not
produce a trusted score.

`docs/song_playback_speed.md` establishes a credible mechanism:

1. patch the main sound's authored pitch in the selected song's XACT sound bank;
2. scale the central gameplay `music_count` by the pitch-quantized effective
   XACT rate;
3. keep chart timestamps in their native content-time domain;
4. adapt consumers that run on a separate wall-time clock;
5. suppress competitive score data for every non-100% play.

The preliminary research is not an approved requirement or design. Its static
XACT and `gamemdx.dll` findings still require a diagnostic cabinet test.

## Existing Code That Shapes the Feature

### Audio replacement

- `src/services/avs_layeredfs/file_hooks.rs::find_mod_replacement` is the single
  replacement decision shared by AVS open, lstat, and path conversion. A song
  XSB generator must extend this path; it must not install another AVS detour.
- `src/services/avs_layeredfs/shader_synthesis.rs` is the nearest generated-file
  precedent: read original AVS bytes, derive deterministic content, cache it,
  and return a generated path.
- `src/services/avs_layeredfs/cache_hasher.rs` hashes paths and host mtimes, not
  source bytes. A safety-sensitive generated-audio cache needs a content digest
  plus exact output-frame targets and algorithm/codec/cache versions.
- `src/services/se_bank_synth/xsb.rs` proves the XACT2 v43 prefix and CRC
  algorithm, but it is a fixed SE-bank writer rather than a stock-song parser.
  Its private CRC should be shared; the writer itself should not be generalized.
- AVS hooks run on arbitrary worker threads and are panic-isolated. Generation
  must use only Rust/file operations, short state snapshots, and idempotent
  immutable outputs; it must not call game-thread-only XACT APIs.

The current replacement API returns only an optional path. That is insufficient
for a transactional rate change: lstat/cache generation cannot prove that the
replacement was opened. `fs_open_body` needs a rate-specific result token or
commit callback so the clock changes only after the generated XSB open succeeds.

### Gameplay clock

- `docs/song_playback_speed.md` identifies one cross-version anchor and the
  authoritative `LEA R14D,[RAX+RBX]` calculation, but documents only a four-byte
  target instruction. A jump needs a verified whole-instruction overwrite
  window of at least five bytes.
- A function detour cannot alter the local `R14D` before all downstream users.
  The fitting mechanism is a one-time inline stub with an identity multiplier,
  modeled after the permanent identity-controlled patch in
  `src/services/cull_window.rs`.
- `src/core/memory.rs` does not currently provide a checked code-patch
  transaction, instruction-cache flush, or write readback. This feature's
  audio/score boundary needs stronger installation checks than existing simple
  patches.
- All consumers should use one pitch-cent-derived effective rate. A fixed-point
  multiplier avoids floating-point drift and hot-path Rust calls.

### Score integrity and lifecycle

- `src/services/score_guard.rs` is the shared taint authority; enforcement is
  already owned by the single save detour in
  `src/services/custom_options_persistence.rs`. Song rate must extend this
  service rather than install another save hook.
- `score_guard::reset_song_taint` clears Quick Fail on gameplay re-entry. A rate
  taint cannot share that reset because Quick Restart preserves the same song
  and bypasses scene 26.
- `src/services/scene_manager.rs` dispatches scene callbacks before constructing
  the next scene. Scene 26 is the best current new-song latch point, but its
  ordering before the first song XSB open still needs live proof.
- Quick Restart produces a gameplay re-entry without selecting a new song. The
  applied rate, generation, and score taint must survive it.
- A non-100% rate must be unavailable unless the clock patch, LayeredFS handler,
  score guard, movie policy, and mode classification are all ready.

### Dependent features

- `src/mods/assist_tick.rs` synthesizes a separate normal-rate XACT voice. Its
  content positions and restart skip must convert from scaled content time to
  wall time using the exact effective song rate. Its fixed 300-second bank also
  covers less chart content at slower rates.
- `src/mods/non_native_os_support.rs` owns the sole
  `DShowPlayer::BuildGraph` detour and always suppresses movies while enabled.
  Conditional song-rate suppression requires shared hook ownership with two
  policy contributors.
- Power User Statistics receives content-domain errors. At non-100%, those are
  not wall milliseconds; the UI and CSV must not silently imply otherwise.
- Real Speed currently describes authored chart speed. Multiplying it by song
  rate requires a larger replacement stub than its existing full code cave.
- There is one physical song stream. Independent simultaneous P1/P2 rates are
  impossible.

## Findings That Change the Preliminary Recommendation

1. LayeredFS has no dynamic replacement registry or applied-open handshake.
2. The existing cache helper is not a source-content fingerprint.
3. A successfully opened XSB can still be rejected later by XACT; the game
   currently ignores `CreateSoundBank` failure. The diagnostic phase must prove
   the generated profile before this becomes a production assumption.
4. The documented clock target needs more disassembly before a safe redirect
   window can be selected.
5. Quick Restart invalidates any design that resets rate taint or the applied
   multiplier at every gameplay entry.
6. The movie hook must become a shared service if non-100% playback suppresses
   DirectShow movies.
7. Assist Tick's 300-second wall-time bank constrains slow-rate support on long
   songs.
8. A polished per-player option requires reliable pre-load side/style/mode
   classification. The code has pieces of this information, but no shared
   pre-stage classifier yet.

## Unknowns Requiring Targeted Research or Live Proof

- A safe whole-instruction overwrite window around the central clock site.
- Whether the XACT pitch field advances every supported streaming XWB at
  `2^(cents/1200)` on native Windows and CrossOver.
- Whether scene 26 always precedes the main XSB open in normal solo and doubles.
- Whether Quick Restart reuses or reloads the XSB.
- A reliable pre-load classifier for active side, doubles, course, local versus,
  matching/BPL, demo, and event/unknown modes.
- P2-started doubles ownership before gameplay actor construction.
- Song-end behavior and any duration timer outside the scaled clock.
- Assist Tick's wall-time formula, output-latency term, and slow-song capacity.
- Which non-score profile aggregates are changed by modified play and survive
  the existing logout sanitizer.

## Proposed PDD Sequence

Use an interleaved sequence:

1. settle product and safety policy in `idea-honing.md`;
2. perform focused Ghidra/source research for the clock window and mode/lifecycle
   facts that affect the design;
3. define a hidden diagnostic phase whose cabinet results gate user-facing UI;
4. confirm readiness, then design the shared service, XSB handler, inline clock
   patch, score/movie integrations, and dependent-feature behavior;
5. plan implementation in proof-first increments so the XACT/clock assumptions
   are tested before broad UI and compatibility work.

The live XACT and cabinet behavior cannot be proven during planning alone. The
design must therefore make the diagnostic pass an explicit implementation gate,
not silently promote static inference into a requirement.
