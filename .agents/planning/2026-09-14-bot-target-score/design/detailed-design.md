# Detailed Design — Multiplayer Bot "Target Score" tier

Status: Approved 2026-09-14

## Overview

The Multiplayer Bot (`src/mods/multiplayer_bot/`) turns a solo song into a
genuine 2P VERSUS session against a computer opponent whose per-note judge
offsets come from a skill model driven by a BOT LEVEL 1–10. This feature adds
an eleventh selector value, **Target Score**: instead of the skill model, the
bot reproduces — note for note — the GHOST DATA the game has already loaded for
the human's pacemaker target (own PB, a rival's score, or the world / area /
machine record chosen at song select). The ghost carries one grade class per
note and no timing, so each reproduced step is given a random offset INSIDE the
window of its ghost grade. The result is a live race against a replay of any
target score; because the game's money score is a pure function of grade
counts, a faithful replay ends on exactly the target's points.

Two maintainer directives shape the rest: the bot's options persist **locally
only** (never over the network — no backend work), and the level selector
renders **text** values (`Level 1` … `Level 10`, `Target Score`) with no new
chip textures.

## Detailed Requirements

Functional

- R1 The `bot_opponent_level` row spans 1..=11; values 1–10 display `Level N`,
  11 displays `Target Score`, in both the in-game options menu and the overlay
  menu. Fine and coarse steps are 1.
- R2 With value 11 and the bot eligible, the bot's every tap note is decided
  from the human's loaded ghost: grade byte 0–3 ⇒ a hit whose |offset| lies in
  that grade's inclusive window (Marvelous ≤ 17, Perfect 18–34, Great 35–84,
  Good 85–124); 5 ⇒ Miss; 4 (Boo, never produced by World) and 7 ⇒ Miss;
  6 (O.K.) on a tap ⇒ Marvelous.
- R3 A ghost freeze N.G. (byte 7 at a kind-2 tail) is reproduced by not
  holding the freeze body; a ghost shock N.G. (byte 7 at a shock note) by one
  press on a shock panel inside the shock window. Ghost O.K.s need no action
  (the bot holds every body and avoids every shock today).
- R4 When the S-Marvelous mod is enabled and the bot side is armed with window
  `W`, a ghost Marvelous samples |offset| ∈ [W+1, 17] — the replay never
  produces an S-Marvelous. Levels 1–10 are unchanged.
- R5 Offsets inside a band are uniform in magnitude; the FAST/SLOW side comes
  from the existing sticky per-song side chain so a replay shows human-looking
  early/late runs.
- R6 If no usable ghost exists at the bot's first judge frame (empty vector,
  ghost id 0, failed download, `ghost.len() ≠ results.len()`, or the GhostActor
  offset underived this boot), the bot plays that song at Level 10, logs one
  WARN naming the reason, and shows a 3 s toast `NO TARGET GHOST - BOT LV10`
  when the toast service is up.
- R7 Name plate: `TARGET` in Target mode; `BOT LV<n>` otherwise.
- R8 Both bot rows (`bot_opponent`, `bot_opponent_level`) persist through the
  offline JSON cache only; neither is emitted in, nor applied from, a network
  save/load. Existing cached values (ids unchanged) keep working; a cached or
  primed value outside 1..=11 clamps.
- R9 The song-end restore INFO reports the mode and, in Target mode, the ghost
  provenance (id, length, per-grade histogram) and a reproduction-miss count.

Non-functional / safety

- R10 Zero new detours. One new AOB (`gpa_ghost_actor_probe`) with a derived,
  published GamePlayActor offset and a callee-prologue identity gate; one new
  RTTI vtable (`ghost_actor_vtable`). Every game pointer is
  `memory::is_readable`-probed before use; the GhostActor is vtable-identity
  gated and must be in state 2.
- R11 Hot path unchanged in cost: the ghost bytes are read ONCE per Results
  rebuild (first frame of a song / in-place reset) and cached; per-note
  decisions are O(1) lookups plus the existing planner walk.
- R12 Fail-open everywhere: any refusal degrades to R6, never to a stuck or
  input-less bot.
- R13 Pure modules stay dependency-free so `tools/bot_sim` mounts them; every
  new pure rule is host-tested.

Assumptions

- A1 The human's ghost vector is final before the bot's first judge frame:
  `GamePlayActor::onUpdate` state 2 waits on `GhostActor::isReady`
  (state == 2), and the GhostActor reaches state 2 on download success,
  download failure, and request failure (Ghidra, gamemdx 20260825
  `FUN_18005cc70` / `FUN_1800569b0` / `FUN_180056d10`).
