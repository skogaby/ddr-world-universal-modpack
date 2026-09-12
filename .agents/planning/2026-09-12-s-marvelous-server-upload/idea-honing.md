# Idea Honing — S-Marvelous server-side awareness (`s_marv` upload node)

Decision register. Ordered by blast radius (wire contract + data model first,
cosmetic last). Status ∈ Proposed / Accepted / Overridden / Assumed / Open.

| ID | Decision | Why it matters | Recommendation | Status |
|---|---|---|---|---|
| D1 | Packet placement | Wire contract; what both ends key on | **One `void` node `/data/s_marv`**, sibling of `<result>`, describing the LATEST stage (the record the kind-2 marshal just serialised — `record[stage_counter()]`, i.e. the highest-`stagenum` `<result>` both bemani-buddy and bemaniutils keep). No `<result>` traversal, no per-stage nodes. Stock nodes byte-identical | Accepted (maintainer 2026-09-12) |
| D2 | Save kinds | When the node appears | **`savekind == 2` only.** Kind 1 (card-in checkpoint) has sentinel results; kind 3 (logout) re-bundles stages the backend ignores for regular songs. Node absent on suppressed saves by construction (the original is never called) | Accepted (2026-09-12) |
| D3 | Node contents (the contract) | Everything downstream reads | Only fields whose value CHANGES with S-Marv awareness, plus identity + interpretation metadata: `mcode`, `style`, `difficulty` (s32 identity echo — backend cross-checks against the result it selected), `window_ms` (s32, the armed window), `judge_smarv` (s32), `judge_marv` (s32, **exclusive** — `stock − smarv`), `fastcount`/`slowcount` (s32, stock + loose-Marvelous share, matching the results tab), `clearkind` (s32, D5), `ghostsize` (s32) + `ghost` (str, D4). NOT duplicated: score, exscore, maxcombo, rank, flare_force, judge_perf/great/good/miss/ok/ng, calorie, gimmick_flag (unchanged by definition). Alternative considered: mirror the whole `<result>` — rejected as pure duplication the backend already has | Accepted (2026-09-12) |
| D4 | Ghost overlay | Web-UI step view | Same index space as the stock ghost (EVERY slot of the `rec+0xB8` vector, `'0'+grade`), with **`'8'`** substituted at slots that are judged ∧ grade 0 ∧ `\|ms\| <= window`. `ghostsize` = same length as stock. `'8'` = `'0'+8`, one past the stock alphabet `'0'..'7'`; a stock decoder never sees it because the stock `<ghost>` is untouched (and D19 keeps it that way on load) | Accepted (`'8'` — maintainer 2026-09-12, supersedes `'S'`) |
| D5 | S-MFC on the wire | New top lamp tier for the UI | `s_marv/clearkind` = stock wire clearkind (`rec+0x54`) **except `10 → 11` when S-MFC** (`clearkind == 10 ∧ smarv == marv ∧ marv > 0`, the same predicate as the results emblem, now on the wire field). 11 extends the stock enum (6 none, 7 FC, 8 GFC, 9 PFC, 10 MFC) with no collision. *You may not have considered this one* | Accepted (2026-09-12) |
| D6 | Source of truth | Correctness / results-screen parity | Recompute from the stage record streams + `state::last_armed_window(side)` — the exact inputs the results tab and emblem use — never live counters. Reuse `records.rs` helpers; add an UNFILTERED read + judged mask for the ghost overlay. Consistency gate (filtered Marvelous count == `rec+0x28`) must pass or the node is omitted | Assumed |
| D7 | Emission gates | No false data | Emit iff: mod enabled ∧ side was armed for this song (per-side latch set at play-scene arm, cleared on `disable`) ∧ `savekind == 2` ∧ not course mode (`stage_records` course gate; v1 omits courses) ∧ record `mcode != -1` ∧ streams consistent. Any gate failing ⇒ no node (stock-identical packet) | Assumed |
| D8 | Failure policy | Never hurt the save | Fail-open: never block, suppress, or reorder the save; never touch a stock node; one latched WARN per failure class (node create failed / record unreadable / streams inconsistent). A half-built node on a mid-way failure is removed via ordinal 164 (best effort) | Assumed |
| D9 | DLL hook shape | Layering; future reuse | Extend `custom_options_persistence` with a small **`/data` subtree producer registry**: `register_data_node_producer(name, fn(side, savekind) -> Option<NodeSpec>)`; `NodeSpec` = ordered typed leaves (`S32`/`Str`), built by a PURE host-testable function in the mod; the service owns the ordinals (new `void`-create helper via ordinal 163 type 1) and runs producers post-original after the string fields. Alternative: a direct `s_marvelous::upload::emit(...)` call from the trampoline (the `per_song_judgement_offsets::leaked_stock` precedent) — works, but bakes a mod name into a service | Accepted (2026-09-12) |
| D10 | Backend parse | Where the node lands | bemani-buddy `handle_save_scores` (kind 2 only): after selecting the highest-`stagenum` `<result>`, read `data.child("s_marv")` leniently (same `ri32`/`rstr` style), **cross-check `mcode` + `chart(style,difficulty)`** against the selected result — mismatch ⇒ log + ignore the node. Only-when-present; absent ⇒ `None`s. `handle_save_dan_results` untouched (D7) | Accepted (2026-09-12) |
| D11 | Backend storage | Schema | New nullable columns on BOTH `ddr_world_scores` and `ddr_world_score_attempts` (the attempt row mirrors every score field today): `smarv_window_ms INT`, `smarv_count INT`, `smarv_marvelous INT`, `smarv_fast INT`, `smarv_slow INT`, `smarv_clear_kind INT`, `smarv_ghost TEXT`, all `NULL DEFAULT NULL` (stock client / mod off / node ignored ⇒ NULL; the `ddr-score-proxy` raw INSERT keeps working). Alternative: a side table keyed on `ddr_world_scores.id` — rejected: the PB row must describe ONE play, and the existing whole-row snapshot upsert gives that for free | Accepted (2026-09-12) |
| D12 | Upsert semantics | Data coherence on the PB row | Ride the existing rule (whole-row replace iff `points >`): a new PB with no `s_marv` node **NULLs** the `smarv_*` columns (the row describes the new play; stale S-Marv data from an older PB would lie). Attempts always store what the save carried. No independent "best S-Marv count" tracking. *You may not have considered the NULL-on-PB case* | Accepted (2026-09-12) |
| D13 | Load-side echo: wire shape | Stock-client safety | The backend NEVER rewrites the stock score CSV (`score_str`'s `{halo}` stays the stock `clear_kind`). Instead `playerdata_load` adds a sibling node the stock client ignores: **`<smarv_scores __type="str">`** = `mcode:chart:clearkind\|…` for the profile's rows with `smarv_clear_kind IS NOT NULL AND smarv_clear_kind <> clear_kind` (in practice: the S-MFCs, `11`). Empty/absent when none. Rejected alternative (maintainer's first sketch): overwrite `clearkind` with the S-Marv value — the card-refresh lamp lookup indexes a pointer table by clearkind with NO bounds check (`[0x1804a0a30 + ck*8]`, orientation §4), so a stock or mod-off client receiving 11 reads past the table; and every stock consumer (MFC folder filter, sort-by-lamp) would stop treating an S-MFC as the MFC it also is | Accepted (2026-09-12; replaces the old D13 "no load/echo") |
| D14 | Load-side echo: DLL consumption | Badge correctness | The existing load-receiver trampoline reads `smarv_scores` (ordinal 176, type 11 — the `mod_judge_offsets` path, incl. the ddrcode-keyed deferral to SONG_SELECT entry) into a per-side `HashSet<(mcode, chart)>` of S-MFCs, cleared on card-in reset. Badge = post-original detour on the song-select header-card refresh (`FUN_18015a450` shape — the lamp block formats `muca_card_%s` from a clearkind→string table and calls the bitmap setter on `fullcombo_%dp_usr`): when the stock lamp resolved to `fc_mfc` and the side's current `(PlayerWork+0x54 mcode, +0x5C difficulty, style)` is in the set, re-set the bitmap to **`muca_card_fc_smfc`** — a net-new texture injected via `atlas_cloner` FRESH mode (donor `muca_card_fc_mfc`; violet recolor in the results-emblem art language). Fail-open: no set / no texture / signature miss ⇒ stock MFC lamp. Scope v1: the header card only; other lamp surfaces (wheel jackets, score popup, rival list) are noted for a later pass after RE | Accepted (2026-09-12) |
| D15 | Local S-MFC before the server round-trip | UX consistency | After an S-MFC the card should show the violet lamp immediately at the next song select, not after the next card-in. The per-side set is ALSO fed locally when the DLL emits an `s_marv` node with `clearkind == 11` for a side (same `(mcode, chart)` key). The server copy overrides on the next load | Accepted (2026-09-12) |
| D16 | Backend query layer | Implementation cost | Score DAO is runtime `sqlx::query` with explicit column lists ⇒ add the columns in `SELECT_SCORES`, `ScoreRow`, `row_to_score!`, INSERT, UPDATE, attempts INSERT; NO `.sqlx/` regeneration. The save side needs no codegen touch (raw handler). The LOAD side (D13) adds `smarv_scores: Option<String>` to the typed `PlayerdataLoadResponse` — which DOES go through codegen, hence D21 | Assumed |
| D17 | Config / UX | Operator surface | No new option row, no config key: emission and badge are implicit with the mod being enabled (fail-open). A kill switch would only matter for a server that rejects unknown nodes, which kbin servers don't do | Accepted (2026-09-12) |
| D18 | Diagnostics | Field debuggability | Save: one INFO per emitted node (`side stage mcode window smarv marv fast slow clearkind ghostlen`); one WARN per omitted-with-reason class. Load: one INFO per side with the S-MFC set size. Backend: `tracing::warn` on identity mismatch, `debug` on apply | Assumed |
| D19 | Tests | Validation without a cabinet | DLL: pure `build_s_marv_payload(grades, ms, judged, stock_counters, window)` host-tested (invariants `smarv + marv_excl == stock_marv`, `smarv + fast_share + slow_share == stock_marv`, ghost length == stream length, `'8'` only at qualifying judged slots, S-MFC 10→11) + pure `smarv_scores` codec round-trip. Backend: handler unit tests in the existing `playdata.rs` style (present / absent / mismatch / NULL-on-PB / load node present-empty-absent) + DAO round-trip | Assumed |
| D20 | Docs | Maintainability | Addendum to `docs/s_marvelous_judgement_research.md` (marshal + card-lamp facts from this pass), AGENTS.md S-Marv row note; bemani-buddy: migration header comment + AGENTS/summary note; fix the stale "play_count on savekind=1" doc line while there | Assumed |
| D21 | bemani-buddy codegen re-sync | Repo hygiene; D13 needs codegen | FIRST (own plan step, before any load-side change): add the two hand-edited fields (`mod_skip_results_fast_exit`, `mod_sync_movie`) to `models/ddr_world/playdata_3.json` in both `PlayerdataLoadOption` and `PlayerdataSaveOption`, regenerate `crates/bemani-protocol/src/ddr_world/playdata_3.rs`, and prove `git diff` on the generated file is empty (or formatting-only). Then D13's `smarv_scores` is added through the JSON → regenerate path, never by hand. Rule for AGENTS.md: generated files are never hand-edited | Accepted (maintainer 2026-09-12) |

## Decision notes

**D1 — placement.** Maintainer directive after weighing `<result>`-child vs
top-level: the game re-sends every stage of the session in each kind-2 save;
both reference backends keep only the highest `stagenum` and discard the rest,
so a per-`<result>` node would be discarded for all but the latest stage anyway.
A top-level node avoids tree traversal in the DLL and matches what the backend
actually applies. Premium Free's frozen stage counter (record[0] overwritten
each play) is consistent with this — the game marshals the same record.

**D2 — kind 2 only.** The existing string-field registry emits on every kind
because it carries no `savekind`; the new producer registry (D9) passes it, so
the gate is expressible. Kind 3 emission was considered for "logout carries
everything" symmetry and rejected: bemani-buddy ignores regular results on kind
3 and bemaniutils' DDR handlers save scores only on the per-song request.

**D3 — contents.** "Everything in a stock upload that needs recalculation with
S-Marv awareness" resolves, from the marshal decompile, to exactly:
`judge_marv` (loses the S-Marv share), `fastcount`/`slowcount` (gain the
loose-Marvelous share — the results-tab rule), `clearkind` (gains an S-MFC
tier) and `ghost` (per-step designators). Score/EX/rank/flare/other grades are
engine-invariant. Identity + `window_ms` are needed to interpret the rest.

**D4 — ghost overlay rule.** The marshal emits ALL slots of the grade vector
(not just judged ones), so the overlay must use the unfiltered stream for
positions and the judged mask for eligibility — otherwise an unjudged tail
slot (grade 0, ms 0) would read as S-Marvelous.

**D5 — S-MFC = 11.** The wire `clearkind` comes from `rec+0x54` — the very field
the emblem predicate reads (a first reading of the marshal placed it at
`rec+0x270`, which is `folder`; corrected 2026-09-12). MFC is 10 (bemaniutils
`GAME_HALO_MARVELOUS_COMBO = 10`), so the packet predicate can be stated on the
wire field itself. Alternative: a separate `is_smfc` bool — rejected; a
clearkind that extends the stock enum is what a lamp renderer wants.

**D9 — producer registry.** Two mods now want post-original tree edits keyed
on `(side, savekind)`; a `NodeSpec` keeps the mod side pure (host-tested) and
the ordinal handling in one place. The one new primitive — creating a `void`
container — is the same `property_node_create` ess uses for `<result>`.

**D11/D12 — columns + NULL-on-PB.** The stock values are already stored per
upload in the existing columns; the seven new ones are the S-Marv-aware
duplicates of exactly the fields that change: `smarv_window_ms` (interpretation
key), `smarv_count` (S-Marvelous), `smarv_marvelous` (exclusive = `marvelous −
smarv_count`), `smarv_fast`/`smarv_slow` (stock + loose-Marvelous share),
`smarv_clear_kind` (stock or 11), `smarv_ghost` (stock string with `'8'`).
Both sets land on the PB row and on every attempt row, so per-upload S-Marv and
non-S-Marv data are both kept. The NULL-on-PB rule falls out of the whole-row
snapshot: `NewDdrWorldScore` carries `Option`s, the UPDATE binds them, `None` ⇒
NULL.

**D13–D15 — echo-back.** The maintainer's goal (client badges an S-MFC from
server data) is met without changing any stock field's domain: the server adds
a node stock clients ignore, the DLL reads it through the load path it already
owns, and the badge is a texture swap on the one card widget whose stock lamp
resolved to MFC. Decompile of the header-card refresh shows the stock lamp
lookup is an unchecked pointer-table index by clearkind, which is why
"overwrite clearkind = 11" is rejected rather than merely disfavoured. The
pacemaker is unaffected either way (the stock ghost is served unchanged).
Rival-list lamps (`rivaldata_load`) are excluded from v1 — same table, would
need a second node and a second surface.

**D21 — codegen.** `models/ddr_world/playdata_3.json` is authoritative per the
bemani-buddy AGENTS.md; commits 017/018 hand-edited the generated `.rs`. The
re-sync is a no-behaviour-change step that must land first so the D13 load
field can be added the sanctioned way.

## Open items carried into research/design

- Deploy-time verification that ordinal 163 with kbin type 1 and no value
  creates an empty container (design: fall back to omitting the node + WARN).
- Card-refresh detour anchor: confirm the `fullcombo_%dp_usr` block shape is
  byte-stable across the four supported builds (`validate_signatures.sh` +
  `shape_diff.py`), and pick the detour point (function entry post-original vs
  the bitmap-setter call) in the design.
- `muca_card_fc_smfc` art: violet recolor of `muca_card_fc_mfc` (extract donor
  from the select-music arc at design time to size the cell).

## Readiness

**Readiness Confirmed 2026-09-12** — register accepted wholesale by the
maintainer ("Approved") after the D4/D13–D15/D21 revisions. No decision is
`Open`; the three items above are implementation-time verifications with
fail-open fallbacks, not design blockers. Research backing the design:
`research/orientation.md` (DLL save/load surface, marshal decompile, backend
survey, card-lamp decompile).
