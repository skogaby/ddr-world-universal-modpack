# Orientation — S-Marvelous server-side awareness (`s_marv` upload node)

Date: 2026-09-12. Sources: repo `src/` + `docs/`, the sibling `bemani-buddy`
checkout (its `crates/`, `migrations/`, `packet-logs/`, git history), the
sibling `bemaniutils` checkout, and a Ghidra decompile of gamemdx 20260825.

## 1. What exists today (DLL side)

**S-Marvelous is presentation-only.** `src/mods/s_marvelous/mod.rs:1-5`: the
engine's grade space is untouched — score/EX/gauge/combo/save/ghost see an
S-Marvelous as a Marvelous. The classification tap (`src/mods/power_user_statistics/data_feed.rs:315-329`)
is read-only on the judge event; nothing in `src/mods/s_marvelous/` references
the persistence service, `score_guard`, or any wire field.

**Every S-Marv-aware number the mod shows is already recomputed from the stage
record, not from live counters.** `src/mods/s_marvelous/records.rs`:

| Helper | What it yields | Notes |
|---|---|---|
| `read_streams(record)` (`:195-217`) | parallel `Vec<u8>` grade classes + `Vec<i16>` ms (`expected − actual`), **judged slots only** (`filter_judged`, `read_note_refs` `:411-444`); consistency gate: filtered Marvelous count == `rec+0x28` | slot streams live at `rec+0xB8` (grades, one byte per note) and `rec+0xD8` (ms) |
| `count_smarv(grades, ms, window)` (`:57-68`) | `g == 0 && \|ms\| <= window` | inclusive window |
| `count_marv_fast_slow` (`:98-119`) | loose Marvelous (`g == 0 && \|ms\| > window`) split FAST (`ms > 0`) / SLOW | invariant `smarv + fast + slow == marv` (host-tested) |
| `stock_fast_slow_from_record` (`:131-141`) | `rec+0x6C` / `rec+0x70` (grades 1..=4 only) | the stock `fastcount`/`slowcount` |
| `results_emblem::is_smfc` (`results_emblem.rs:238-…`) | `clear_kind(+0x54) == 10 && smarv == marv && marv > 0` | the S-MFC predicate, from the record |

Window provenance: `state::last_armed_window(side)` (`state.rs:126-128`), sticky
past disarm — valid through the results scene, which is when the save fires.
Record location for the stage just played: `stage_records::stage_record(side, stage_records::stage_counter())`
(the marshal runs in scene 30 BEFORE the scene-31 stage bump —
`docs/premium_free_stale_record_bug.md:70-74`). Under Premium Free the counter
is frozen at 0 and `record[0]` is overwritten each play — consistent with what
the game marshals.

**Save-packet injection surface** — `src/services/custom_options_persistence.rs`:

- ONE detour on ess `sys_playerdata_save_sender` (`:677-705`, resolved by log
  string, not AOB). Args `(job, kbin_ctx)`; `savedata = *(job+0x10)`,
  `playside = *(savedata+0x90)`, `savekind = *(savedata+0x74)` — 1 card-in
  checkpoint, **2 per-stage**, 3 logout (`:107-116`).
- The kbin tree does NOT exist before `original.call` (`:954`); all edits are
  post-original (`:956-1022`), in order: league strip → `<timing_music>`
  rewrite → s32 `mod_*` children → string fields → JSON cache. Everything
  current lands under `/data/option`; nothing touches `/data/result`.
- libavs ordinals (`:583-646`): 162 find child `(0, parent, name)`, 163
  add child `(ctx, parent, kbin_type, name, value)` — s32 (type 6) by value,
  str (type 11) by POINTER — 164 remove node, 175 get ctx (from the tree
  root), 176 read value. **No void-container create exists yet**: every
  current emission is a leaf. `<s_marv>` needs 163 with kbin type 1 (`void`,
  `src/services/avs_layeredfs/kbin/types.rs:14`) and no value — the same
  `property_node_create` ess uses for `<result>`/`<option>` — a small new
  helper, verify at deploy.
- There is NO `custom_options` container node in the profile save — the
  `mod_*` fields are flat children of `/data/option` (`:1222-1259`). The
  rough idea's analogy is to the *mechanism*, not to a container.
- `StringSaveFn`/`emit_string_fields` carry no `savekind` (`:162`, `:1136-1189`)
  — the registry emits on kinds 1/2/3 alike. The new producer must be a
  dedicated hook point that sees `savekind`, not a string-field registration.
- Suppressed stage saves (`score_guard::is_stage_suppressed`) never call the
  original (`:905-942`) — the node is naturally absent on tainted plays.

