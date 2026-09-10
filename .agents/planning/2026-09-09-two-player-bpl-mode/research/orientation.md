# Orientation — 2-Player BPL Mode

Date: 2026-09-09. Sources: `docs/in_shop_battle_local_versus_research.md` (display-side
RE + re-hosting plan, the authoritative starting point), `docs/bpl_battle_mode_research.md`
(network stack — establishes WHY the stock mode is unreachable locally), and a codebase
sweep of the services the mod will reuse.

## 1. What the idea actually is, once grounded

"Force enable the in-shop matching versus UI in local 2P versus" decomposes into the
gameplay battle HUD (per-player score boards, score-ratio gauges, live 1st/2nd rank
badges, score-margin readout) — ONE self-contained game actor,
`sequence::dance::MatchingBattleFrameActor` (0x280 bytes), which the matching play
sequence creates as a sibling of the two `GamePlayActor`s. Its art (`dance_matching`) is
already resident in normal gameplay (loader mask 0x8000 & 0x9000) and its screen anchor
(`matching_usr` in `dance_root`) is registered by the shared `LayoutActor` builder in
both play-sequence variants. Only its DATA source is network-bound (it reads scores from
`CNetworkManager` cabinet blocks, null in local play).

The research doc already evaluated three approaches and recommends **A — re-host the
stock actor with a mod-owned vtable clone** (slot 4 `onInitialize` wrapper flips
`GameWork+0` to 0 around the stock call so it picks the 2-player `main_single` layout;
slot 6 `onUpdate` replaced by a ~15-line re-implementation that feeds
`BATTLE_INFO[i].score_target` from `GamePlayActor+0x1D0/1D4/1D8`, then the stock
smoothing + stock rank fn). Zero detours, zero byte patches. Spoofing the matching mode
(C) is rejected (blocks on network state 4, forces EX rules, re-routes results/logout).

The OTHER "in-shop battle" UI pieces (matching song-select panels, `in_battle` /
`battle_rank_usr` results badge, total-results BPL rank header, `bgm_bpl`, battle
announcer lines) are all gated on `GameWork+0xD0 ∈ {1,2}` = the event-mode selector
that the whole results/logout chain keys on. They are NOT part of the HUD actor and
each would need its own re-host. The research doc lists the results rank badge as a
phase-2 candidate only.

## 2. Codebase findings that shape the design

