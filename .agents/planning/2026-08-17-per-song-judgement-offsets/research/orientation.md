# Orientation: Per-Song Judgement Offsets

Findings from the initial blind-spot pass over both codebases (DLL + bemani-buddy).

## 1. Stock JUDGEMENT OFFSET — where it lives, how it's consumed

- `ddr::player::Option+0x24` = `timing_music` (JUDGMENT TIMING, ±100 ms). The Option
  struct is **inlined into PlayerWork at +0xE0**, so `Option+0x24` ≡ `PlayerWork+0x104`.
  Attested by `src/mods/assist_tick.rs:175-214` (`CTX_OPTION_OFFSET = 0xE0`,
  `OPTION_TIMING_MUSIC = 0x24`), `src/mods/mine_render.rs:98-113`, and
  `docs/song_playback_speed.md` (layout table, stable since 20250805).
- Read path already shipped: `player_option_ctx_load` AOB + `derive_player_option_table`
  (`src/core/signatures.rs:1309, 3039-3083`), verified on all four builds. Walk:
  `*(*(table + side*8)) + 0xE0 + 0x24`.
- Sign convention: positive = judged moment LATER (ear-validated in assist_tick).
- Consumption: per `ra-rb-timing-chain.md`, judgement timing is consumed **at
  judge-compare time inside judgeNotes** — not baked into note timestamps. So a
  write at GAMEPLAY entry (or first judge dispatch) takes effect for the whole song.
- Write precedent: `src/services/song_rate/real_speed.rs:249` writes `Option+0x10`
  per side at the side's first judge dispatch (GAMEPLAY-entry latch + judge_hook).
  `song_reset` writes `Option+0x7C`. Nothing writes `+0x24` today.

## 2. THE critical hazard: profile write-back

The game's save marshal (`ReflectSavePlayerData` → ess `sys_playerdata_save_sender`)
sources the native ~29-field option block **from PlayerWork** — the same object we
would override. Saves fire:

- `savekind=2` after **every song** (per-stage save), and
- `savekind=3` at card-out (the profile write-back).

An override left in `Option+0x24` at save time would likely be marshalled into the
native option block and **permanently overwrite the player's stock JUDGEMENT OFFSET
on the server**. This is the central design problem. Available levers (all proven):

- `save_sender_trampoline` (`src/services/custom_options_persistence.rs:707`) runs
  *before* `original.call` — a restore-then-forward can be hosted there.
- EAM_EXIT scene callback precedent (`register_logout_sanitiser`) — pre-save memory
  edits with proven timing.
- Post-build tree edit precedent (`strip_league_node`, libavs ordinal 164).
- Simplest: restore stock value on GAMEPLAY exit (28→29 scene change) + a
  belt-and-braces restore in the save trampoline.

Open RE question: confirm whether the savekind-2 marshal includes the option block
(the report says yes — the native block is emitted under `/data/option` on the saves
the trampoline sees), and exactly *when* it snapshots PlayerWork relative to scene 29.
Also possible shortcut: bemani-buddy's protocol model
(`models/ddr_world/playdata_3.json`) names every native option field — we can identify
the judgement-timing field name without Ghidra.

## 3. Custom options framework — fit assessment

- `RegisterSpec::scalar` supports negative ranges; `ScalarFormat::Integer` renders
  negatives correctly (`rows.rs:1998-2016`). A −100..+100 step-1 row is representable.
- **One value per side, not per (side, song)** — the row is a *view*; the per-song
  store must be mod-owned. Re-seed on wheel-selection change via
  `set_value_silent(id, side, v)` (no callback — the profile_fields::seed pattern);
  capture user edits via `on_change(side, value)`.
- Values are read live from the registry per frame, so programmatic updates repaint
  an open menu same-frame (`mod.rs:398-406`). `set_scalar_bounds` precedent shows
  runtime mutation while the menu is open works (Training Mode).