**Ghost encoding (Ghidra, gamemdx 20260825 `FUN_18001e1d0` =
`ark::network::GetGhostData`):** `cVar3 = *pcVar5 - 0x30` per character —
i.e. the wire ghost is ASCII `'0' + grade_class` per note, alphabet `'0'..'7'`
(0 M, 1 P, 2 Gr, 3 Gd, 4 Boo, 5 Miss, 6 OK, 7 NG). bemani-buddy packet logs
attest `'5'` only (an all-miss run). The stock `<ghost>` stays untouched, so a
designator outside `'0'..'7'` (maintainer: **`'S'`**) can never reach a stock
decoder.

**The marshal itself (Ghidra, gamemdx 20260825 `FUN_180018ee0` =
`ark::network::ReflectSavePlayerData(side, kind, stage)`), kind-2 branch:**

- Record = `work + 0x590 + stage*0x2B8`, **or the course record `work+0x2D8`
  when `work+0x4C == 10`** (course mode) — kind-2 saves DO fire in courses.
- `stagenum` = the `stage` argument (what `SavePlayerDataActor:%dP Stage%d`
  passes); `mcode = rec+0x00`, `style = rec+0x08`, `difficulty = rec+0x04`.
- `judge_marv..ng` = `rec+0x28,+0x2C,+0x30,+0x34,+0x3C,+0x40,+0x44` (the
  `+0x38` Boo slot is never sent); `fastcount/slowcount = rec+0x6C/+0x70`;
  `score/exscore/maxcombo = rec+0x10/+0x14/+0x20`; `rank = rec+0x50`;
  **wire `clearkind = rec+0x54`** — the same field the emblem predicate reads
  (10 = MFC — bemaniutils `GAME_HALO_MARVELOUS_COMBO = 10`, 9 PFC, 8 GFC,
  7 FC, 6 none). CORRECTION 2026-09-12: this pass first read the staging
  slot `+0x1B5C ← rec+0x270` as the clear kind; it is `folder` (wire order
  `playtime, stagenum, folder, mcode, style, difficulty, rank, clearkind`),
  and the first cabinet run shipped `s_marv/clearkind = 7` against a stock
  `clearkind = 10`. The packet log is the authority for staging→wire mapping.
- **Ghost = EVERY element of the `rec+0xB8..+0xC0` byte vector, each `+ '0'`;
  `ghostsize` = the vector length.** No judged filtering — on a partial play
  the unjudged tail is emitted as `'0'` (the grade-0 garbage
  `records.rs:179-186` filters for the results tab). An `s_marv/ghost` overlay
  must therefore index the UNFILTERED stream and substitute `'S'` only at
  slots that are judged AND grade 0 AND `|ms| <= window`.
- The result is staged into a per-side buffer (`+0x1B50..`, ghost cap
  `0x2004` chars) that ess `sys_playerdata_save_sender` serialises — which is
  why every current DLL edit is post-original on the kbin tree.

## 2. Backend side (bemani-buddy, `../bemani-buddy`)

- MySQL + sqlx 0.8; migrations `migrations/NNN_ddr_world_<topic>.sql`
  (001–018), run at startup.
- `playdata_3.playerdata_save` is a RAW handler
  (`crates/game-server/src/handlers/ddr_world/playdata.rs:567-605`): kind 2 →
  `handle_save_scores` then `handle_save_profile`; kind 3 → profile → Dan
  results (courses only) → league → play count.
