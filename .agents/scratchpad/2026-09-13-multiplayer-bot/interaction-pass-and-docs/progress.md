# Progress — interaction-pass-and-docs (Step 6, task-01)

Status: Complete (uncommitted — maintainer commits manually)

Task: `.agents/tasks/2026-09-13-multiplayer-bot/step06/task-01-interaction-pass-and-docs.code-task.md`
Mode: auto (approval chain verified as for the earlier tasks). No `CODEASSIST.md`. Commit skipped
per AGENTS.md.

## Checklist
- [x] Interaction audit (table in the task file; findings below)
- [x] `multiplayer_bot::is_bot_side(side)` shared predicate
- [x] Exclusions: `premium_free::effective_freeze` (`human_entered`), `announcer_mute::effective_mute`,
      `training_mode::pre_shift_side`, `training_mode::bounds::try_resolve_row_bounds`,
      `timing_offsets::calibration` census, `assist_tick` GAMEPLAY latch
- [x] D22 SE pan: `game_audio::{versus_pan, set_versus_pan}` (+0x20C4 off the derived
      `audio_manager_global`; byte-identical disp32 on all four builds — raw scan of `FUN_1801aa500`'s
      shape), `Snapshot.pan_byte: Option<u8>`, written 1 after the versus word, restored by
      `restore()` AND the arm-failure `undo`
- [x] `docs/multiplayer_bot_research.md` (10 sections: the two facts, PW/GW header, versus-word
      readers + pan byte, commit/record mirroring, AutoFootPanel + judge algebra, Option layout,
      extra-stage grant, saves, phantom-player governance audit, scene edges)
- [x] AGENTS.md Key Entry Points row "Multiplayer Bot" (after 2-Player BPL Mode); `judge_hook.rs`
      doc comment now names `foot_panel_swap` as the ONE swap owner
- [x] Gates: `cargo check` clean → `cargo fmt` (both) → `./build.sh` clean →
      `./scripts/validate_multiplayer_bot.sh` 73/73 → `./scripts/validate_signatures.sh` ALL GREEN →
      path hygiene (no new hits)

## Findings (code review)
- **Real defect class found:** with the human on P2, every "P1 governs when both entered" policy
  read the BOT side's option cache — premium free's stage-bump NOP (re-resolved every scene change,
  incl. scene 31), training's pre-shift + loop latch (taint + death bypass), calibration ("2P"
  refusal), assist tick (stale clap track), announcer mute. Root cause: the flip makes both sides
  read entered, but `versus_mirror` (which makes those policies value-neutral in real versus) only
  engages at song select — never inside the window. Fix = one predicate + six one-line exclusions.
- OK by construction: `song_rate`'s scene-26 classifier (service callback registered before the
  mods' ⇒ fires before the flip on the same edge), `two_player_bpl_mode` (engages against the bot —
  D9 intended), autoplay precedence (service), `per_song_judgement_offsets` (offset-agnostic judge
  algebra), quick restart/fail (`session::classify` host-tested).
- Cosmetic, documented only: training strip placement follows P1; PUS widget / S-Marv paint for the
  bot's pane.

## Deviations
- None from the task.

## Maintainer notes
- Cabinet checks still owed: §7.3 items 1–10 (see the feature `progress.md` Deploy & test log).
  New for this step: human on P2 with a stale P1 cache (premium free / loop song / assist tick /
  announcer mute ON on P1's cache) ⇒ the bot session must follow P2's rows; SEs pan left/right during
  a bot session and centre again afterwards.