- A2 Ghost byte `i` is the grade of Results entry `i` of the same chart (the
  result commit writes the record's grade stream from the actor's Results ring
  in order; the wire `<ghost>` is that stream as `'0'+grade`; bemani-buddy
  stores and serves it verbatim). The bot plays the human's chart with the
  human's option block, so indices align whenever the target's play used the
  same chart shape.
- A3 The freeze judge resolves N.G. whenever any body panel was released during
  the body; the shock judge resolves N.G. on a `wasJustPressed` on a shock
  panel inside `[mc−34, mc+84]` (documented in the gauge/judge scoring RE).
- A4 Every selectable target carries a ghost id (own PBs via `playerdata_load`,
  rival/world/area/machine via `rivaldata_load`; `ghostdata_load(id)` serves
  the stored string, empty when the saving play had none).

## Architecture Overview

```mermaid
flowchart LR
  subgraph options[custom_options]
    ROW[bot_opponent_level\nScalar 1..=11\nScalarFormat::Labeled\nPersistMode::Local]
  end
  subgraph flip[song-select → stage edge]
    ELIG[eligibility::evaluate\nPlan{human, bot, mode}]
    IMP[impersonation::apply\nplate TARGET / BOT LVn\nfiller::start_song(mode)]
  end
  subgraph song[GAMEPLAY — judge pre-callback]
    FILL[filler::fill\nResults rebuild]
    SRC[ghost_source::read_human_ghost\nGPA_h + gpa_ghost_actor_off → GhostActor\nvtable gate · state==2 · +0x98 vector]
    ST[planner::SongState\nwith_ghost(bytes, smarv_floor)\nor Level-10 fallback]
    PL[planner::plan_frame\ntaps via ghost::decide_tap\nfreeze N.G. drop hold · shock N.G. press]
  end
  ROW --> ELIG --> IMP --> FILL
  FILL -->|first frame| SRC --> ST --> PL
  PL --> FLAGS[BotPanelFlags → foot_panel_swap → judgeNotes]
```

Data flow for one Target-mode song: the flip stores `BotMode::Target` in the
filler's `SongCtx`; the first `fill()` (Results length known) reads the human's
GhostActor vector, validates it, and builds a `SongState` carrying the bytes
and the S-Marv floor; every later frame is the existing planner walk with the
decision source swapped.

## Components and Interfaces

### Options framework (`src/services/custom_options/`)

- `PersistMode::Local` — matrix `(saved_to_network: false, loaded_from_network:
  false, json_cached: true, session_scoped: false)`. The single load gate
  `resolve_from_load` grows a `LoadSource { Network, JsonPrime }` argument:
  `Network` gates on `loaded_from_network()`, `JsonPrime` on `json_cached()`.
  Call sites: `custom_options_persistence::apply_pending_loads` (Network) and
  `json_load_once` (JsonPrime). Existing modes keep identical behaviour (for
  `Full` both gates are true; for the other three both are false).
- `ScalarFormat::Labeled { prefix: &'static str, terminal_value: i32,
  terminal_label: &'static str }` — renders `"{prefix}{value}"` except at
  `terminal_value`, which renders `terminal_label`. Plain ASCII, `Copy`,
  handled in `format_scalar_value` (bytes) and therefore in
  `format_scalar_value_utf8` (overlay).

### Signatures (`src/core/signatures.rs`)

- New AOB `gpa_ghost_actor_probe` = the GhostActor wait in
  `GamePlayActor::onUpdate` state 2:
  `48 8B 8F ?? ?? 00 00 48 85 C9 74 ?? E8 ?? ?? ?? ?? 84 C0 0F 84`
  (`MOV RCX,[RDI+disp32]; TEST; JZ; CALL isReady; TEST AL,AL; JZ`). Unique on
  20260825 / 20260224 / 20250805 (verified); 20260721 via the sweep.
- Derivation `derive_ghost_actor_probe` (soft, `get_address`): publishes
  `gpa_ghost_actor_off` = disp32 at match+3 (expected `0x1F8` on 2026-03+,
  `0x1F0` on 20250805/20260224) ONLY IF the CALL target at match+12 contains,
  within its first 0x30 bytes, the byte run
  `0F B7 81 82 00 00 00 48 8B D9 83 7C C1 58 02 74`
  (`MOVZX EAX,[RCX+0x82]; MOV RBX,RCX; CMP dword [RCX+RAX*8+0x58],2; JZ`) —
  the identity gate that also re-attests the GhostActor state layout per
  build. Accessor `SignatureStore::gpa_ghost_actor_off() -> Option<usize>`.
