# Idea Honing — 2-Player BPL Mode

Decision register. Status ∈ Proposed / Accepted / Overridden / Assumed / Open.
Ordered by blast radius (user-visible behaviour + interfaces first, cosmetic last).

| ID | Decision | Why it matters | Recommendation | Status |
|---|---|---|---|---|
| D1 | Scope of "the versus UI" for v1 | Determines whether this is one actor re-host or several | **Gameplay battle HUD only** (`MatchingBattleFrameActor`). Results-screen `battle_rank_usr` badge, total-results BPL header, `bgm_bpl`, battle announcer lines are separate `GameWork+0xD0`-gated features — each its own re-host; defer as phase 2 | Accepted |
| D2 | Mechanism | The whole implementation shape | **Approach A from the research doc**: construct the stock actor in the normal DPS with a mod-owned vtable clone (slot 4 wraps stock `onInitialize` with the `GameWork+0` flip; slot 6 = mod `onUpdate` feeding scores from the two `GamePlayActor`s + stock smoothing + stock rank fn). Zero detours / byte patches. B (re-implement on `dance_matching` assets) stays the documented fallback | Accepted |
| D3 | Session gate | What "2-player versus session" means to the code | Arm iff `GameWork+0 == 1` (versus) AND `side_entered(0) && side_entered(1)` AND `event_mode() ∉ {1,2}` (a real BPL session already has the frame) AND not course (`GameWork+course_field != 0`) AND scene == GAMEPLAY. Solo, doubles, courses, attract demo: inert. 2P TRAINING sessions ARE versus and get the HUD (scores are whatever the actors hold; pure display) | Accepted |
| D4 | Score type displayed | Gauge max + digits; must agree with the stock per-player readouts on screen | **Follow the cabinet**: pass the game's own `GamePlayActor+0x1D0` (cached `use_ex_score` result) — money score / 1,000,000 by default, EX / chart EX-max when the operator enabled EX scoring. Forcing EX like real BPL is rejected for v1: it would disagree with the stock readouts and needs proof that `+0x1D8` accumulates when EX mode is off | Accepted |
| D5 | Toggle surface + persistence | Menu shape, config keys | **Mod on/off only** (Mods tab toggle = `mods["two-player-bpl-mode"]`), cabinet-wide, default ON like every non-hardware mod. No custom option row, no per-player setting, no config section. Enable/disable applies at the NEXT song (creation happens once per play sequence); a mid-song disable leaves the current frame alone | Accepted |
| D6 | Mod identity | Ids are wire/config keys | id `two-player-bpl-mode`, display name `2-Player BPL Mode`, module `src/mods/two_player_bpl_mode/` (`mod.rs` + pure `logic.rs`; amended from a single file during design so the gate/name/smoothing arithmetic is host-testable — the repo's `validate_*.sh` harness pattern) | Accepted |
| D7 | Player identity on the boards | What the name/base art shows | Names from `PlayerWork` via the game's own getName semantics (empty ⇒ `PLAYER1`/`PLAYER2`, exactly like the stock HUD for guests); `ddrcode` = `PlayerWork+0x18` or −1 for guests (only logged by the ctor); `team_id = 0` (stock `dama_score_base_{1,2}p` art, never BPL team art). Board position 0 ← side 0 (P1 left), 1 ← side 1 | Accepted |
| D8 | Stock per-player score readouts | Screen clutter vs. fidelity | **Keep them** — stock BPL play shows both the frame and the per-player readouts. Hiding is a possible later cosmetic step | Accepted |
| D9 | Creation trigger + latch | Correctness across restarts | Per-frame poll (`input_manager::on_frame`) armed by the GAMEPLAY scene callback, disarmed on exit. Create once per DPS INSTANCE (latch = DPS pointer): a quick-restart `finish` builds a fresh DPS ⇒ new frame; a `song_reset` in-place restart keeps the DPS ⇒ the existing frame stays (its snap-down smoothing absorbs the score reset). Wait for DPS step ≥ 2 (children created, LayoutActor built) before constructing | Assumed |
| D10 | Failure policy | In-process hook DLL: every miss must degrade, never crash | Fail-open throughout: any missing signature/derivation ⇒ mod skipped (`required_signatures`); slot-4 wrapper pre-checks the `dance_matching` package slot (31) is resident and, if not, skips the stock call and marks the actor finished (stock would NULL-deref); every game-object read range-checked; hook bodies `catch_unwind`; one latched WARN per failure class | Assumed |
| D11 | Service change | Shared code touched | Promote `song_reset::live_dps` / `gameplay_actors` / `read_step` (+ the DPS step constants) to `pub(crate)` instead of a third private copy; everything else new lives in the mod file + `signatures.rs` | Assumed |
| D12 | Cross-build validation bar | Project rule | All new AOBs/derivations must hit exactly once on all four builds (`validate_signatures.sh`) and `shape_diff.py` must be run for every `match+N` read (notably the `+0x1D0` isEx offset attestation) before the first cabinet deploy | Assumed |
| D13 | HUD overlap with other mod-owned widgets | Visual only; cabinet-observable | No repositioning in v1. Verify on cabinet against power_user_statistics widgets (P1 x=80 / P2 x=1200, y=425) and the training strip; if they collide, the PUS `widget_offset_*` config already lets the operator move those | Assumed |

## Notes per decision

**D1** — The user's framing is "track the current leader during gameplay", which the HUD
delivers fully. The results/total-results pieces read record fields (`rec+0x5E0/+0x5E4`)
the matching DPS fills from the network stage-result record, i.e. they need their own
data-feed re-host, not a flag. Cleanly separable.

**D2** — The doc's §3 rejects C (spoofing the matching mode drags in the network session,
EX rules and the 0xD0-keyed results/logout chain) and prices B at ~10× A. A's only
non-obvious risk is the `GameWork+0` flip inside `onInitialize` — research item R1.

**D3** — `GameWork+0` is written at mode commit (`SelectStyleSequence` case 1) as
`(2 players) ? 1 : 0`, so it is exactly the versus predicate. Courses are excluded in
v1 because the frame's mcode/diff come from the per-stage record header, which the
course record does not share; a versus course is a plausible follow-up.

**D4** — In real BPL `FUN_1801ea320` returns 1 unconditionally; locally it returns the
operator option, and `GamePlayActor+0x1D0` caches it per song. Passing that flag through
means the frame always agrees with what the players see on their own score readouts.

**D7** — Research item R2: whether `PlayerWork+0x0C` is an inline `char[]` or an MSVC
`std::string`; R3: whether `main_single` has a fixed left/right sidedness.

**D9** — `song_reset` restarts are in-place; the actor's `max_score` is per-song and
`display` eases toward `target` (snaps down), so nothing needs resetting.

## Register accepted

Accepted 2026-09-09 (D1–D8 accepted as recommended, D9–D13 assumed, no overrides).

## Readiness Confirmed 2026-09-09

Register accepted (D1–D8), assumptions D9–D13 stand, no Open decisions. Research:
`research/orientation.md` (codebase) + `research/re-findings.md` (Ghidra R1–R7). R5
corrected the source doc's "null network blocks are harmless" claim — the design adds a
fail-closed `local_idx == -1` pre-check; no accepted decision was invalidated.
