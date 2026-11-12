# Idea Honing: StepMania Pack Loader

Decision register. `Status`: Proposed / Accepted / Overridden / Assumed / Open.
`§n` references are into `docs/stepmania_runtime_packs_feasibility.md`.

| ID | Decision | Why it matters | Recommendation | Status |
|---|---|---|---|---|
| D1 | v1 scope | Bounds every other decision | `.ssc` + `.sm`; `dance-single` + `dance-double`; OGG/MP3/WAV audio; charts, preview, jacket, thumbnail, STEPMANIA folder + series, local-only play. Out: movies/`#BGCHANGES`, courses, keysounds, lifts, non-dance styles, link/matching/BPL play, `.dwi` | Accepted |
| D2 | Music DB injection | Architecture; boot race | `startup.arc` member transform appending `<music>` entries to the post-overlay `musicdb.xml` (text insert before `</mdb>`); arena patch applied from inside that same open | Accepted |
| D3 | Asset delivery | Hook surface; latency; persistence | **Materialise** every artefact under `data_mods/_cache/stepmania/` and redirect opens by path (no new hooks). Virtual fds deferred to v2 (differs from the feasibility note's recommendation) | Accepted |
| D4 | Pack location | User-facing layout | Fixed root `./stepmania/Songs/<Pack>/<Song>/` (sibling of `data_mods/`, created if missing when enabled). No configurable roots | Overridden |
| D5 | Wheel grouping | User-visible | One STEPMANIA custom folder (via `folder-expansion`) + one STEPMANIA custom series (via `series-expansion`), shipped static art, registered programmatically. Hidden from ALL MUSIC by default (maintainer override 2026-10-23); operator-only `stepmania_packs` section with `exclude_from_all_music` (default **true**) and `audio_cache_mb` (default 1024), `auto_sync` (default true). Per-pack folders deferred | Accepted |
| D6 | Score isolation | Integrity | Global injected-song taint latched at the gameplay bank load; `mark_session_tainted` for entered non-bot sides; the EXISTING whole-side logout wipe (stage saves already happened at results — maintainer 2026-10-23, targeted sanitiser dropped); filter injected basenames from `mod_judge_offsets` wire copy; `/lastplay` stripped/rewritten once E6 shows its shape. Fail-closed: hide all injected songs if full sanitisation is unavailable at boot | Accepted |
| D7 | Identity scheme | Keys every asset, cache and offset | basename `z` + 5 base-36 chars of a stable hash of case-folded `<pack>/<song>`; mcode in 9,000,000–9,999,999 by hash + probe; collisions with the post-overlay base DB → drop + WARN | Accepted |
| D8 | Chart fidelity policy | Gameplay correctness; boot safety | Quads split 1 tick (never a shock byte); rolls → holds; lifts/fakes dropped; `#FAKES` ranges dropped; delays → stop 1 tick early; warps/negative BPM/negative stops → `hide_chart` (default); per-chart SSC timing → split SSQ per level when single/double agree, else hide double; Edit dropped; duplicate difficulty → first; double Beginner dropped; partial-row mines → type-20 chunk; full-row mines → shock. Every SSQ self-checked before its slot is declared | Accepted |
| D9 | Meter → `diffLv` | User-visible levels | Per-pack `meter_map`, default `clamp` to 1..19 (20 pending E9) | Accepted |
| D10 | Audio decoding | Pack coverage; build toolchain | `symphonia` (pure Rust; features `mp3`, `vorbis`, `wav`, `pcm`, optionally `flac`) rather than `lewton`. Mono → stereo, >2 ch downmix, 44.1/48 kHz pass-through, others resampled to 44.1 kHz with `core/xact/resample.rs`. No MP3 encoder-delay trim (match StepMania) | Accepted |
| D11 | Audio delivery timing | Preview/gameplay latency | Two complete-bank variants: preview variant built on highlight (`selectmusic_highlight`), full bank speculative on highlight + unconditional on decide; LRU in memory + disk (`audio_cache_mb`). Not-ready: preview → silent preview; gameplay → bounded wait in the open on a FileManager worker thread (E2), else hold the stage start | Accepted |
| D12 | Converter sourcing | Maintenance | Port the pure files from `ddr-chart-tools` into `core/sm/`, `core/ssq/writer.rs`, `core/xact/{xwb_builder,xsb}.rs` with the fixes it lacks; byte-identity harness leg vs the sibling CLI; upstream fixes by hand | Accepted |
| D13 | Index + caches | Boot time | Scan on worker threads from `lib.rs` step 0; `startup.arc` transform waits bounded (30 s default); warm index `_cache/stepmania/index.bin` keyed by `(path, size, mtime)`; SSQs + XSBs + thumbnails produced at scan, jackets lazily on highlight | Accepted |
| D14 | Art sourcing | User-visible | Jacket priority `#JACKET` > `#BANNER` (square canvas over blurred copy) > `#BACKGROUND` (centre crop) > filename heuristics > pack banner > shipped placeholder. 512² PNG jacket arc, 192² DXT1 thumbnails in the STEPMANIA series' arc (both `ja`/`ua` names) | Accepted |
| D15 | Title / artist text | User-visible | Native `#TITLE` when every codepoint is in the song-title KBF font (glyph set extracted offline into a shipped table), else `#TITLETRANSLIT`; `#SUBTITLE` appended as ` (sub)` when it fits; generated `title_yomi`; ≤ 127 bytes | Accepted |
| D16 | Mod identity | Registry semantics | Id `stepmania-packs`, **default ON** (no-op with an empty root: no folder/series/transform), boot-only `is_active` semantics, splash line "N StepMania songs, M rejected" | Overridden |
| D17 | Required refactors of neighbours | One-detour rule; correctness | Promote `build_ssq_path` to `services/ssq_path` resolver chain; `fast-bootup`: content-hash identity, `ME1529` quarantine for injected mcodes, radar-accumulator exclusion; one shared asset resolver for `chart_length`, dancers' tempo source, judgement-offset bootstrap, split discovery | Accepted |
| D18 | Spikes first | De-risking | E1 (hand-built song via overlay, online + offline), E2 (open thread), E3 (arena headroom), E6 (wire capture) are the first plan steps; E4 only if D3 flips to virtual fds | Accepted |
| D20 | Auto-sync | Sync of community packs to DDR World's convention | Port `ddr-chart-tools/src/sync/` (onset envelope + offset estimate, ±60 ms cap, refusal gates). Measured once per song on the decode the XWB build already does — post-boot background sweep at below-normal priority (paused during gameplay) plus on-decide for unmeasured songs — persisted in `_cache/stepmania/sync.bin`; applied by shifting the SSQ's seconds anchors. Refused ⇒ chart as authored. Replaces per-pack `sync_bias_ms`. Gated by `stepmania_packs.auto_sync` (bool, default true) | Accepted |
| D21 | Wheel neighbour prefetch | Preview latency | Promote `s_marvelous`'s wheel-card-refresh detour (reads `card+0x148`) to `services/wheel_cards` with `on_card_bound(mcode)`; prefetch preview bank + jacket for every injected song entering the visible window, highlight first. No new RE; ±K display-list lookahead is a fallback spike only if this measures short | Accepted |
| D22 | Scroll gimmicks | Visual fidelity for ITG tech packs | Display-only remap of `#SPEEDS`/`#SCROLLS` through the renderer's per-note scroll function (owned seam), cull window widened; timing/judgement untouched; `stepmania_packs.scroll_gimmicks` (default true). Plan Step 9, ahead of the remaining steps (maintainer 2026-10-23) | Accepted |
| D19 | Offline vetting | Operator UX; CI-less repo | `scripts/validate_stepmania.sh` harness over the pure files + a host-side dry-run tool that converts a pack folder to the would-be artefacts and runs the self-checks | Accepted |

## Detail

### D1 — v1 scope
Q: What is in the first shippable version?
A: As above. Rationale: §2 scope; movies need a DirectShow graph per song (out of
reach of XWB-style injection); courses need `coursedb.xml`; link/matching needs the
song on a second cabinet. MP3 is included (common in older packs) because `symphonia`
makes it cheap; FLAC optional. Alternative rejected: OGG-only v1 (leaves many packs
half-loaded, which is worse UX than slightly more decoder code).

### D2 — Music DB injection
Q: How do injected entries reach the game's music DB?
A: The `startup.arc` member transform (§7.1 option A). Rejected: `createList` detour
(loses the `onBoot` race, needs gamemdx signatures before they resolve) and
hand-built `music::Info` objects (three heaps, re-implements sort/active-list/analysis).
Includes: cache key for the repacked arc gains the index fingerprint + original arc
bytes; the XML arena patch moves into the open (§7.5), shared with
`song-limit-expansion`, self-diagnosing like `shader_synthesis::status()`.

### D2 — clarification (2026-10-21)
On disk only inside the machine-owned `data_mods/_cache/arc/startup.arc` repack that
LayeredFS already produces; the user's `musicdb.xml` / `startup.arc` are never
modified. Works with a replaced stock arc (the repack reads it as the original), with a
LayeredFS `startup_arc` overlay (overlays run first, then the append), and is
irrelevant to `musicdb.merged.xml` fragments (they never reach the game's DB). Generated
ids are collision-checked against the post-overlay musicdb.

### D3 — Asset delivery
Q: How are SSQ/XSB/XWB/jacket/thumbnail bytes handed to the game?
Disk budget (2026-10-21 discussion): source audio ≈ 1 MB/min, XWB ≈ 2.9 MB/min, so a
mirror would be ~3× pack audio (2,000 × 2 min ≈ 11.6 GB) — rejected. Instead: SSQ, XSB
and thumbnails for every song are materialised (needed at boot; ≈ 100–150 MB per 1,000
songs); jackets, preview banks and full banks live in a bounded LRU (`audio_cache_mb`,
default 1024), evicted songs re-encode on the next play inside the decide → load
window. Virtual fds remain the v2 route to flatten disk further; the provider interface
is shaped so only the XWB delivery path changes.
A: Materialise under `data_mods/_cache/stepmania/{ssq,xsb,xwb,jacket,thumbnail}/`
and let LayeredFS redirect the stock paths there (a dedicated resolver in
`find_mod_replacement`, not a mod folder walk). Rationale: zero new AVS hooks (no fd
table, no fd-range risk, no `CreateFileA` placeholder); artefacts persist across
boots so replays and reboots cost nothing; the DLL's own host-path readers (D17)
get a real path; `_cache` is already machine-owned and safe to delete. Cost: disk
(bounded by `audio_cache_mb` LRU; ~3 MB/min of audio) and a write before the open
(negligible next to the encode). Virtual fds (§11, the feasibility note's
recommendation) stay the v2 path if latency measurements or disk constraints demand
it; nothing in v1 precludes it because providers produce bytes first and persist
second.

### D4 — Pack location + config
Q: Where do packs live and how is the mod configured?
A: `./stepmania/Songs/` next to `data_mods/` (not under it — every `data_mods/*` is a
LayeredFS mod root). Operator-only section `stepmania_packs`: `song_roots[]`,
`packs{<folder>: {enabled, sync_bias_ms, meter_map, series?}}`, `gimmick_policy`,
`edit_policy`, `max_song_minutes`, `max_audio_mb`, `audio_cache_mb`,
`prefer_translit`, `index_timeout_s`, `exclude_from_all_music`. Never written by the
DLL. Non-ASCII folder names are fine (`std::fs` wide APIs; the game only sees codes).

### D5 — Wheel grouping
Q: How do players find the songs?
A: One STEPMANIA folder (`folder-expansion` custom folder, a free `<property>` bit,
static art in `data_mods/stepmania_packs/`) and one STEPMANIA series
(`series-expansion`, value from the free 22–255 range) applied to every injected
song. The series buys flare-skill exclusion and makes the game itself request the
thumbnail arc. Both mods gain a programmatic registration API callable before their
`enable`. Default `exclude_from_all_music = false` so songs also sort into level/BPM
views; operators who run public cabinets can flip it. Per-pack folders need a runtime
text rasteriser (none exists) — v2.

### D6 — Score isolation
Q: How is "never reaches the server" guaranteed?
A: §12. Taint is global (both sides play the same mcode), latched at the scene-26
commit point `song_rate` uses, cleared in `reset_session`. Stage save: existing
trampoline. Logout: targeted wipe of records whose mcode is injected (fallback:
whole-side wipe). Wire filters for `mod_judge_offsets`. `/lastplay`,
`eventlog`/`pcbevent`: capture in E6, then strip/rewrite. If
`is_full_sanitization_available()` is false once services are up, set the corruption
flag on every injected entry (hidden at the next active-list rebuild) and suppress
every save of any session that commits an injected mcode anyway. Link/matching/BPL:
out of v1 scope; a `FUN_1801b56b0` visibility service is the v2 answer.

### D4 / D5 / D16 — overrides (2026-10-21)
No configurable song roots: the root is fixed at `./stepmania/Songs/`. The operator
section `stepmania_packs` is kept minimal: `exclude_from_all_music` (default false),
`audio_cache_mb` (default 1024), `auto_sync` (default true, 2026-10-21); never written by the DLL. The mod is default ON; with an
empty or missing root it registers no folder, no series and no transform.

### D7 — Identity scheme
Assumed per §6.4. Stability under `_cache` deletion matters for
`per-song-judgement-offsets` and the fast-bootup cache; renaming a song folder renumbers
it, which is acceptable.

### D8 — Chart fidelity policy
Quad split (2026-10-21): a one-side 4-panel row ORs to `0x0F`/`0xF0`, which the step
chunk reserves for shock arrows. Emit two rows one measure tick apart (1/1024 beat ≈
0.4 ms at 150 BPM): L+R (`0x09`) then D+U (`0x06`); same for freeze heads and for
each full side on doubles. Visually one quad, both rows in one judgement window; costs
one extra judgement vs StepMania. Engine tolerance for 1-tick-apart rows is unverified
→ E7 test chart; fallback policy `reduce_to_jump`.
Colour (2026-10-21): the game's quantisation→palette-row selector (`FUN_180028130`,
decoded in `docs/chart_strip_hud_research.md` §4) rounds the beat tick to a 1/48-beat
grid (`q = round(tick·3/64)`) before classifying 4th/8th/16th/other, so a +1 tick row
keeps the colour of its parent row for every standard SM quantisation (4/8/12/16/24/32/
48/64/192). Same in the beat-cycling palette mode (boundaries at ticks 36/292/548/804,
off every standard grid). Vertical offset is < 1 px. Presentation is therefore
identical to a quad except for the extra judgement.
Q: What happens to SM constructs SSQ cannot express?
A: Table in §8.3 plus the §8.2 converter fixes. `gimmick_policy` default `hide_chart`;
`approximate` (collapse warped beats into a ≥ 1 ms steep segment) deferred to E7.
Mines-only or holds-only charts are never declared (`ME1529` risk). Self-check (§8.6):
monotonic tempo, FINISH/END bracketed, END ≥ last note, ≥ 1 tap/jump/shock per slot,
no unintended shock byte.

### D9 — Meter mapping
`clamp` default; `identity` and custom tables per pack. Level 20 only if E9 finds art.

### D10 — Audio decoding
Vs ddr-chart-tools (2026-10-21): the tool uses `lewton` (Vorbis only, unmaintained).
`symphonia` is a strict superset (Vorbis/MP3/WAV/FLAC), packet-based (natural streaming
decode → encode), and leaves MP3 encoder delay untrimmed by default
(`enable_gapless = false`) — the StepMania/libmad behaviour. ADPCM encoder and bank
writers stay byte-identical with the tool; harness compares symphonia vs lewton Vorbis
output within ±1 LSB. Candidate for upstreaming to the tool.
`symphonia` is pure Rust so it builds under cargo-xwin and the Win7 `-Z build-std`
leg; `lewton` covers only Vorbis. Rejected: C decoders (`minimp3`, libvorbis — need a
`cc` toolchain under xwin). Match StepMania's libmad behaviour: no gapless trim.
Vorbis first-granule behaviour vs libvorbisfile is E10.

### D11 — Audio delivery timing
Variant chosen by request context (scene 25 ⇒ preview). Preview window from
`#SAMPLESTART`/`#SAMPLELENGTH`, clamped 5–20 s, ~0.25 s fade-in / ~1.5 s fade-out,
never empty. Lead-in normalisation: prepend N ms silence so `tempo_data[0] ≥ 0`, first
note ≥ tick 4096 and ≥ ~2 s of audio before it; `tempo_data[0] = −#OFFSET·1000 + N +
sync_bias_ms`. Full bank ~1–2 s single-threaded per 2 min song; parallel ADPCM blocks.

### D12 — Converter sourcing
Port, as `se_bank_synth` was ported, keeping files `crate::`-free so
`scripts/validate_stepmania.sh` can mount them. Rejected: git/path dependency (pulls
`clap`/`env_logger`/`vorbis_rs`/`rustfft`; the private `job/` glue would still need
lifting; every §8.2 fix is needed anyway).

### D13–D19
Engineering choices recorded as assumptions; see the table. D17's `fast-bootup`
quarantine means the injection decision (taken inside the `startup.arc` open, before
signatures resolve) is conditioned on the config plus a per-build "quarantine capability
resolved" flag cached in the index.

### D20 — Auto-sync (2026-10-21)
Q: Can `ddr-chart-tools`' `--auto-sync` run in the DLL's real-time-ish setting?
A: Yes, with scheduling. The estimator (`src/sync/{onset,estimate,events,timemap}.rs`,
~1,400 lines, pure, `rustfft` only — pure Rust, builds under xwin) needs the **whole
decoded song**: a 10 ms / 512-point FFT every 32 samples (≈ 165k FFTs for 2 min ≈
0.5–1 s native, more under CrossOver) and then a cheap ±70 ms score curve against the
chart's tap events. The decode is shared with the XWB build, so the marginal cost on a
decide is ~+0.5–1 s inside the decide → `onSetup` window. Cold-scanning a 2,000-song
library this way (~1 h of CPU) cannot block boot, hence:
- a post-boot background sweep on one below-normal-priority thread, paused during
  gameplay scenes, persisting `{identity, source fingerprint} → outcome` in
  `_cache/stepmania/sync.bin`;
- on decide of a not-yet-measured song, measure on the bank build's decode; if the
  result is not in by the SSQ open, serve the as-authored SSQ this once and persist for
  next time;
- a song is measured once per source fingerprint, so a player's later manual
  `per-song-judgement-offsets` tuning is never double-corrected;
- the correction shifts every seconds anchor (`shift_timeline`); lead-in `N` is chosen
  with ≥ 60 ms headroom so applying it never re-encodes audio; the fast-bootup
  identity hashes sync-invariant SSQ content so a late correction does not re-analyse;
- `TARGET_OFFSET_MS` is calibrated against DDR World's own stock sync, so the result
  lands SM packs on DDR World's convention regardless of ITG (+9 ms) or null-sync
  authoring — this is why it replaces a per-pack `sync_bias_ms`;
- refusals (`too_few_events`, `ambiguous_peak`, `halves_disagree`, `beyond_cap`, …) keep
  the chart as authored and log one line; harness compares against the CLI with ±1 ms
  tolerance (rustfft's runtime SIMD paths are not bit-exact across hosts).

### D21 — Wheel neighbour prefetch (2026-10-21)
Q: Can previews be ready before the wheel settles on a song?
A: Yes, without RE. `s_marvelous/lamp_badge.rs` already detours the wheel card refresh
and reads each card's own song holder (`card+0x148`, identical on 20250805/20260224/
20260825). Each refresh is a "song visible on the wheel" event. Under the one-detour
rule the detour moves to a service; `s_marvelous` subscribes for lamps, the StepMania
scheduler subscribes to queue preview bank + jacket builds (highlighted song first,
then visible neighbours, LRU-deduped). The wheel is a 2-D grid, three songs per row (maintainer-confirmed
2026-10-21): sideways moves stay inside already-bound cards, a vertical scroll binds one
new row of three. The scheduler uses one worker per preview job so a new row's three
previews run concurrently, and re-prioritises the highlighted song to the queue head. The model's `+0x1C8` list is the pre-filter base list, not display order, so a
±K lookahead would need new RE — deferred unless measurements demand it.

## Readiness Confirmed 2026-10-21

All decisions D1–D21 Accepted or Overridden (none Open). Maintainer confirmed readiness for design.

### D5 / D6 / caps — maintainer overrides (2026-10-23, after the Step 6 demo)
- `exclude_from_all_music` defaults to **true**: the STEPMANIA folder is the only entry point.
  The series value stays registered (thumbnail arc request, flare exclusion, series-name
  table safety); its VERSION cell is kept as zero-cost (useful when the flag is false).
- Score isolation: the whole-side logout wipe already in `custom_options_persistence` is
  the correct model (per-stage saves happen at results; the logout save only restates
  them). The design's targeted sanitiser is dropped. `/lastplay` and `mod_judge_offsets`
  are NOT scrubbed either (maintainer 2026-10-23): they round-trip through the profile
  harmlessly; only scores matter. Step 7 scope = fail-closed hiding + tests.
- Song length cap raised from 15 to **60 minutes** (audio byte cap 256 MiB).