- **OFF state**: scalar rows have no OFF representation today. Options: (a) extend
  `ScalarFormat` with a sentinel-renders-as-OFF variant, (b) parent bool + child
  scalar pair (`ShowWhen::Equals`) — both have full precedent (assist_tick volume
  child). Decision for the register.
- Menu lifecycle: options modal is a child of scene 25; `on_menu_open/close` hooks
  exist per side.

## 4. Song-wheel selection visibility

`src/mods/music_wheel_song_length.rs` gives the exact pattern to reuse:
`selectmusic_model` signature (already in `signatures.rs`, verified 4 builds) →
`*(model)+0x1B0` weak_ptr poll in an `input_manager::on_frame` callback → liveness
via ctrl-block strong count → `read_song_code()` (vtable getter, fully guarded).
Selection change = raw pointer comparison. `chart_length::request` shows the
follow-on-lookup idiom.

## 5. Persistence

### Local CSV
- No CSV reader exists in the DLL; only writer precedent is
  `power_user_statistics/csv_export.rs` (hand-rolled, CWD-relative sibling dir).
  Hand-rolled parse is trivial; `mod-config.json` is CWD-relative
  (`config.rs:292`) so `judgement_offsets.csv` beside it = just the bare filename.

### Network — the string channel does NOT exist yet
- The existing `mod_{id}` wire channel is **s32-only** (kbin type 6,
  `emit_network_children` at `custom_options_persistence.rs:1016-1022`). A string
  payload needs a new emit path (kbin `str` child) and a load-side string read.
  libavs ordinals for property ops are already resolved (162/163/175/176, +164);
  need to check which ordinal(s) create/read str-typed nodes.
- Server side (bemani-buddy) is fully ready for it:
  - kbin `str` nodes are u32-length-prefixed — no protocol size limit.
  - TEXT column precedent exists (`scores.ghost TEXT`), round-trips via kbin str.
  - Standard add-a-field commit anatomy (from `git show 072bcf8`): migration +
    model + DAO (3 spots) + protocol struct + JSON model + handler save/load lines +
    new-player None + tests + `.sqlx` regen.
  - Known gotcha: `models/ddr_world/playdata_3.json` and the generated
    `playdata_3.rs` are currently desynced (015 field hand-edited into .rs only).
  - Hard rule: nullable, no default, omitted-when-NULL (un-hooked-client safety).

### Per-side vs per-profile
CSV has `p1_offset,p2_offset` (cabinet-local sides); the network string is
per-*profile* (each side's save carries its own profile), so the wire string needs
only `song|offset` pairs. Merge semantics (server load vs local CSV precedence)
need a register decision — existing pattern is "JSON cache offline fallback,
network load wins".

## 6. Sizing

A heavy player might maintain offsets for ~200–500 songs. At ~12 bytes/pair the
wire string is ≤ 6 KB — trivially within kbin str and MySQL TEXT (64 KiB).

## 6b. Write-back hazard CONFIRMED (bemani-buddy protocol check)

The native option field is **`timing_music`** (`models/ddr_world/playdata_3.json:95`).
bemani-buddy's `handle_save_profile` parses `<timing_music>` on **every savekind
(1, 2, and 3)** and writes it to `opt_timing_music` (playdata.rs:689), and echoes it
back on load (playdata.rs:329). So any override left in `Option+0x24` at any save is
persisted to the profile — the restore mechanism is mandatory, and it must cover the
per-stage (savekind 2) save, not just logout.

## 7. Unknowns to research (Step 4 candidates)

1. ~~Which native option field is judgement timing~~ — CONFIRMED: `timing_music`
   (see §6b).
2. Exact stock JUDGEMENT OFFSET range/step in the game UI (±100? step 1?) so the
   new row matches. (assist_tick clamps ±100 as a sanity bound.)
3. When the savekind-2 marshal reads PlayerWork relative to scene 28→29, to place
   the restore point safely.
4. libavs property-node creation ordinal for str-typed children (DLL emit path).
5. Whether the stock options menu's own JUDGEMENT OFFSET row would visibly show our
   override if we write early (argues for writing only during gameplay).