| Finding | Consequence |
|---|---|
| `song_reset::live_dps()` and `gameplay_actors(dps)` are PRIVATE (`src/services/song_reset/mod.rs:1272`, `:1288`); `quick_restart_or_fail.rs:814` carries a private duplicate | Promote to `pub(crate)` (tiny service change) rather than a third copy. Child walk: first child `+0x18`, next sibling `+0x10`, parent `+0x08`, flags `+0x20` (dead mask `0x24`), vftable identity = pointer compare at `+0x00` |
| Scene callbacks fire INSIDE the `createNextSequence` detour, before the original runs — at `next == GAMEPLAY` the DPS does not exist yet | Creation must be driven from a per-frame poll (`input_manager::on_frame`, `src/services/input_manager.rs:1254`), armed by the scene callback, disarmed on scene exit. `song_reset::read_step` (private, `:1375`) reads the DPS StackStep (`base 0x68 / index 0x92`; in-song = 7) — needed to wait for "DPS past case 1" (= LayoutActor built + GamePlayActors created) |
| Vtable-clone precedent exists: `src/services/custom_options/rows.rs::build_mod_vtable` (`:1071`) — `memory::alloc_zeroed((N+1)*8)`, copies the donor's RTTI COL into slot −1, copies slots, overrides some, returns `raw+8`; object gets `write_ptr(obj, vtable)` after the donor ctor | Reuse the exact shape. The frame actor's vtable has 9 slots (0..8). `agcs` `__RTDynamicCast` reads `[-1]`, so the COL copy is load-bearing |
| Heap: `agcs_heap_malloc(handle, size, 0, 0)` / `agcs_heap_free` / derived `app_heap_handle` (a `*const *const u8` global, deref at use) — all existing signatures; consumer shape in `note_types_expansion/hooks.rs:258` | Allocation matches the actor's deleting dtor (`flag&1 → agcs_heap_free`) by construction |
| RTTI vtable lookup: `SignatureStore::find_vtable_by_rtti(".?AV…@@", label)` (`src/core/signatures.rs:5897`); `find_gauge_vtables` (`:5161`) is the loop template | `MatchingBattleFrameActor` and `LayoutActor` vftables come from RTTI, no AOB needed for them. Stock slots 4/5/6/7 + the rank fn are then READ off the resolved vftable (slot 6's tail `JMP rel32`) |
| No `Actor::addChild` signature exists | New AOB (research §3 table). Alternative worth checking in Ghidra: whether the DPS's own child-append is reachable via a vslot instead |
| `GamePlayActor+0x1D4/+0x1D8` already named in `song_reset` (`GPA_SCORE_OFFSET`/`GPA_EX_SCORE_OFFSET`, ≤ `+0x1E9` ⇒ build-stable); `+0x1D0` (isEx) is referenced nowhere in src | New offset; attest with `shape_diff.py` on the matching-DPS case-0xB read as the research doc asks |
| `stage_records` exposes `game_work()`, `side_entered`, `event_mode()`, `stage_counter()`, `stage_record(side, stage)`, `course_field_offset()`; record header offsets `+0x0` mcode / `+0x4` diff are hardcoded by consumers (`premium_free/ghost_cache.rs:59`) | Gate + mcode/diff all available. Course detection idiom: `read_u64(game_work + course_field_offset()) != 0` |
| `PlayerWork+0x18` ddrcode read exists (`custom_options_persistence.rs:129`); `+0x0C` name is read NOWHERE — whether it is an inline `char[]` or an MSVC `std::string` is not pinned by code | Research item (Ghidra: `FUN_1801e88a0` getName + the record builder `FUN_1800a7410` case 1) |
| `Mod` trait: `is_active()` = "CAN work"; registry skips the mod when `required_signatures` miss; Mods-tab toggle is automatic for every registered mod | No option row needed for an on/off cabinet-wide mod. Template: `anytime_speedmod.rs` (init/signature shape) + `classic_difficulty.rs` (callback lifecycle) |
| Signature harness (`scripts/sig_harness/report.py`) auto-discovers literal `required_signatures`/`get_address`/`require_address` names and `[+] … (derived)` log lines | Just add entries; run `validate_signatures.sh` + `shape_diff.py` before deploy |
| A Ghidra instance (`DDRWorld_Ghidra`, `gamemdx_20260825.dll` open) is live | The doc's cabinet-verification items #2 (callee `GameWork+0` readers) and #3/#4 (name field, board sidedness) can be settled in research instead of on the cabinet |

## 3. Unknowns going into the register

1. Scope: HUD only vs. HUD + results-screen rank badge (phase 2 in the doc).
2. Score type shown: follow the cabinet's `use_ex_score` (what the stock per-player
   readouts show) vs. force EX like real BPL — forcing requires knowing whether
   `GamePlayActor+0x1D8` accumulates when EX mode is off.
3. Whether `onInitialize`'s callee set reads `GameWork+0` (the flip's only risk).
4. `PlayerWork+0x0C` name representation; guest display (stock `PLAYER1/2` fallback vs. blank).
5. Sidedness of `main_single` boards (is board 1 always P1/left?).
6. Course/Dan and 2P training sessions: include or stay inert.
7. Screen overlap with power_user_statistics widgets / training strip at the
   `matching_usr` anchor — cabinet-only.

## 4. Proposed sequence

Register first (most decisions are scope/behaviour, not RE), then a short Ghidra
research pass on items 3–5 (+ the `addChild` derivation), then design + plan. Light
pass: single-file mod, ~5 new signatures/derivations, one `pub(crate)` promotion in
`song_reset`.