- RTTI `.?AVGhostActor@dance@sequence@@` → `ghost_actor_vtable` (added to the
  existing RTTI vtable list).

### `multiplayer_bot::eligibility` (pure)

- `pub const TARGET_VALUE: i32 = 11;` `pub fn clamp_value(raw: i32) -> i32`
  (1..=11).
- `pub enum BotMode { Level(u8), Target }` with `BotMode::from_value(i32)`
  (clamps; 11 ⇒ Target) and `fn seed_level(&self) -> u8` (level, or 11).
- `Plan { human, bot, mode: BotMode }` (replaces `level`).

### `multiplayer_bot::session` (pure)

- `format_bot_name(mode: BotMode)` — `TARGET` for `Target`, `BOT LV<n>` else.

### `multiplayer_bot::skill` (pure)

- `Form::next_side(&mut self, rng, c) -> f64` — the sticky Markov side draw,
  factored out of `next_lean` (which now calls it, behaviour unchanged).
- `Rng::below(&mut self, n: u32) -> u32` — uniform integer in `0..n`.

### `multiplayer_bot::ghost` (pure, NEW)

- Grade constants `GRADE_MARVELOUS..=GRADE_NG` (0..=7).
- `pub fn tap_band(grade: u8, smarv_floor: i32) -> Option<(i32, i32)>` —
  inclusive |d| band for a tap; `None` = Miss. Marvelous band is
  `(min(smarv_floor + 1, 17), 17)` when `smarv_floor > 0`, else `(0, 17)`.
  Perfect `(18, 34)`, Great `(35, 84)`, Good `(85, 124)`; 4/5/7 ⇒ `None`;
  6 ⇒ the Marvelous band.
- `pub fn decide_tap(rng, form, c, grade, smarv_floor) -> Plan` — magnitude
  uniform in the band, sign from `form.next_side`.
- `pub fn histogram(bytes: &[u8]) -> [u32; 8]` for diagnostics.
- `pub fn expected_grade(byte: u8) -> u8` — the grade the replay aims to
  reproduce (6 on a tap ⇒ 0; 4/7 ⇒ 5), used by the reproduction-miss counter.

### `multiplayer_bot::planner` (pure)

- `SongState::with_ghost(capacity, bytes: Vec<u8>, smarv_floor: i32)`; the
  state keeps `ghost: Option<GhostPlan { bytes, smarv_floor }>`,
  `drop_hold: Vec<bool>` (per Results index) and `repro_miss: u32`.
- `resolve`: when `ghost` is set and the note is a tap, the raw plan comes from
  `ghost::decide_tap(rng, form, c, bytes[idx], smarv_floor)`; a missing byte
  (index past the vector — cannot happen after the length check, kept for
  safety) falls back to `skill::decide`. After the floors, if the resolved
  grade differs from `ghost::expected_grade(bytes[idx])`, `repro_miss += 1`.
- Freeze N.G.: when a head (kind 0, some `length[p] > 0`) resolves and the
  ghost byte of its tail is 7, mark `drop_hold[head]` and `drop_hold[tail]`.
  Tail lookup = the first later entry with `kind == 2`, `beat_count ==
  head.beat_count + max(length)` and a shared panel with `state ≥ 2`, bounded
  to 512 entries forward. The body-hold emission skips notes with `drop_hold`.
- Shock N.G.: for a shock note whose ghost byte is 7, once `mc ≥ note.mc`, set
  `was_just_pressed = 1` on the FIRST shock panel in addition to the stock
  assignment for the other panels (the shock judge keys on `wasJustPressed`
  only; the tap judge keys on `is_held`, which stays 0).
