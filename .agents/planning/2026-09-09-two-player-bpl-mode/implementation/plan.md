# 2-Player BPL Mode — Implementation Plan

Status: Approved 2026-09-09

Design: `design/detailed-design.md`. Register: `idea-honing.md`. RE: `research/re-findings.md`.
Project rules that apply to every step: no `git commit` unless the maintainer asks;
readiness gates before any deploy = `cargo check` → `cargo fmt` (whole crate) →
`./build.sh` → `validate_signatures.sh` green when `signatures.rs` changed.

- [x] Step 1: Signatures, derivations, service promotion — offline four-build sweep green
- [x] Step 2: Pure logic module + host validation harness
- [x] Step 3: Mod skeleton with gating, tree walk and DRY-RUN placement log (cabinet checkpoint)
- [x] Step 4: Vtable clone, slot wrappers, real frame creation (cabinet: HUD visible)
- [x] Step 5: Cabinet validation matrix, docs (AGENTS.md row, RE doc addendum, README) — docs done; core matrix (2P versus HUD, fresh-DPS recreation, quick-fail teardown) cabinet-validated 2026-09-10; EX-scoring / in-place-restart / overlap checks remain optional follow-ups

---

## Step 1: Signatures, derivations, service promotion

**Objective.** Everything the mod needs from the binary resolves on all four supported
builds before any mod code exists — this is the step most likely to invalidate the
design, so it goes first.

