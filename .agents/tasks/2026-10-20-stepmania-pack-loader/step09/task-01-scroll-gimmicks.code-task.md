# Task: Scroll gimmicks — display-only `#SPEEDS` / `#SCROLLS` remap

## Description
Reproduce StepMania's visual scroll gimmicks for injected songs by remapping the beat delta
the game's per-note scroll function receives (`#SCROLLS` → display beat) and scaling its
result (`#SPEEDS` → distance multiplier), per frame, without touching timing or judgement.
Also adopt StepMania's hold-error tolerance in the parser.

## Background
`get_offset_y(dBeatCount: i64, speed: i32 /*×100*/, boost: i32, musicCount: i32) -> f32`
(signature `get_offset_y`, `FUN_180024a00`) is the pure scroll-Y function every note pass,
the freeze head, the shock pass, the guideline draw and `note_types_expansion::mine_render`
call; `dBeatCount = note.beat_count − renderer.beat_count` with 1024 ticks per beat
(`GameNote.beat_count`; renderer `+0xA8` current beat, `+0xAC` current music ms, `+0xA0`
speed×100, `+0xA4` boost mode). Nobody detours it today (`note_types_expansion` only CALLS it
through the signature). `render_notes_hook` is the per-frame dispatcher with the renderer
pointer (pre-subscribers run before the stock render); `cull_window`/`playfield_styling`
own the 720-px collector cull and expose contribution setters. StepMania semantics
(`TimingData`): `#SCROLLS: beat=factor,…` — piecewise-constant factor; display beat =
∫ factor d(beat) (negative allowed; identity when the only segment is `0=1`);
`#SPEEDS: beat=ratio=delay=unit,…` — the distance multiplier ramps linearly from the previous
segment's ratio to `ratio` over `delay` beats (unit 0) or seconds (unit 1) starting at
`beat`, then holds; drawn offset = (display(note) − display(now)) × speed(now) × speed-mod.
Acceptance chart: Tech Spectrum Super `260-MISC.[MASTER] Matt Silver` (`#SPEEDS` …
`160.000=0.000=0.000=0, 160.021=1.000=6.000=0, 196.000=0.500=0.000=0, 196.021=1.000=16.000=0`;
`#SCROLLS` 0-factor regions at 116.0–116.5, 171.5–171.75, 264.167–282.667 alternating
0/1.5, 293=0). Parser strictness to relax: StepMania turns an unclosed hold head into a tap
and ignores an orphan `3` (two TSS songs are rejected today for this).

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-10-20-stepmania-pack-loader/design/detailed-design.md (F8, §4.1)
- Plan: .agents/planning/2026-10-20-stepmania-pack-loader/implementation/plan.md — Step 9 (semantics, pieces, tests)