- **`handle_save_scores` (`:1072-1180`) keeps only the `<result>` with the
  highest `stagenum`** and ignores the rest. bemaniutils does the same
  (`bemani/backend/ddr/ddrace.py:480-492`, "Find the highest stagenum
  played"). The game re-sends every stage of the session in each kind-2 save
  (packet logs: stagenums 0..3 repeated), so a per-stage `s_marv` for anything
  but the latest stage would be discarded anyway.
- `<result>` children read (`:1121-1143`): `mcode style difficulty score rank
  clearkind maxcombo flare_force exscore ghost judge_marv judge_perf judge_great
  judge_good judge_miss judge_ok judge_ng fastcount slowcount calorie
  gimmick_flag` (+ `stagenum` for selection). On the wire but unread:
  `playtime folder ghostsize bpm_* chara_* is_share select_*_sec` and the
  nested per-result `<option>`/`<filtersort>`.
- Parsing is fully lenient (`ri32 = parse().unwrap_or(0)`, `rstr =
  unwrap_or("")`); nested reads are plain chaining
  (`data.child("league").and_then(|l| l.child("current"))`, `:1004`).
  Optional fields follow the only-when-present pattern (`:704-729`).
- Storage: `ddr_world_scores` (`migrations/004_ddr_world_scores.sql:2-29`,
  `UNIQUE (profile_id, mcode, chart)`, `chart = style==0 ? diff : diff+5`) +
  the append-only mirror `ddr_world_score_attempts` (`:32-58`). Score DAO
  (`crates/db/src/mysql/ddr_world/score.rs`) uses RUNTIME `sqlx::query` with
  an explicit column list (`SELECT_SCORES` `:43-47`, `ScoreRow` `:168-194`,
  `row_to_score!` `:12-41`, INSERT `:58-69`, UPDATE `:78-89`, attempts INSERT
  `:148-160`) — no `.sqlx/` regeneration for score columns (unlike profile).
- Upsert = whole-row snapshot iff `points > old.points` (`score.rs:51-99`);
  ties never update.
- No JSON/blob column anywhere; no web UI / REST layer in this repo (only the
  e-amusement POST endpoints — `crates/game-server/src/server.rs:41-43`);
  score readers are `playerdata_load` CSV, `rivaldata_load`, `ghostdata_load`.
- Second writer: `crates/ddr-score-proxy/src/sql.rs:105-125` emits a 7-column
  raw INSERT — new score columns must be nullable.
- Template commit for "client-emitted field end to end": `2873823` (`mod_judge_offsets`,
  migration 016). NOTE the codegen trap: `models/ddr_world/playdata_3.json` is
  stale vs the hand-edited `crates/bemani-protocol/src/ddr_world/playdata_3.rs`
  (017/018 skipped the JSON). The raw save handler does not use codegen, so a
  `<data>` child needs no model edit — avoid the trap entirely.
- Nothing S-Marvelous exists in bemani-buddy (code, migrations, history).

## 3. What changes the idea

1. The mod already has every recompute helper the packet needs, keyed off the
   same record + window the results screen uses ⇒ packet == results screen by
   construction; no session tracking, no live counters.
2. A top-level `/data/s_marv` for the LATEST stage (maintainer directive)
   sidesteps `<result>` traversal entirely and matches the highest-stagenum
   rule both backends already apply.
3. The one genuinely new DLL primitive is creating a `void` container node.
4. The kind-2-only gate is new too (the current registry can't express it) —
   a dedicated post-original emission call in the trampoline, not a
   `register_string_field`.
5. Backend work is small and follows an established template; the raw-handler
   path avoids the stale-JSON codegen trap.

## 4. Song-select FC lamp (for the echo-back)

Ghidra, gamemdx 20260825, `FUN_18015a450` (the header-card refresh; two
xrefs to `fullcombo_%dp_usr` @ `0x180376648`), tail at `0x18015ba53..0x18015bcdc`:

```
clearkind = FUN_1800ff4a0(scoreTable, &songWeakPtr, sideIndex)   ; loaded-score lookup
has_lamp  = PTR_TABLE[0x1804a0a30 + clearkind*8] != 0           ; clearkind → "fc_mfc"/"fc_pfc"/…
setVisible(layer@this+0xD0, fmt("fullcombo_%dp_usr", side+1), has_lamp)     ; FUN_180257dd0
if has_lamp:
    setBitmap(layer, fmt("fullcombo_%dp_usr", side+1),
              fmt("muca_card_%s", PTR_TABLE[clearkind]))                    ; FUN_180257d80
```

- The lamp strings are the SAME `fc_mfc`/`fc_pfc` literals (`0x180369aa0`) the
  results badge table uses (`scre_total_player_fc_%s` in `results_emblem.rs`).
- **The table is indexed by the loaded wire clearkind with no bounds check.**
  A stock client handed `clearkind = 11` reads `[0x1804a0a30 + 0x58]` — one
  slot past the table — and formats whatever pointer lives there. This kills
  the "overwrite clearkind on load" shape for any server shared with stock or
  mod-off clients.
- DLL badge mechanism (design candidate): post-original detour on the card
  refresh, or a name-swap on the bitmap setter when `widget ==
  fullcombo_%dp_usr && texture == muca_card_fc_mfc` and the side's
  `(PlayerWork+0x54 mcode, PlayerWork+0x5C difficulty, GameWork+0 style)` is
  in the S-MFC set → `muca_card_fc_smfc` (net-new texture via `atlas_cloner`
  FRESH mode, donor `muca_card_fc_mfc`, art = violet recolor like the emblem
  set). Other lamp surfaces (wheel jackets, score popup) are a research item.

## 5. Unknowns

- Ordinal 163 with type 1 (void) + no value creating a container — deploy
  check (ess itself creates `<result>`/`<option>` this way).
- Whether `stage_records::stage_counter()` always equals the marshal's `stage`
  argument at save time (the trampoline's rate-ledger election already assumes
  it). Mitigation: echo `mcode/style/difficulty` in the node so the backend
  can detect a mismatch and ignore.
- Course/Dan kind-2 saves marshal the course record; v1 omits `s_marv` under
  the course gate (the backend treats courses only on kind 3 anyway).