**Implementation guidance.**
- `src/core/signatures.rs`: add `SignatureDefinition`s for `battle_frame_ctor`,
  `actor_add_child`, `dance_matching_slot_probe`, `gpa_score_select` (byte shapes: design
  Appendix B; doc-comment each with the disassembly and the consumer's `match+N` reads).
  Add the two RTTI lookups (`battle_frame_actor_vtable`, `layout_actor_vtable`) next to
  `find_gauge_vtables`. Add `derive_two_player_bpl()` to `resolve_derived`: rank fn from
  the stock `vtable[6]` tail `JMP rel32` (window scan, inside-module check);
  `matching_local_cabinet_idx` from the first `48 63 05` in the ctor body;
  `publish_value` for `dance_matching_slot_off`, `gpa_is_ex_off`, `gpa_ex_score_off`,
  `gpa_money_score_off`; cross-checks (ctor's 2nd `LEA RAX,[rip]` == RTTI vftable; probe
  inside `[vtable[4], +0x200)`), each failure a `[-]` WARN line.
- `src/services/song_reset/mod.rs`: `pub(crate)` on `live_dps`, `gameplay_actors`,
  `read_step`, `FIRST_CHILD_OFFSET`, `NEXT_SIBLING_OFFSET`, `GPA_SIDE_OFFSET`. No
  behaviour change.
- Do NOT hardcode `0x7F0` / `0x1D0` / `0x1D4` / `0x1D8` anywhere outside the
  signature doc-comments.

**Tests.** `./scripts/validate_signatures.sh ~/Desktop/ddr_modules` — all four new AOBs
hit exactly once per build, both RTTI vtables resolve, every `(derived)` line present,
exit 0. `scripts/sig_harness/shape_diff.py --json … --dir …` reports no divergence within
the read windows of `battle_frame_ctor` (+0x78 LEA), `dance_matching_slot_probe` (+13),
`gpa_score_select` (+2/+11/+19) and the `vtable[6]` tail. `cargo check` clean.

**Integration.** Additive to the store; nothing consumes the new keys yet (the harness
attributes them once Step 3's `required_signatures` literal exists — run the sweep again
then).

**Demo.** Boot log (or the offline sweep output) shows `[+] battle_frame_ctor @ …`,
`[+] battle_frame_rank_fn (derived) @ …`, `[+] gpa_is_ex_off (derived) = 0x1D0` … on
every build, with the derived values equal across builds.

## Step 2: Pure logic module + host harness

**Objective.** The decision/arithmetic layer exists and is proven on the host before it
touches game memory.

**Implementation guidance.** `src/mods/two_player_bpl_mode/logic.rs` (dependency-free):
`GateInputs`/`Gate`/`eligibility`, `player_name`, `smooth`, `gauge_fraction`,
`clone_vtable_image` exactly as specified in the design's "Pure logic" section.
`src/mods/two_player_bpl_mode/mod.rs` = a stub that only declares `pub mod logic;` so the
crate compiles; `pub mod two_player_bpl_mode;` in `src/mods/mod.rs`.
`scripts/validate_two_player_bpl.sh`: copy `scripts/validate_training_mode.sh`, mount
`logic.rs`.

**Tests** (in `logic.rs` `#[cfg(test)]`): eligibility truth table incl. every `None` ⇒
`Unavailable` and each single-gate failure ⇒ `Ineligible(reason)`; `player_name` (entered +
empty ⇒ `PLAYER1`/`PLAYER2`; ≤ 8-byte copy; unterminated input; not-entered + empty ⇒
empty); `smooth` (ease-up halving sequence, snap-down, fixed point, `i32::MAX`
neighbourhood — use `i64` intermediate); `gauge_fraction` (max 0 ⇒ 0.0);
`clone_vtable_image` (index 0 = COL, slots copied, only 4 and 6 overridden).

**Integration.** Step 1's crate + a new module; `cargo check` for the DLL target still
clean.

**Demo.** `./scripts/validate_two_player_bpl.sh` runs the suite green on the host.

## Step 3: Mod skeleton with gating, tree walk and DRY-RUN placement

**Objective.** The whole runtime path except the allocation/construction/attach runs on
the cabinet and proves the timing (when the DPS, LayoutActor and both GamePlayActors are
present), the gates, and the identity inputs — with zero writes to game memory.

**Implementation guidance.** In `mod.rs`: `TwoPlayerBplMod` (`id`, `name`, `description`,
`required_signatures` listing every key from Step 1 + `agcs_heap_malloc`,
`app_heap_handle`, `gameplay_actor_vtable`), `init` (resolve into the `Sites` `OnceLock`,
cross-checks, `resolved = true`), `enable`/`disable` (scene + frame callbacks, `ENABLED`),
`is_active`. `create_frame()` steps 1–5 from the design (gates via `logic::eligibility`,
`song_reset::live_dps` + child walk classifying LayoutActor / GamePlayActors / existing
frame, network-idle check, package-resident check, mcode/diff/is_ex/names) returning
`NotReady | Refused(reason) | Ready(inputs)`; on `Ready` log ONE INFO
`2P BPL (dry run): would place frame dps=%p layout=%p gpa=[%p,%p] is_ex=%d mcode=%d diff=%d names=[%s,%s] pkg=%p`
and latch `DONE_FOR_DPS`. Register in `src/lib.rs`. Build the vtable clone in `init`
already (it is inert until Step 4 installs it) so its allocation path is exercised.

**Tests.** `cargo check`; `validate_signatures.sh` re-run (the `required_signatures`
literal now attributes the new keys to `two-player-bpl-mode` in `report.py`'s consumer
graph — confirm no HARD loss). Deploy: solo, doubles, and 2P versus plays.

**Integration.** Consumes Step 1's keys and the `song_reset` promotions; calls Step 2's
pure functions. The mod appears in the Mods tab as `[ON]`.

**Demo.** Cabinet log: solo/doubles ⇒ one INFO gate-refusal line, no WARN; 2P versus ⇒
the dry-run line within the first frames of GAMEPLAY with plausible names/mcode/diff and
a non-null package pointer; nothing on screen changes.

## Step 4: Vtable clone install, slot wrappers, real frame creation

**Objective.** The battle HUD renders in local 2P versus.

**Implementation guidance.** `create_frame()` steps 6–9 (allocate 0x290 via
`agcs_heap_malloc(*heap_handle, …)`, stock ctor with `actors = frame + 0x280`, install
`CLONE_VTABLE`, `+0x1B0 = 2`, fill `BATTLE_INFO[0..1]` and neutralise `[2..3]`,
`add_child` + parent-field verification, `DONE_FOR_DPS`, INFO line). Slot 4
`on_initialize_wrapper` (package re-check → neutralise `+0x50 &= !3` on miss; scope-guarded
`GameWork+0` flip around the STOCK slot 4) and slot 6 `on_update_replacement` (per-actor
identity check → target → `logic::smooth` / `gauge_fraction` → stock rank fn), both
`extern "C"`, `catch_unwind`-wrapped, no `unwrap`/indexing. Remove the dry-run line.

**Tests.** `cargo check` + `./build.sh`; host suite from Step 2 still green (the wrappers
call into it). Deploy and run design "Cabinet" items 1–5 and 7.

**Integration.** Completes the runtime path of Step 3; the clone built in Step 3's
`init` is now installed into the instance.

**Demo.** 2P versus: frame visible from the READY panel; names/`PLAYER1/2`; gauges fill;
1st/2nd badges swap with the lead; margin readout equals the stock readouts' difference;
in-place restart snaps boards to 0; quick restart re-creates; song end / quick fail /
results tear down with no WARN. Solo/doubles/course unchanged.

## Step 5: Cabinet validation matrix + documentation

**Objective.** Close the loop: the visual checks only the cabinet can answer, and the
project's documentation contract.

**Implementation guidance.**
- Run the remaining cabinet items: EX-scoring ON (design item 3), overlap with
  power_user_statistics widgets / training strip (item 6); record findings in
  `progress.md`. If overlap is objectionable, note the PUS `widget_offset_*` remedy in
  README rather than adding code (R-OVERLAP).
- `AGENTS.md`: add the Key Entry Points row for `2-Player BPL Mode` (mechanism,
  signatures, gates, fail-open table, the `local_idx == -1` dependency).
- `docs/in_shop_battle_local_versus_research.md`: append an implementation addendum
  correcting §3's "null blocks are harmless" (Appendix A of the design), recording the
  final signature set and the vtable-clone details; move the doc from "feasibility" to
  "implemented".
- `README.md`: operator-facing paragraph (what it is, default ON, where the toggle is).
- Any code fixes the cabinet run demands, each re-validated by Steps 1–2's gates.

**Tests.** Full readiness gate: `cargo check` → `cargo fmt` → `./build.sh` →
`validate_signatures.sh` green → `validate_two_player_bpl.sh` green. Cabinet matrix
complete (design "Cabinet" 1–7) with no WARN in a healthy 2P session.

**Integration.** Documentation only plus fixes; no new surface.

**Demo.** A clean 2P versus session log (no WARN), the documented row in AGENTS.md, and
the updated research doc.