**Additional References (if relevant to this task):**
- src/mods/note_types_expansion/mine_render.rs (`actor::{SPEED,BOOST,BEAT_COUNT,MUSIC_COUNT}`, the `get_offset_y` call shape), src/services/render_notes_hook.rs (pre-subscriber API), src/services/cull_window.rs + src/mods/playfield_styling (cull contributions), docs/custom_arrow_renderer_research.md §6–§7, docs/playfield_styling_research.md
- src/core/sm/{simfile,notes,model}.rs (Step 4), src/services/stepmania/{index,model,mod,taint}.rs (per-song data to the runtime; `FORMAT_VERSION`)
- .agents/summary/interfaces.md (hook-ownership map — add the `get_offset_y` row)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `core/sm/scroll.rs` (pure, harness-mounted): `ScrollTable { segments: Vec<(Beat, f64)> }` with `display_beat(beat: f64) -> f64` (piecewise-linear integral; identity when empty/`0=1`), `SpeedTable { segments: Vec<SpeedSeg { beat, ratio, delay, unit }> }` with `speed_at(beat: f64, song_s: f64) -> f64` (ramp from the previous ratio; `unit` 1 ramps in seconds using `song_s` and the ramp's start time — the caller supplies the beat→seconds mapping via a closure or the tempo), both parsed from `#SCROLLS`/`#SPEEDS` text (tolerant like the other lists). Model: `Song.scrolls`, `Song.speeds` (per-chart `#SCROLLS/#SPEEDS` inside `#NOTEDATA` override the song's — StepMania supports both; keep per-chart on `Chart`). `is_identity()` so songs without gimmicks cost nothing.
2. Parser: unclosed hold head → `Tap` + warning; orphan tail → ignored + warning (match StepMania); update tests.
3. Index: store the committed chart's tables per basename (compact in `index.bin`; `FORMAT_VERSION` → 3); `stepmania::scroll_tables_for(basename) -> Option<Arc<ScrollGimmicks>>`; publish the current song's tables at gameplay load (`taint::note_bank_request` already identifies the full-bank serve — hook the publication there) and clear at scene exit.
4. `services/scroll_remap.rs` (new, single owner of the `get_offset_y` detour; name the service `scroll_remap` and list it in `interfaces.md`): a `render_notes_hook` pre-subscriber snapshots `(renderer.beat_count, renderer.music_count)` per frame into statics; the detour: `if !active → original(args)`; else `now = snapshot.beat / 1024.0`, `note = now + dBeat/1024.0`, `d' = (display(note) − display(now)) × 1024`, `y = original(d' as i64, speed, boost, music)`, `y × speed_at(now, music/1000)`. Allocation-free, panic-free (pure arithmetic; tables behind an `ArcSwap`-like `Mutex<Option<Arc<_>>>` read once per frame into a static, not per call), `f32` result. Installed via `hooks::install_enabled` from the service's `init(signatures)`; fail-open.
5. Cull: when the song has gimmicks, register a cull contribution so notes whose remapped offset is within the window are collected (the collector compares `get_offset_y(...)` against the (patched) 720 bound — since the detour remaps the collector's call too, verify that the collector uses the same function; if it does, no extra widening is needed except for speed < 1 regions where far notes land inside the window — which the remap already makes visible). Document what you find.
6. Config: `stepmania_packs.scroll_gimmicks: bool` (default true) — when false the tables are not published.
7. Tests: `scroll.rs` vectors (identity; a 0-factor hold flattens display beat; negative factor; the Matt Silver tables: `display_beat(116.25)==display_beat(116.0)`, `display_beat(116.5+x)` resumes; `speed_at(160.0)==0`, `speed_at(163.0)≈0.5`, `speed_at(166.021+)==1`, the 196 ramp 0.5→1 over 16 beats; unit-1 ramp in seconds); remap arithmetic (`d'` for a few (now, dBeat) pairs; speed scaling); harness leg.
8. Gates: `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`, `./build.sh`, `./scripts/validate_stepmania.sh` green with the new legs, path-leak grep. `.agents/summary/interfaces.md` row for `get_offset_y`.

## Acceptance Criteria
1. **Matt Silver** — Given the song playing (cabinet), then notes squeeze onto the receptors around beat 160 (combo ~582) and expand back over 6 beats; `#SCROLLS 0` regions show frozen-then-released notes; the judgement CSV is identical to a run with `scroll_gimmicks=false`.
2. **Stock untouched** — Given a stock song or an injected song without gimmicks, then the detour passes straight through (`active=false`) and nothing changes visually.
3. **Composition** — Given BOOST/BRAKE/WAVE or the hallway perspective, then the remap composes (remapped delta in, stock math, scaled out).
4. **Parser** — Given `Tell` and `Glacierfall`, then both index with their charts kept and a warning.

## Metadata
- **Complexity**: High
- **Labels**: renderer, hook-ownership, parser, pure
- **Required Skills**: Rust hooks, StepMania timing semantics
- **Generated By**: code-task-generator 2026-10-23
- **Source Plan**: .agents/planning/2026-10-20-stepmania-pack-loader/implementation/plan.md
- **Plan Step**: Step 9: Scroll gimmicks — display-only `#SPEEDS` / `#SCROLLS` remap
