# Task: Interaction pass (phantom-player governance), D22 SE pan, RE note, AGENTS.md row

## Description

Close plan Step 6 (design §7.3 items 9–10, register R11 / N6 / D14 / D15 / D22 / D23): review
every cross-mod interaction the design accepted as-is, fix the ones that alter the HUMAN's play,
document the merely cosmetic ones, take the optional SE-pan cosmetic (D22) now that
`audio_manager_global` is confirmed derived, and leave durable documentation — a consolidated RE
note and an AGENTS.md Key Entry Points row.

## Background

The Step 4 flip makes BOTH sides read `stage_records::side_entered == Some(true)` during the
play window, but `versus_mirror` never engages (it only engages at SONG_SELECT with both sides
entered — the flip lands after leaving scene 25 and `scene_manager` updates `current_scene`
BEFORE the callbacks fire). Several cabinet-wide policies were written for real versus where the
mirror guarantees both sides' rows agree: "P1 governs when both entered". In a bot session with
the human on **P2** those policies read the BOT side's rows — per-side option values that outlive
the player (the JSON cache of whoever last used P1). Audit (2026-09-13, code review):

| Consumer | Policy | Effect with human = P2 | Class |
|---|---|---|---|
| `premium_free::effective_freeze` | `(Some(true), _) ⇒ p1`, re-resolved on EVERY scene change | the stage-bump NOP state during scene 31 follows P1's stale cache — the human loses or gains a free stage | breaks play |
| `training_mode::pre_shift_side` | both entered ⇒ side 0 | a stale P1 LOOP SONG + SONG START cache pre-shifts the human's song start | breaks play |
| `training_mode::bounds::try_resolve_row_bounds` | first side whose actor resolves (= 0) | a stale P1 LOOP SONG cache latches a loop (taint + death bypass) the human did not ask for | breaks play |
| `timing_offsets::calibration` census | exactly one entered side | refuses as "2P" — auto-calibration unavailable in bot songs | feature loss |
| `assist_tick` GAMEPLAY latch | per-side enables, 2 enabled ⇒ side 0 | a stale bot-side `assist_tick = ON` plays a clap track the human did not turn on | audible |
| `announcer_mute::effective_mute` | P1 wins | the bot side's cached mute governs | audible |
| `training_mode::strip_hud::latch_placement` | P1 | the timeline strip uses P1's placement | cosmetic — document only |
| `song_rate` scene-26 classifier | service callback registered BEFORE the mods' | fires before the flip on the same edge ⇒ sees one entered side (the human) ⇒ correct | OK |
| `versus_mirror` | engages only at scene 25 with both entered | never engages in the window; the restore clears `+0x4` before scene 25 returns | OK |
| `two_player_bpl_mode` | `GameWork+0 == 1` ∧ both entered ∧ … | engages against the bot with the bot's name (D9 intended) | OK — cabinet §7.3 item 9 |
| `autoplay` on the bot side | `foot_panel_swap` Bot > Perfect; watermark asks `controller == Perfect` | precedence INFO, bot drives, no watermark on the bot side (N6) | OK |
| `per_song_judgement_offsets` | writes bot `Option+0x24` at first dispatch | harmless — `event = mc − (mc − E) = E` is offset-agnostic (bot-controller RE §4) | OK |
| PUS / S-Marvelous | treat the bot as an entered side | a stats widget / violet paint for the bot's pane | cosmetic — document |
| quick restart / quick fail | 28→27→28 / →24 | `session::classify` Reseed / Restore (host-tested) | OK — cabinet §7.3 item 5 |

When the human is on P1 none of the "P1 governs" rows misbehave (P1 IS the human). The fix is
one shared predicate and one-line exclusions at the five sites.

D22: the versus SE pan byte is `*(audio_manager + 0x20C4)` — `FUN_1801aa500` is
`MOV RAX,[rip+audio_manager_global]; MOV byte [RAX+0x20C4],BL`, byte-identical (disp32 `0x20C4`)
on all four builds; `audio_manager_global` is already derived (`derive_audio_manager_and_play`,
`+0x6F2D68` on 20260825) and `game_audio` holds it. Stock writes the byte from
`createNextSequence` cases 0x16/0x24/0x2e/0x3b = `GameWork+0 == 1`; readers pan side 0/1 SEs
left/right when set. Trivial ⇒ take it.

## Reference Documentation

