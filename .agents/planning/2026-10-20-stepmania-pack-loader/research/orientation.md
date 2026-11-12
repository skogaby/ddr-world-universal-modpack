# Orientation: StepMania Pack Loader

Step 2 blind-spot pass. What exists, what changes the idea, what is unknown.

## 1. The territory is already mapped

`docs/stepmania_runtime_packs_feasibility.md` (2026-09-25, ~1,460 lines) is a full
RE + feasibility record for exactly this feature. Verdict: **feasible**, no static
blocker. It inventories every game-side contract, the modpack pieces that exist, and the
open experiments. This PDD pass does not redo that work — it turns it into decisions,
a design and a plan. Section references below (`§n`) are into that note.

No spike from the note's §16 experiment list (E1–E11) has been run since it was written
(no scratchpad / learnings entries; the note's last commit is `5ec3ae5f`).

## 2. Core facts the design rests on

| Fact | Consequence | Source |
|---|---|---|
| `musicdb.xml` is parsed out of the resident `data/arc/startup.arc`, the game's first boot read, by `music::createList`; LayeredFS's `avs_fs_open` detour is live before `gamemdx` loads | Inject `<music>` entries by transforming the `startup.arc` member during the LayeredFS repack. No hook on the music DB itself. Proven race-free by the `zzzt` clone test (1470 songs, 0 `ME1529`) | §4.1, §7; learnings 2026-09-03 |
| Per-chart level/state getters fall back to XML `diffLv`/`limited_ary` when the server has no row for the mcode | Injected charts need `limited_ary 0` + `diffLv 1..254`; absent charts `-1`/`0`. Playable online and offline, the server can never touch them | §4.4, §4.5 |
| Every per-song asset is fetched by path through FileManager → AVS → LayeredFS: `mdb_apx/ssq/<b>[_N].ssq`, `sound/win/dance/<b>.xsb/.xwb`, `arc/jacket/<b>_jk.arc`, thumbnail `<b>_tn` in `arc/thumbnail/jacket_thumbnails_<ja\|ua>_<series>.arc` | One XML entry + five byte streams per song | §3, §4.6 |
| `basename` ≤ **6** ASCII chars (constructor truncates), `title`/`artist` ≤ 127 UTF-8 bytes, mcode unique u32 < 2^31, a handful of reserved mcodes/basenames | Generated identities: `z` + 5 base-36 hash chars; mcodes in a 7-digit reserved window; both pure functions of `<pack>/<song>` folder path | §4.2, §6.4 |
| The XWB is read **whole** into RAM both for the 0.4 s wheel preview and at stage load | Audio latency is the hard problem: preview bank on highlight (~50–200 ms), full bank speculative on highlight / on decide (~1–2 s per 2 min song, single-threaded, more under CrossOver) | §9.4–§9.6 |
| A declared chart whose SSQ analysis yields zero steps raises boot-blocking `ME1529` | Self-check every generated SSQ before declaring its slot; quarantine path via `fast-bootup`'s owned `CheckStepDataActor::onUpdate` detour | §8.5, §8.6 |
| The musicdb property-tree arena is 1 MiB; `song-limit-expansion` lifts it to 8 MiB from `early_apply`, which is **after** `resolve_all` — a latent race | Apply the arena patch from inside the `startup.arc` open (targeted 2-pattern scan) | §7.5 |
| `score_guard` has per-song and session-sticky taints; stage save trampoline suppresses saves; logout sanitiser wipes tainted sides | Add a global injected-song taint latched at the scene-26 commit; fail-closed: hide songs if full sanitisation is unavailable | §12 |
| `folder-expansion` already supports custom genre folders keyed on `<property>` bits ≥ 10 with shipped static art, and `exclude_from_all_music` | One STEPMANIA folder is almost free; per-pack folders would need runtime text rendering (none exists) | `src/mods/folder_expansion.rs`, `data_mods/custom_folders/` |
| `series-expansion` supports custom series ≥ 22 with pre-rendered labels; series ≥ 22 are excluded from flare skill and raise the thumbnail-arc loop bound | One shared STEPMANIA series gives flare exclusion + a thumbnail arc the game requests itself | §7.4, §10.3 |
| `build_ssq_path` is detoured privately by `split-ssq-auto-discovery` | Per-chart SSC timing needs split SSQs → promote to a `services/ssq_path` resolver chain (one-detour rule) | §8.4 |
| LayeredFS can only redirect to a real path today; close/lseek/fstat are resolved but not hooked | Delivery choice: materialise artefacts under `data_mods/_cache/stepmania/` (no new hooks) vs a virtual-fd layer (3 new hooks + fd table + `CreateFileA` placeholder) | §11 |
| Several DLL-side readers read SSQs from host paths (`chart_length`, `fast_bootup` identity, `background_dancers::tempo_source`, `per_song_judgement_offsets::bootstrap`, split discovery) | Whatever delivery is chosen, these need one shared resolver | §11.5 |
| Tools already in-tree: deterministic MS-ADPCM codec, streaming XWB parser/serializer (re-emits parsed banks, no from-scratch builder), resampler, ARC writer, PNG/DXT pipeline (`image`, `texpresso`), SSQ chunk walker (parse only), fast-bootup SSQ analysis cache | New pure code needed: SM/SSC parser, SSQ writer, song-profile XSB writer, from-scratch XWB builder, audio decoders | §8.1, §9.2–§9.3, `Cargo.toml` |

## 3. Things that change or sharpen the rough idea

1. **"Without converting" means no user-facing step.** The game can only consume SSQ /
   XWB / XSB / ARC; the DLL must convert in memory (or into its own cache) at runtime.
   Nothing is written into the user's pack folders, and `musicdb.xml` on disk is never
   edited.
2. **Songs appear only across reboots.** The music DB is built once at boot. Adding a
   pack = next boot. The mod is boot-only in the `Mod::is_active` sense.
3. **Packs must not live under `data_mods/`** — every `data_mods/*` folder is a LayeredFS
   mod root and is walked into the index. A sibling root (e.g. `./stepmania/Songs/`) plus
   configurable extra roots is the natural layout.
4. **The folder is cheap; the server guarantee is a whole subsystem.** Beyond the stage
   save there are leak paths: the logout re-send, `/lastplay`, the `mod_judge_offsets`
   wire string, `eventlog`/`pcbevent` payloads, link/matching play across cabinets.
5. **Boot safety is the top fidelity risk**, not sync: a malformed chart can block boot
   (`ME1529`), so declaration must be gated on a self-check, with quarantine as defence
   in depth.
6. **Audio latency drives the architecture** (whole-file XWB reads, 0.4 s preview
   budget): on-highlight synthesis, LRU caches, and a fallback for "not ready at open".
7. **Delivery mechanism is an open design choice.** The feasibility note recommends
   virtual fds; a materialise-to-`_cache` approach needs no new hooks, persists converted
   audio across boots (replays and reboots become free), and gives the DLL-side readers a
   real path. See the register (D-delivery).
8. **SM gimmicks have no SSQ form.** Warps / negative BPM / negative stops → hide the
   chart (default) or approximate. Delays → stop one tick early. Quads must never emit a
   shock byte. Lifts/fakes dropped. Rolls → holds.
9. **Difficulty meters differ.** ITG meters run past 19; DDR has 1–19 (20 art
   unverified). Needs a per-pack meter map (default clamp).
10. **Radar normalisation is library-global** (inferred): extreme ITG charts could shrink
    every stock song's radar unless injected items are excluded from the accumulators.

## 4. Unknowns (from §16, still open)

| # | Question | Blocks |
|---|---|---|
| E1 | Does a hand-built injected entry appear on the wheel, preview, play, reach results — online and offline? | Everything; this is the Phase-0 spike |
| E2 | Which thread performs FileManager opens/reads? | Whether a provider may block in the open |
| E3 | Property-arena headroom per entry | Whether the in-open arena patch is mandatory at small N |
| E4 | AVS's real fd range | Virtual-fd design only |
| E5 | Behaviour with missing `_jk` / `_tn`; late-loaded thumbnail arc | Art fallback |
| E6 | `/lastplay`, `eventlog`, `pcbevent` payloads after an injected play | Server-leak sanitiser scope |
| E7 | Delay-as-early-stop fidelity; steep tempo segments | Gimmick policy |
| E8 | Radar maxima consumer | Whether exclusion is required |
| E9 | Level art above 19 | Meter clamp |
| E10 | MP3/Vorbis start alignment vs StepMania | Sync |
| E11 | Wheel scalability with thousands of songs | Caps |

## 5. ddr-chart-tools

See `research/ddr-chart-tools.md` (survey of the sibling checkout).

## 6. Proposed sequence

Clarification first (the decision space is wide and well understood from the feasibility
note), with one research track in parallel (the `ddr-chart-tools` survey, running now),
then readiness → design → plan. Phase-0 cabinet spikes (E1–E4, E6) are implementation
steps, not PDD research: they need a deploy.
