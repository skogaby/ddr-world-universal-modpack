# Progress — extra-stage-guard

Status: Complete (uncommitted — maintainer commits manually)

## Checklist
- [x] `extra_stage_grant` `SignatureDefinition` appended (new `// ── Multiplayer Bot` block)
- [x] Sweep attests 4/4 BEFORE the consumer: `+0x1C6970` (20250805), `+0x1CA7E0` (20260224),
      `+0x1DD0B0` (20260721 — the build the design left to the sweep), `+0x1DDCD0` (20260825);
      independent raw byte scan = exactly one match per build
- [x] `extra_stage_guard.rs` (`init`/`enable`/`disable`/`is_installed`, `grant_hook` with the
      `EnteredRestore` drop guard)
- [x] `mod.rs` wiring (`init` → guard init, non-gating; `enable` → guard enable after the rows;
      `disable` → passthrough; enable INFO reports `extra-stage guard on|OFF (stock rule)`)
- [x] Gates: `cargo check` clean → `cargo fmt` → `./build.sh` clean → harness 73/73 →
      `./scripts/validate_signatures.sh ~/Desktop/ddr_modules` `RESULT: ALL GREEN`; fresh
      `report.py --json` shows `extra_stage_grant → {mod:multiplayer_bot: ['get']}` (soft)

## Log
- The stale `target/sig_harness/out/sigsweep.json` (dated 2026-09-03) is NOT rewritten by the plain
  sweep — pass `--json` to regenerate; do not read consumer classifications from it otherwise.

## Deviations
- None.

## Cabinet log lines (maintainer, §7.3 item 6)
- Boot: `MultiplayerBot: extra-stage guard installed` and the enable INFO's `extra-stage guard on`.
- Results window-out of stage 0 during a bot session: `MultiplayerBot: extra-stage grant evaluated
  without the bot (side N)` — the human who AAAs on a 3-stage setting still gets EXTRA STAGE even
  when the bot did not.
- Signature missing (never expected on the four builds): `MultiplayerBot: extra_stage_grant signature
  missing -- the extra-stage grant will consider the bot (stock rule)` and the mod still enables.
