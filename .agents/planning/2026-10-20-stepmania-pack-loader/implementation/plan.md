# Implementation Plan: StepMania Pack Loader

Status: Approved 2026-10-21 (Step 9 scroll gimmicks inserted 2026-10-23 at the maintainer's request; later steps renumbered)

Design: `design/detailed-design.md` (approved 2026-10-21). Register: `idea-honing.md`.
Cabinet observations go in `progress.md` (this directory) as each step lands. The
maintainer commits; agents never do.

- [x] Step 1: Spike E1/E3/E5/E9 — hand-built injected song via overlay
- [x] Step 2: Diagnostic build — E2 open-thread ids and E6 wire capture
- [x] Step 3: LayeredFS infrastructure — resolver registry, member transforms, in-open arena patch
- [x] Step 4: Pure chart pipeline — `core/sm` parser, SSQ writer + self-check, harness
- [x] Step 5: Pure audio pipeline — decoders, song bank + XSB writers, lead-in
- [x] Step 6: End-to-end injection — index, identity, musicdb, SSQ/XSB/thumbnails, blocking banks, folder/series, primary taint
- [x] Step 7: Score isolation — fail-closed hiding (scope reduced 2026-10-23)
- [x] Step 8: Audio delivery — `wheel_cards` service, scheduler/prefetch, progressive banks, LRU, jackets
- [x] Step 9: Scroll gimmicks — display-only `#SPEEDS` / `#SCROLLS` remap (added 2026-10-23)
- [x] Step 10: `ssq_path` service and per-chart timing split SSQs
- [x] Step 11: Boot hardening — fast-bootup quarantine and radar exclusion
- [x] Step 12: Auto-sync — estimator port, sweep, on-decide measurement
- [x] Step 13: Polish — native titles, splash, dry-run tool, docs, release layout

---

## Step 1: Spike E1/E3/E5/E9 — hand-built injected song via overlay

**Objective.** Prove the visibility contract end-to-end before any code: an entry with
`limited_ary 0`, real `diffLv`, a 7-digit mcode and a 6-character basename appears on the
wheel, previews, plays and reaches results, online and offline.

**Guidance.** Convert one SM song offline with the sibling `ddr-chart-tools` CLI using
`--song-code zab12c`; place `zab12c.{ssq,xsb,xwb}` into a throwaway `data_mods/_spike_sm/`
mod folder at the stock paths; build a jacket arc (`core/arc.rs` via a small host script
or `scripts/unpack_arc.py`'s inverse) and a thumbnail DDS; write a `startup_arc`
overlay `musicdb.xml` = stock + the hand-written `<music>` element (design §5.4) using
series 30 (an operator custom series already configured) so the thumbnail arc
`jacket_thumbnails_<r>_30.arc` is requested. For **E3**, generate N = 100 / 500 / 1,000
clone entries (unique mcodes/basenames, pointing at the same assets) and count loaded
songs in the log with the stock 1 MiB arena. For **E5**, omit the jacket arc and the
thumbnail for one entry. For **E9**, grep the select-music IFS texturelists for
`*level_20*`. Keep fixtures and the generator script under `prototypes/` of this planning
directory; nothing enters `src/`.

**Tests.** None in code. Record: wheel presence, preview audible, play, results, no
`ME1529`, behaviour online (server musicdata loaded) and offline, E3 counts, E5 rendering,
E9 finding.

**Integration.** None.

**Demo.** A converted StepMania song is selectable and playable on the cabinet via overlay
alone, with log lines recorded in `progress.md`. If E1 fails, stop and return to design.

## Step 2: Diagnostic build — E2 open-thread ids and E6 wire capture

**Objective.** Pin which thread performs FileManager opens/reads for SSQ, XSB/XWB, jacket
and thumbnail paths, and capture what reaches the server after an injected play.

**Guidance.** Add one-shot `log_info!` lines in `file_hooks::fs_open_body` (and the read
hook) printing `GetCurrentThreadId()` and whether it equals the render thread id recorded
by `frame_pump`, keyed on paths under `mdb_apx/ssq/`, `sound/win/dance/`, `arc/jacket/`,
`arc/thumbnail/` — behind `layeredfs.verbose`. Play the Step 1 song against the dev
server with a request log enabled; capture `/lastplay`, `eventlog_3.write`,
`pcbevent.put` and the logout save after a full credit.

**Tests.** None (diagnostic). The verbose gate keeps the lines out of normal boots.

**Integration.** Builds on Step 1's fixture.

**Demo.** `progress.md` records, per path family, the opening thread (E2) and the exact
server payload fields that carry the injected mcode/basename (E6). These select the
design's §4.8 wait rules and §4.13 sanitiser scope.

## Step 3: LayeredFS infrastructure — resolver registry, member transforms, in-open arena patch

**Objective.** Give LayeredFS the three capabilities the feature needs, each usable by
other features, with no new detours.

**Guidance.** `mod_paths::register_resolver` consulted after the mod list in
`find_first_modfile`; `arc_handler::register_member_transform` applied after overlays in
`handle_arc`; `CacheHasher` fingerprints the original arc's bytes (size + sampled hash)
and accepts extra fingerprint contributors; `song_limit_expansion::apply_arena_patch`
extracted and idempotent, called from the `startup.arc` transform path after resolving
`gamemdx` by module name, with `early_apply` no-oping when already applied; publish
`transform_ran` and add the one-shot WARN in `lib.rs` when the music DB is populated with
the flag clear. Keep the resolver consultation off the hot path for non-matching paths
(cheap prefix check first).

**Tests.** Host tests for the hasher fingerprint change (a modified original arc
invalidates; unchanged does not) and for transform ordering (overlay then transform) over
in-memory arcs built with `core/arc.rs`. Cabinet: a temporary transform appending the Step
1 `<music>` element (no overlay file) makes the song appear; `early_apply` logs "already
applied"; a boot with an operator `startup_arc` overlay still shows both the overlay's and
the appended entry.

**Integration.** Replaces Step 1's overlay with the mechanism the feature will use.

**Demo.** The Step 1 song appears with the overlay file deleted, injected purely by a
registered member transform, and the arena patch lands before `createList` (log order).

## Step 4: Pure chart pipeline — `core/sm` parser, SSQ writer + self-check, harness

**Objective.** Port and extend the simfile → SSQ path as `crate::`-free pure code with a
host harness.

**Guidance.** Port `ddr-chart-tools` `src/ssc/{mod,notes,msd}.rs`, `src/sm/mod.rs`,
`src/model/`, `src/ssq/writer.rs` and the private `job/` glue into `core/sm/` and
`core/ssq/writer.rs`; add `core/ssq/check.rs`. Apply the design §4.1/§4.2 changes (encoding
detection, per-chart isolation, tolerant tags, stripped annotations, fakes, delays as
early stops, quad split, gimmick marking, per-chart timing grouping, lead-in arithmetic,
`shift_seconds`). Add `encoding_rs`. Create `scripts/validate_stepmania.sh` on the
`validate_*.sh` model with a byte-identity leg against the sibling CLI gated on
`DDR_CHART_TOOLS` (skipped when absent).

**Tests.** The design §7 parser and SSQ fixture list; quad-split colour-class invariance
(`q = round(tick·3/64)`) for every standard quantisation; self-check negatives; byte
identity for a no-fix corpus.

**Integration.** None at runtime yet; `cargo check` clean, harness green.

**Demo.** `./scripts/validate_stepmania.sh` green; a host CLI stub (`tools/sm_pack_check/`,
started here) converts a pack folder to SSQs and prints per-chart verdicts.

## Step 5: Pure audio pipeline — decoders, song bank + XSB writers, lead-in

**Objective.** Produce stock-shaped XSB/XWB banks from pack audio in pure code.

**Guidance.** Add `symphonia` (features `mp3`, `vorbis`, `wav`, `pcm`); `core/audio/`
with `probe` and the block iterator (mono/downmix/resample/lead-in/pad-truncate, no MP3
gapless trim, preview window + fades); `core/xact/song_bank.rs` (`SongBankPlan`, header
and seek-table emission, main-first entry order, block appends) and `core/xact/xsb.rs`
(port of `src/xsb/mod.rs::write`, CRC-16). Confirm both decoders and `rustfft`-free code
build under `./build.sh` and `./build_win7.sh`.

**Tests.** XSB golden bytes (4/5/6-char codes); XWB plan vs tool output per entry and
strict `parse_song_bank` acceptance; ADPCM round trip against the tool's decoder;
symphonia vs lewton Vorbis ±1 LSB; MP3 probe frame count vs decode; lead-in / preview
clamp arithmetic. Extend the harness and `tools/sm_pack_check/` to emit banks.

**Integration.** Cabinet check: drop the tool's XSB/XWB for the Step 1 song into the spike
mod folder in place of the CLI's output; preview and gameplay audio play identically.

**Demo.** `tools/sm_pack_check/` converts a song folder into SSQ + XSB + XWB that the
cabinet plays via the Step 3 transform.

## Step 6: End-to-end injection — index, identity, musicdb, SSQ/XSB/thumbnails, blocking banks, folder/series, primary taint

**Objective.** Songs dropped into `./stepmania/Songs/` boot, appear in a STEPMANIA folder
and series, preview, play and never save — with the simplest audio delivery.

**Guidance.** `services/stepmania/{mod,index,identity,musicdb,cache,providers,taint}.rs`
and `mods/stepmania_packs/` per design §4.7/§4.15; `early_init` at `lib.rs` step 0' with the
`startup.arc` transform and resolver; `index.bin` warm path; materialise SSQ/XSB/thumbnail
arc at scan; `core/art` thumbnail path (jackets use the shipped placeholder arc this
step). Banks: on open, build the requested variant synchronously on the opening thread if
it is not the render thread (Step 2 result), else serve the silent variant — no scheduler
yet. `folder_expansion::register_runtime_folder` / `series_expansion::register_runtime_series`
with shipped art under `data_mods/stepmania_packs/`; lowest free bit/series. Primary taint:
`score_guard::set_injected_song_taint` + `mark_session_tainted` at the scene-26 commit
(the existing whole-side logout wipe applies). Config section `stepmania_packs` in
`config.rs` (operator-only). Titles use `#TITLETRANSLIT` when present else `#TITLE`
(glyph check comes in Step 12). Release layout: `scripts/build_release_archive.sh` ships
an empty `stepmania/Songs/` with a README; the updater's user-owned merge list preserves
`stepmania/`.

**Tests.** Harness: identity determinism/probing/collision drop; musicdb fragment schema
and 127-byte truncation; `index.bin` fingerprint invalidation; LRU accounting shell.
Cabinet: a real pack (dozens of songs) boots with 0 `ME1529`; STEPMANIA folder and VERSION
entry present; thumbnails drawn; preview plays after a stall ≤ ~1 s; a song plays to
results; the dev-server log shows no `/result` for it; logout save sanitised; stock songs
unaffected; a second boot is warm (log timing).

**Integration.** Uses Steps 3–5; replaces the Step 1 spike (delete `_spike_sm`).

**Demo.** Copy a StepMania pack into `./stepmania/Songs/`, boot, play a song from the
STEPMANIA folder; nothing reaches the server.

## Step 7: Score isolation — fail-closed hiding (scope reduced 2026-10-23)

**Objective.** Complete the local-only guarantee per design §4.13 using Step 2's E6 facts.

**Guidance.** (Scope reduced 2026-10-23: the existing whole-side logout wipe is the correct
model — no targeted sanitiser; `/lastplay` and `mod_judge_offsets` round-trip harmlessly —
no wire filters.) Fail-closed hiding at enable (`find_music_by_mcode` + the `+0x1B0`
anchor exposed by `fast_bootup`) and `HIDDEN_FOR_SAFETY` save suppression; `reset_session`
clears the taint. Also carries the `exclude_from_all_music = true` default, the 60-minute
song cap and the highest-free folder bit.

**Tests.** `score_guard_tests.rs`: injected taint in `is_stage_suppressed`, cleared on
reset. Pure tests for the targeted record wipe over synthetic record layouts. Cabinet:
mixed credit (stock song, injected song, stock song) → the two stock saves forward at their
results screens, the injected one does not, the logout re-send has all records wiped; a boot
with sanitisation forced unavailable hides every injected song.

**Integration.** Extends Step 6's taint.

**Demo.** Dev-server request log after a mixed credit shows exactly the stock records and
no injected identity anywhere.

## Step 8: Audio delivery — `wheel_cards` service, scheduler/prefetch, progressive banks, LRU, jackets

**Objective.** Previews ready within the settle, gameplay banks with no stall, bounded disk.

**Guidance.** Promote the wheel-card refresh detour from `s_marvelous/lamp_badge.rs` to
`services/wheel_cards.rs` (`on_card_bound`); `scheduler.rs` (pool ≥ 3, priorities per
design §4.7); preview + jacket jobs on card bound, highlighted song re-prioritised;
speculative full bank after a highlight dwell, unconditional at commit; progressive
full-bank writer + `file_hooks` in-flight read watermark (design §4.8(3), §5.6); bounded
open wait only off the render thread; silent preview fallback; byte-budgeted LRU over
`xwb/` and `jacket/` (`audio_cache_mb`); `core/art` jacket composition replacing the
placeholder.

**Tests.** Pure: scheduler priority/dedupe/cancel; LRU eviction order and budget;
progressive plan offsets; watermark wait logic with a fake clock. Harness leg for jacket
composition on fixture images (dimensions, opaque, placeholder fallback). Cabinet: scroll
a row of injected songs — previews audible within the settle; vertical scroll — new row's
previews ready by the time the highlight lands; decide immediately after highlight — song
plays with no dropouts (log watermark waits); LRU eviction observed at a small
`audio_cache_mb`; jackets drawn on the header card.

**Integration.** Replaces Step 6's synchronous bank build; `s_marvelous` becomes a
`wheel_cards` subscriber with identical behaviour.

**Demo.** Browsing injected songs feels like stock: instant previews, jackets, no stalls.

## Step 9: Scroll gimmicks — display-only `#SPEEDS` / `#SCROLLS` remap

**Objective.** Reproduce StepMania's visual scroll gimmicks (`#SPEEDS` multiplier ramps,
`#SCROLLS` segment factors) in DDR's lane without touching timing or judgement — e.g. Tech
Spectrum Super's `260-MISC.[MASTER] Matt Silver` squeeze-and-expand at beat 160.

**Guidance.** (Added 2026-10-23 at the maintainer's request, ahead of the remaining steps.)
Semantics to match (StepMania `TimingData`): `#SCROLLS` `beat=factor` defines a
piecewise-constant scroll factor whose integral maps beat → *display beat*; `#SPEEDS`
`beat=ratio=delay=unit` ramps a global distance multiplier from the previous ratio to
`ratio` over `delay` beats (`unit` 0) or seconds (`unit` 1); the drawn offset of a note is
`(display_beat(note) − display_beat(now)) × speed(now) × player speed-mod`, freeze bodies
spanning both ends. Pieces: (1) RE spike on the renderer's per-note scroll function
(`get_offset_y`, signature-resolved; no detour existed before this step): which
inputs it reads (note `beat_count` / `music_count`, renderer `+0xA0` speed, `+0xA8/+0xAC`
beats) and where the remap must be applied so NORMAL/BOOST/BRAKE/WAVE and the hallway
transforms compose; (2) `core/sm` keeps `#SPEEDS`/`#SCROLLS` in the model (pure
`core/sm/scroll.rs`: segment tables, `display_beat(beat)`, `speed_at(beat, time)`, with
StepMania's edge rules — negative factors allowed, zero-length ramps, ties); (3) the index
stores the tables per basename (`index.bin` FORMAT_VERSION bump) and the service publishes
the committed song's tables at gameplay load; (4) a render-time remap under the existing
`get_offset_y` owner (promote to a shared service if a second consumer appears) applied per
note per frame, panic-free and allocation-free; (5) `cull_window` contribution widening the
collect window to the remapped extent (speed 0 collapses every future note onto the
receptors — SM draws them all); (6) a `stepmania_packs.scroll_gimmicks` config bool
(default true) so operators can turn the effect off. Timing/judgement untouched by
construction (the remap only feeds the draw offset).

**Tests.** Pure: `scroll.rs` against StepMania-derived vectors (constant 1.0 identity; a
0-factor segment freezing display beat; the Matt Silver tables: display beat at 116.0–116.5
flat, speed 0 → 1 over 6 beats from 160.021; `unit` 1 ramps in seconds; negative factor);
harness leg in `validate_stepmania.sh`. Cabinet: Matt Silver squeezes at combo ~582 and
expands back; a `#SCROLLS 0` region shows the notes frozen then released; judgement CSV
deltas identical to a run with `scroll_gimmicks=false`; stock songs and the hallway
perspective unchanged; no note culled early at speed < 1.

**Integration.** Extends `core/sm` (Step 4), the index (Step 6) and the renderer seams
owned by `player-perspective`/`cull_window`.

**Demo.** Matt Silver plays with its ITG scroll gimmicks intact, scores identical.

## Step 10: `ssq_path` service and per-chart timing split SSQs

**Objective.** Honour SSC per-chart timing through DDR's own split-SSQ mechanism under the
one-detour rule.

**Guidance.** `services/ssq_path.rs` owns the `build_ssq_path` detour with a priority
resolver chain; `split-ssq-auto-discovery` becomes a consumer; the StepMania provider
answers injected basenames from the index (`<b>.ssq` or `<b>_N.ssq`); the index writes
split files per design §4.2 and declares/hides slots accordingly. Also lands the E7 test
charts (quad split, delay nudge) and the `gimmick_policy` config read.

**Tests.** Pure chain ordering tests; writer tests for level grouping and double-hide
rule; harness fixtures with per-chart timing. Cabinet: a split-timing SSC plays each level
in sync; `split-ssq-auto-discovery` still resolves its disk splits; E7 charts judge as
expected (two judgements per quad, delay notes judged after the delay).

**Integration.** Extends Step 6's SSQ materialisation; refactors an existing mod.

**Demo.** An SSC with different BPM maps per difficulty plays correctly at every level.

## Step 11: Boot hardening — fast-bootup quarantine and radar exclusion

**Objective.** Make a bad chart impossible to turn into a boot blocker, and keep stock
radars untouched.

**Guidance.** In `fast_bootup`'s `CheckStepDataActor::onUpdate` detour, injected items
that analyse to zero steps set `+0x1B0`, WARN, and skip the `ME1529` reporter; exclude
injected items from the radar-axis maxima fold (E8). Record the quarantine capability in
`index.bin` keyed by the gamemdx PE stamp for diagnostics. Verify the arena patch
necessity against Step 1's E3 numbers and cap the injected count with a WARN if the
patch did not land.

**Tests.** Pure tests for the predicate wiring. Cabinet: hand-corrupt one materialised SSQ
(remove its step rows) → boot succeeds, the song is hidden, one WARN; compare a stock
song's radar with and without an extreme ITG chart injected.

**Integration.** Extends Step 6; `fast_bootup` API addition.

**Demo.** A deliberately broken injected chart hides itself instead of raising `ME1529`.

## Step 12: Auto-sync — estimator port, sweep, on-decide measurement

**Objective.** Charts land on DDR World's sync convention automatically, gated by
`auto_sync`.

**Guidance.** Port `ddr-chart-tools/src/sync/` to `core/sync/` (add `rustfft`); chart
events from the parsed model; `sync_sweep.rs` below-normal-priority thread paused during
gameplay scenes; on-decide measurement sharing the full-bank decode; `sync.bin`
persistence per source fingerprint; atomic SSQ rewrite via `shift_seconds` (per split
file); lead-in headroom already reserved in Step 4.

**Tests.** Harness: estimator vs CLI `--auto-sync report` within ±1 ms on fixtures; every
refusal path; persistence/idempotence (measured once); `shift_seconds` round trip.
Cabinet: an ITG-synced pack (+9 ms) measures ≈ +9 ms deltas; a null-synced pack ≈ 0;
corrected songs feel in sync at ×1.0 against a stock song; the sweep's CPU stays
below-normal and pauses in gameplay (log).

**Integration.** Extends Steps 6/8 (decode sharing) and Step 4 (`shift_seconds`).

**Demo.** With `auto_sync: true` a mixed pack plays in sync without manual offsets; with
`false` nothing is rewritten.

## Step 13: Polish — native titles, splash, dry-run tool, docs, release layout

**Objective.** Operator-grade finish.

**Guidance.** Extract the song-title KBF font's codepoint set offline
(`scripts/gen_title_glyph_table.py` → shipped bitmap) and switch titles to native-when-
renderable; splash line `N StepMania songs (P packs), M rejected`; finish
`tools/sm_pack_check/` (full artefact set, musicdb fragment, reasons); README section;
module `//!` docs; `docs/stepmania_runtime_packs_feasibility.md` gets a "what shipped"
header pointing at the modules; `.agents/learnings/learnings.md` entries for the traps hit;
regenerate `.agents/summary/` (codebase-summary workflow); `cargo fmt`, `./build.sh`,
`./build_win7.sh`, `./scripts/validate_signatures.sh` (no new signatures expected — confirm
the sweep is unchanged).

**Tests.** Harness leg for the glyph-table lookup and title fallback; all previous legs
green; readiness gate per `AGENTS.md`.

**Integration.** Final.

**Demo.** A fresh install with a CJK-titled pack shows native titles on the wheel, the
splash reports the library, and an operator can vet a pack with `sm_pack_check` before
copying it to the cabinet.
