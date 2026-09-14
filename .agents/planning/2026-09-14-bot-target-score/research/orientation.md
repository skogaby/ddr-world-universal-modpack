# Orientation — Target Score tier for the Multiplayer Bot

Research pass 2026-09-14. Sources: the shipped bot (`src/mods/multiplayer_bot/`),
the premium-free ghost cache (`src/mods/premium_free/ghost_cache.rs`),
`docs/premium_free_stale_record_bug.md` (2026-09-01 addendum),
`docs/pacemaker_display_research.md`, `docs/gauge_and_judge_scoring_research.md`,
`docs/s_marvelous_judgement_research.md` §3.8 + wire contract, the bemani-buddy
handlers (`crates/game-server/src/handlers/ddr_world/playdata.rs` in the sibling
checkout), and live Ghidra reads on gamemdx 20260825 / 20260224 / 20250805
(file-relative to `0x180000000`).

## 1. What the shipped bot already gives us

| Piece | Where | Relevance |
|---|---|---|
| Per-song decision = `skill::decide(rng, form, curve) -> Plan::{Hit{d_ms}, Miss}` | `skill.rs` | The ONE seam the target tier replaces: a per-note *grade → offset* sampler instead of the level curves |
| Planner resolves each Results index ONCE when it enters the walk window, applies per-panel monotonic floors, turns a floor past ±124 into a Miss | `planner.rs::resolve` | The decision source is injected through `resolve`; nothing else about the planner changes for taps |
| Freeze body hold: every frame, `was_just_pressed[p]=1` for panels with `state[p] >= 2` while `cur_beat < beat + max(length)` | `planner.rs::plan_frame` | Freeze O.K. is automatic today; a ghost **N.G.** needs the hold dropped for that head |
| Shock: presses every NON-shock panel (assignment) — always avoided | `planner.rs::plan_frame` | A ghost shock **N.G.** (7) needs a press on a shock panel inside `[mc−34, mc+84]` |
| `filler.rs` builds `NoteView`s from the live `GamePlayActor` Results vector (`actor+0xB0`, 0x40 stride, `NoteView.idx` = vector index) and owns the per-side `SongCtx { st, rng, curve, … }` | `filler.rs` | The ghost bytes bind here (once per song; re-read on the Results-length rebuild) |
| Level row = scalar 1..=10, `ScalarFormat::Integer`, `PersistMode::Full` (wire `mod_bot_opponent_level`), load clamp | `mod.rs` | Becomes 1..=11 with a text formatter; persistence becomes local-only |
| Name plate `BOT LV<n>` written at the flip (`PlayerWork+0xC`, 8 chars + NUL) | `session.rs::format_bot_name`, `impersonation.rs::apply` | Target tier needs its own plate |
| Eligibility gate → `Plan { human, bot, level }` | `eligibility.rs` | `level` grows a variant / value 11 |
| Host harness `tools/bot_sim` mounts `eligibility/planner/session/skill.rs` via `#[path]`; `scripts/validate_multiplayer_bot.sh` | | New pure module must be dependency-free and get mounted |

## 2. Ghost data — where it lives, when it is final, what it means

### 2.1 Storage shape (stage record ⇄ wire ⇄ GhostActor)