- Tally: unchanged (`planned` still counts the bot's own resolved grades).

### `multiplayer_bot::ghost_source` (engine-facing, NEW)

- `init(signatures)` captures `gpa_ghost_actor_off` and `ghost_actor_vtable`
  into statics; `is_available()`.
- `read_human_ghost(bot_side) -> Result<Ghost { id: i64, bytes: Vec<u8> },
  Refusal>` with `Refusal::{Unavailable(&str), NoActor, NotReady, Empty,
  TooLong}`: walks `song_reset::live_dps()` → `gameplay_actors(dps)`, picks the
  actor whose `+0x84 != bot_side`; probes `gpa + off` (8 bytes), reads the
  GhostActor pointer, probes `0xA0` bytes, checks `*ghost == ghost_actor_vtable`,
  `idx = u16 @+0x82 ≤ 8`, `state @+0x58+idx*8 == 2`, reads the vector
  `+0x98..+0xA0` (`len ≤ 100_000`), copies it. Game-thread only (called from
  the judge pre-callback).

### `multiplayer_bot::filler` (engine-facing)

- `start_song(side, mode: BotMode, seed)`; `SongCtx` gains `mode`,
  `ghost: Option<Vec<u8>>` (cached bytes), `ghost_id`, `ghost_hist: [u32; 8]`,
  `fallback: Option<&'static str>`, `smarv_floor`.
- On the Results rebuild (`views.len() != count`), in Target mode: bind the
  ghost (cached, else `ghost_source::read_human_ghost`); require `len ==
  count`; `smarv_floor = s_marvelous::state::armed_window(side)` when
  `s_marvelous::is_enabled()`, else 0; `st = SongState::with_ghost(...)`. On any
  refusal: `curve = skill::curve(10)`, `st = SongState::new(count)`, record the
  reason, one WARN (`MultiplayerBot: no usable target ghost (<reason>) --
  playing at LV10`) and `toast::flash_with_hold("NO TARGET GHOST - BOT LV10",
  3000)`, both once per song.
- `SongSummary` gains `mode`, `ghost_id`, `ghost_len`, `ghost_hist`,
  `repro_miss`, `fallback`.

### `multiplayer_bot::impersonation` / `mod`

- `Active.mode: BotMode`; plate via `format_bot_name(mode)`; `filler::start_song`
  and `skill::seed(..., mode.seed_level())`; the flip INFO prints the mode; the
  restore INFO appends `mode=target ghost_id=… ghost_len=… target=[…]
  repro_miss=…` (or `fallback=<reason>`).
- `mod.rs`: `LEVEL` atomics store 1..=11 via `clamp_value`; `level(side)` →
  `mode(side) -> BotMode`; row spec `RegisterSpec::scalar(OPT_LEVEL_ID, 1, 11,
  1, ScalarFormat::Labeled { prefix: "Level ", terminal_value: 11,
  terminal_label: "Target Score" }).persist_mode(PersistMode::Local)`, the bool
  row `.persist_mode(PersistMode::Local)`; `init` calls `ghost_source::init`
  (its miss is a WARN, not a mod refusal — R6 covers it).

### S-Marvelous (`src/mods/s_marvelous/state.rs`)

- `pub fn armed_window(side) -> i32` (0 when not armed) — read-only exposure of
  the armed window, `pub(crate)` re-exported from the mod.

## Data Models

Ghost byte alphabet (per Results entry, chart order): 0 Marvelous, 1 Perfect,
2 Great, 3 Good, 4 Boo, 5 Miss, 6 O.K., 7 N.G.

Tap bands (inclusive |d| ms) and the S-Marv floor `F` (0 = none):

| ghost | band | note |
|---|---|---|
| 0 / 6 | `[F>0 ? min(F+1,17) : 0, 17]` | `F=16` ⇒ exactly 17 |
| 1 | `[18, 34]` | |
| 2 | `[35, 84]` | |
| 3 | `[85, 124]` | |
| 4 / 5 / 7 | Miss | World never grades Boo; 7 on a tap is unreproducible |

`GhostActor` (World, all supported builds; state layout re-attested per build by
the derivation's identity gate): state pairs `i32 state, f32 timer` at
`+0x58 + idx*8`, `idx` u16 at `+0x82`, side i32 `+0x84`, ghost id i64 `+0x90`,
`vector<u8>` begin/end/cap at `+0x98/+0xA0/+0xA8`. Reached from
`GamePlayActor + gpa_ghost_actor_off` (published; `0x1F8` on 2026-03+, `0x1F0`
on 20250805 / 20260224).

Option value: `bot_opponent_level` ∈ 1..=11 (11 = Target), stored raw in the
JSON cache under `custom_options.p1/p2`; never on the wire.

## Error Handling

| Condition | Behaviour |
|---|---|
| `gpa_ghost_actor_probe` missing / identity gate fails / RTTI vtable absent | `gpa_ghost_actor_off` unpublished ⇒ `ghost_source` unavailable ⇒ every Target song takes R6 (`derivation missing`), one WARN at init |
| No live DPS / no human actor / pointer unreadable / vtable mismatch | R6 (`no actor` / `unreadable` / `identity`) |
| GhostActor state ≠ 2 at first fill (should not happen — A1) | R6 (`not ready`), WARN carries the state |
| Ghost empty (id 0, empty or failed download) | R6 (`empty`, id logged) |
| `ghost.len() ≠ results.len()` | R6 (`len mismatch a≠b`) — field data decides whether a relaxation is ever warranted |
| Ghost inconsistent with the chart (e.g. O.K. tail under a Missed head, floors pushing a hit past its band) | Reproduced as best the judge allows; counted in `repro_miss`, never a WARN |
| Toast service / widget renderer down | Toast skipped silently; WARN still logged |
| Persistence: a network load carrying `mod_bot_opponent*` (an old server) | Ignored (`Local` never loads from the network) |

All engine-facing code stays panic-free on the hot path (the filler's existing
`catch_unwind` backstop remains); the ghost read happens once per Results
rebuild inside that guard.

## Testing Strategy

Host (`cargo test` at the crate root for the framework; `tools/bot_sim`
mounts for the bot's pure files via `scripts/validate_multiplayer_bot.sh`):

- `persist_matrix_tests.rs`: `Local` row in the exact matrix; invariants
  updated (`loaded_from_network ⇒ saved_to_network`; `json_cached ⇒ !session`);
  a `Local` option is absent from the save snapshot, ignored by a Network load,
  applied by a JsonPrime load, and JSON-persisted.
- `scalar_format_tests.rs` / `api.rs` tests: `Labeled` renders `Level 1`,
  `Level 10`, `Target Score` in bytes and UTF-8.
- `ghost.rs`: band edges per grade; S-Marv floor 0 / 12 / 16 / 17; alphabet
  mapping (4/5/7 ⇒ Miss, 6 ⇒ Marvelous band); sampled magnitudes never leave
  the band over 1e5 draws; both signs occur; `expected_grade` table.
- `planner.rs`: ghost-driven tap resolves to the ghost grade (stock offsets);
  Marvelous never below the S-Marv floor; freeze tail byte 7 drops the body
  hold on head AND tail entries while a 6 keeps it; shock byte 7 presses
  exactly one shock panel from `note.mc`, byte 6 keeps the stock avoidance;
  `repro_miss` increments when a floor changes the grade; `with_ghost` state
  without a ghost byte falls back to the skill model.
- `eligibility.rs` / `session.rs`: `clamp_value` (0 → 1, 11 → 11, 12 → 11),
  `BotMode::from_value`, `format_bot_name(Target) == "TARGET"`.
- `skill.rs`: `next_side` refactor keeps the existing side-chain statistics
  tests green.

Cross-build: `./scripts/validate_signatures.sh ~/Desktop/ddr_modules` must show
`gpa_ghost_actor_probe` `[+]` on all four builds and `gpa_ghost_actor_off
(derived) = 0x1F0 / 0x1F8` as expected; `shape_diff.py` for the new AOB.

Cabinet (the only validation for the engine-facing wiring):

1. Options menu shows `Level 5` → `Target Score` as text; overlay PLAYER
   SETTINGS agrees; `mod-config.json` carries the value; the network save
   contains no `mod_bot_opponent*` fields (spice2x log / bemani-buddy request).
2. Target Score vs. own PB: log shows `mode=target ghost_id=<id> ghost_len=N`
   with `N == Results count`; the bot's final money score equals the target's
   points on the results screen; `repro_miss` is ~0; no S-Marvelous shown on
   the bot's lane with the S-Marv mod on.
3. Target Score on a never-scored chart: WARN `no usable target ghost (empty)`,
   toast, bot plays at LV10.
4. A chart with freezes / shocks whose target has N.G.s: the bot's N.G. counts
   match the ghost histogram.
5. Old-build sanity (20260224 or 20250805 cabinet): boot log
   `gpa_ghost_actor_off (derived) = 0x1F0`, then test 2.

## Appendix — Alternatives considered

- **Text-rendering Enum rows**: the Enum donor has no value TextLayer; adding
  one is a framework rework for no functional gain over a labelled Scalar row.
- **Refusing Target mode at the flip when no ghost exists**: requires
  reproducing the score-DB lookup (`FUN_18001dc00`) before the GhostActor
  exists — new RE for a case the R6 fallback covers.
- **Taps-only reproduction**: simpler, but the bot would beat any target that
  had freeze/shock N.G.s, breaking the score-parity invariant.
- **Prefix mapping on length mismatch**: misaligned indices reproduce the wrong
  grades silently; an explicit fallback plus a WARN with both lengths is the
  honest v1.