**Required:**
- Design: `.agents/planning/2026-09-13-multiplayer-bot/design/detailed-design.md` — §7.3 (items
  9–10), §2.3 (pan byte assumption), Appendix A (the facts the RE note consolidates), Appendix B.

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-13-multiplayer-bot/research/{versus-impersonation-re.md,
  bot-controller-re.md, autoplay-internals.md, side-entry-model.md}` (sources for the RE note).
- `docs/gauge_and_judge_scoring_research.md` (pointed at, not duplicated).
- `src/mods/premium_free/mod.rs` (`effective_freeze`), `src/mods/training_mode/{mod.rs,bounds.rs}`
  (`pre_shift_side`, `try_resolve_row_bounds`), `src/mods/timing_offsets/calibration.rs` (census),
  `src/mods/assist_tick.rs` ~482–499 (GAMEPLAY latch), `src/mods/announcer_mute.rs`
  (`effective_mute`), `src/services/game_audio.rs` (`MANAGER_GLOBAL_ADDR` / `sound_bank_in_slot`
  probe shape).
- `AGENTS.md` Key Entry Points table (row style: "2-Player BPL Mode", "Premium Free").
- `.agents/learnings/learnings.md` "Per-side option values OUTLIVE the player".

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. **Shared predicate** — `src/mods/multiplayer_bot/mod.rs`: `pub fn is_bot_side(side: usize) ->
   bool` = `impersonation::active_bot_side() == Some(side)` (lock-free). Document it as the
   "phantom player" query for cabinet-wide policies that fold both sides' option values.
2. **Exclusions** (each a minimal edit + a one-line comment naming the reason):
   - `premium_free::effective_freeze`: treat a bot side as `Some(false)`.
   - `training_mode::pre_shift_side`: same.
   - `training_mode::bounds::try_resolve_row_bounds`: skip the bot side in the governing-side
     `find_map`.
   - `timing_offsets::calibration`: census inputs treat the bot side as `Some(false)`.
   - `assist_tick` GAMEPLAY latch: `enabled = ASSIST_TICK_ENABLED[side] && !is_bot_side(side)`.
   - `announcer_mute::effective_mute`: treat a bot side as `Some(false)`.
3. **D22 SE pan** — `src/services/game_audio.rs`: `pub fn set_versus_pan(on: bool) -> bool`
   (probe the manager global + object, write the byte at `+0x20C4`, `false` when unavailable) and
   `pub fn versus_pan() -> Option<bool>`; `impersonation::apply` snapshots + writes 1 after the
   versus word (probed; failure is NOT a refusal — log once, continue), `restore()`/`undo` write the
   snapshot back. `Snapshot` gains `pan_byte: Option<u8>`.
4. **RE note** — `docs/multiplayer_bot_research.md`: consolidate Appendix A.1–A.7 + the durable
   facts of the two research notes (actor-count seam, versus-word readers incl. the pan byte,
   song-select commit + record mirroring, extra-stage grant, `AutoFootPanel` vtable + judge algebra
   + no-Boo, `Option` layout, cardless entered side, the "phantom player governance" audit table
   above) — addresses file-relative to `0x180000000`, build 20260825 unless stated, no local
   paths; point at `docs/gauge_and_judge_scoring_research.md` for the judge/gauge/scoring
   transcription.
5. **AGENTS.md** — one Key Entry Points row "Multiplayer Bot (…)" in the established style: mod
   path + id, rows + wire names, `services/foot_panel_swap` (Controller precedence, the judge slot
   ownership pre Late / post Early), the two RE facts, no-Boo, the planner's rules, fail-open
   behaviour, `is_bot_side` governance rule, `tools/bot_sim` / `scripts/bot_sim.sh` /
   `scripts/validate_multiplayer_bot.sh`, the RE doc. Update rows that describe autoplay's swap as
   living in `autoplay.rs` ("Judge hook subscribers" if it names autoplay) to name the service.
6. Final gates: `cargo check` → `cargo fmt` (both crates) → `./build.sh` →
   `./scripts/validate_multiplayer_bot.sh` → `./scripts/validate_signatures.sh ~/Desktop/ddr_modules`
   → `git grep -nE "/(Users|home)/[^/ ]+/" -- . ':!target'` adds no new hits.

## Dependencies

- Steps 1–5 in tree; `game_audio::MANAGER_GLOBAL_ADDR` (private — add the two pub fns beside
  `sound_bank_in_slot`).

## Implementation Approach

1. `is_bot_side` + the six exclusions → `cargo check`.
2. `game_audio::{set_versus_pan, versus_pan}` + impersonation wiring → `cargo check`.
3. RE note, AGENTS.md row.
4. Gates.

## Acceptance Criteria

1. **Human on P2 keeps their own settings** — Given a bot session with the human on P2, When premium
   free / training / calibration / assist tick / announcer mute resolve their governing side, Then
   they read P2's values (the bot side is treated as not entered); human on P1 is unchanged.
2. **SE pan follows the flip** — Given a bot session, When SEs play, Then P1/P2 SEs pan like a real
   versus session and the byte is restored at the window exit; an unavailable manager logs once and
   the session proceeds.
3. **Docs** — `docs/multiplayer_bot_research.md` exists with the sections above and no local paths;
   AGENTS.md has the row; the "Judge hook subscribers" neighbourhood no longer implies autoplay owns
   the swap.
4. **Gates** — all green incl. the signature sweep.

## Metadata
- **Complexity**: Medium
- **Labels**: interaction-pass, docs, multiplayer-bot
- **Required Skills**: Rust, this repo's cross-mod conventions, RE-note writing
- **Generated By**: code-task-generator 2026-09-13
- **Source Plan**: `.agents/planning/2026-09-13-multiplayer-bot/implementation/plan.md`
- **Plan Step**: Step 6: Interaction pass, optional SE pan, RE note, AGENTS.md row