- Stage record `rec+0xB8..0xC0` = `vector<u8>`, **one grade-class byte per Results
  entry, in chart order** (written by the result commit from the actor's
  `+0xB0` ring; `docs/premium_free_stale_record_bug.md` table, S-Marv wire
  contract). Alphabet: 0 Marvelous, 1 Perfect, 2 Great, 3 Good, 4 Boo (never
  produced by World's judge), 5 Miss, 6 O.K. (freeze held / shock avoided),
  7 N.G. (freeze dropped / shock stepped). Unjudged slots of a failed-out play
  are `0`.
- Wire `<ghost>` = the same bytes as ASCII `'0'+grade` (decoder
  `ark::network::GetGhostData` = `c − 0x30`), `<ghostsize>` = length.
- bemani-buddy stores `ghost TEXT` verbatim per score row and hands EVERY score
  a ghost id: own PBs via `playerdata_load` (`score_str` field 6 = `s.id`),
  rival / world / area / machine records via `rivaldata_load` (last field =
  `score.id`). `ghostdata_load(ghostid)` returns the stored string
  (`ghostsize 0`, empty string when the row has none) — so **every selectable
  target has ghost data unless the play that set it saved an empty ghost**.
- `sequence::dance::GhostActor` (child of `GamePlayActor`): state pairs
  `+0x58 + idx*8` (idx u16 at `+0x82`; 0 pending, 1 polling, **2 ready**),
  side `+0x84`, `NoteResultActor*` `+0x88` (its `+0xC0` = pacemaker
  visibility), ghost id i64 `+0x90` (< 0 local stage slot, 0 none, > 0 network),
  **ghost `vector<u8>` `+0x98..+0xA0`**. Init (`ghost_actor_init` AOB, in
  `signatures.rs`) resolves the id and either copies a local slot (state 2 at
  once), kicks `GhostDataLoadRequest` (state 0→1), or leaves it empty (state 2).
- RTTI `.?AVGhostActor@dance@sequence@@` exists (20260825 `0x180482e70`) —
  `find_vtable_by_rtti` can supply an identity gate.

### 2.2 The ghost is FINAL before the first judge frame (Ghidra, 20260825)

`GamePlayActor::onUpdate` (`FUN_18005cc70`) state 2:

```
18005d186  48 8B 8F F8 01 00 00   MOV  RCX,[RDI+0x1F8]      ; GhostActor*
18005d18d  48 85 C9               TEST RCX,RCX
18005d190  74 0D                  JZ   advance
18005d192  E8 rel32               CALL GhostActor::isReady   ; FUN_1800569b0
18005d197  84 C0                  TEST AL,AL
18005d199  0F 84 ...              JZ   keep_waiting
```

`isReady` (`FUN_1800569b0`) = `state[idx] == 2`, else a `TIMEOUT_GHOST` clock
(default `0x7fffffff`). `GhostActor::onUpdate` (`FUN_180056d10`) moves to
state 2 on download success (decodes into `+0x98`, raises the visibility byte),
on download failure, AND on request failure — so the actor cannot advance past
state 2 (into the judging state 4) until the human's ghost vector is in its
final shape. **No "ghost arrives late" handling is needed: at the bot's first
`fill()` the human's GhostActor is state 2 and its vector is what the pacemaker
will use for the whole song.**

### 2.3 The GhostActor field is build-dependent (derive it)

| build | `MOV RCX,[RDI+disp32]` at the wait site | GhostActor offset |
|---|---|---|
| 20260825 | `0x18005d186` | **`+0x1F8`** |
| 20260224 | `0x180058dc6` | **`+0x1F0`** |
| 20250805 | `0x180059d86` | **`+0x1F0`** |

So the GamePlayActor layout fork already sits at `+0x1F0` (AGENTS.md's "≥ ~0x208"
was the lowest field anyone had needed so far). A hardcoded `+0x1F8` would read
the wrong field on the two old builds.

AOB `48 8B 8F ?? ?? 00 00 48 85 C9 74 ?? E8 ?? ?? ?? ?? 84 C0 0F 84` is
**unique on 20260825 / 20260224 / 20250805** (20260721 to be confirmed by the
sweep). Derivation: disp32 at match+3 → published `gpa_ghost_actor_off`; CALL
rel32 at match+12 → the `isReady` callee, whose prologue is byte-identical on
20260825 and 20250805 and pins the state layout:

```
0F B7 81 82 00 00 00   MOVZX EAX,[RCX+0x82]    ; state idx
48 8B D9               MOV   RBX,RCX
83 7C C1 58 02         CMP   dword [RCX+RAX*8+0x58],2   ; ready
74                     JZ
```

Requiring that shape inside the callee's first 0x30 bytes is the identity gate
(and re-attests `+0x82`/`+0x58`/ready==2 per build). Fail-closed: no offset ⇒
the Target tier is unavailable that boot.

### 2.4 Index alignment

The ghost byte for Results index `i` is the grade of note `i` of the SAME chart
(same mcode/style/difficulty, same note set). The bot's chart is the human's
(the flip copies the human's `Option` block, so CUT/JUMP/FREEZE-style chart
modifiers match the human's), but the TARGET's play may have used different
modifiers, and a stored ghost can be truncated. Policy needed for
`ghost.len() != results.len()`; exact match expected in the normal case.

## 3. Judge facts the sampler and planner extensions depend on

From `docs/gauge_and_judge_scoring_research.md` (all four builds):

- Tap windows (inclusive ms): Marvelous ±17, Perfect ±34, Great ±84, Good
  ±124; 125..160 matched-but-rejected ⇒ Miss at `mc > note.mc + 160`. World
  has no Boo.
- **Freeze judge**: for a judged, non-Missed head, while `cur_beat − note.beat
  < length` the panel must be held; a release bumps a per-panel release counter
  and the tail resolves **O.K. if every panel stayed held, N.G. otherwise**. A
  Missed head ⇒ N.G. ⇒ dropping the body hold reproduces a ghost N.G.
- **Shock**: `wasJustPressed` on a shock panel inside `[mc−34, mc+84]` ⇒ N.G.
  (grade 7); untouched through `mc+84` ⇒ O.K. (grade 6). One press at `mc`
  reproduces a ghost N.G.
- S-Marvelous (`src/mods/s_marvelous`): presentation tier `|d| ≤ W`, `W` =
  the armed window (`state::WINDOW_MS[side]`, set at play-scene entry from the
  live `LIVE_WINDOW_MS`, clamp 1..=16, default 12). Exclusive Marvelous is
  therefore `W < |d| ≤ 17`; `W = 16` leaves exactly `|d| = 17`.
- Money score depends only on grade counts
  (`((((Marv+OK+Perf)·5 + Great·3 + Good)·200000)/((taps+freezes+shocks)·10) −
  Good − Great − Perf)·10`), so a faithful grade replay reproduces the target's
  `points` exactly — a cabinet-checkable invariant.

## 4. Options framework facts

- **Text rendering**: only `UiKind::Scalar` rows render their value as text
  (the `OptionElement<int>` donor's value TextLayer → the game's
  `string::assign` + BmpString compositor, mixed case proven by the shipped
  `Char #` / `kg` / `ms` formats, 15-byte SSO comfortable for `Target Score`).
  `UiKind::Enum` rows render a `seop_op_<key>` chip texture per value — the
  maintainer wants no new chips, so the "enum-like" row must be a Scalar row
  with a labelled `ScalarFormat`. Both menus render through one function
  (`api::format_scalar_value` / `_utf8`).
- **Persistence**: `PersistMode` matrix (`api.rs`) has no local-only mode:
  `Full` (net save + net load + JSON), `SaveOnly`, `None`, `Session`. The JSON
  prime and the network load share ONE gate (`resolve_from_load` →
  `loaded_from_network()`); the JSON write uses `json_cached()`. Two call sites
  (`custom_options_persistence.rs` `apply_pending_loads` — network — and
  `json_load_once` — JSON prime). Adding a `Local` mode means splitting that
  gate by load source; the exhaustive-match design + `persist_matrix_tests.rs`
  make the addition mechanical.
- Existing `mod-config.json` caches already hold `bot_opponent` /
  `bot_opponent_level` (1..=10) under `custom_options.p1/p2` — keeping the
  option ids preserves them.

## 5. Other consumers to keep coherent

- Human's pacemaker compares the human against the same ghost ⇒ in Target mode
  the pacemaker delta and the 2P BPL frame's score margin describe the same
  opponent.
- `impersonation.rs` restore INFO logs the planned/judged tally — extend with
  the ghost provenance (len, grade histogram, reproduction misses).
- `is_bot_side` governance, extra-stage guard, score taint: unchanged (the bot
  side is still autoplay-tainted, never saved).
