# Rough Idea: Bulk Hack Porting

Port a curated subset of mods from the pre-modded `gamemdx_20250805_MODIFIED.dll`
into this universal hook DLL, grouped into three new mods. Each must work
version-agnostically across all DDR World binary releases (currently 20250805
through 20260421). Mods are scoped via the existing mod menu; per-player mods
also expose in-game options (gated by the global mod's enable state) that
persist over the network through the existing custom options framework.

## Preamble

The current scene-ID gating on the mod menu (only-openable during attract loop,
auto-closes when leaving the attract scene range) should be removed. The user
should be able to open the mod menu on any screen, no matter what.

## New Mod 1: StageProgressionHacks (global, mod-menu gated only)

(1) **Unlimited stages / Premium Free** — Freeze the stage counter at the
current round, enabling infinite gameplay. Scores must still post to the
backend after each song; perpetually overwriting the same stage slot in the
save packet is acceptable behavior (most DDR World backends — including the
user's own `~/Desktop/Projects/bemani-buddy` — read the last index in the
save call).

(2) **Quick song restart (stay on the same round)** — Either player presses
**1** on their respective keypad three times in 1.5 seconds during scene 28
(GAMEPLAY) to restart the song. Approach: trigger another scene transition
to scene 28 to reinitialize gameplay; clear the internal step-timing
accumulators the game collects during play.

(3) **Quick song fail (skip results screen)** — Either player presses **3**
on their respective keypad three times in 1.5 seconds during scene 28
(GAMEPLAY) to abort to scene 25 (SONG_SELECT). Stage counter increment
behavior must be confirmed (whether it happens at song-pick or
song-completion). If the Premium Free hack is on, do not increment.

## New Mod 2: SongSelectionImprovements (global, mod-menu gated only)

(1) **"Real Speed" Calculations Fixed** — Use Core BPM instead of Max BPM
in the scroll-speed display formula. Refer to
`docs/binary_modpack_research.md §4`.

(2) ~~**Updated Speed Toggle (smaller increments)**~~ — DROPPED FROM SCOPE.
The user verified live on 20260421 that Konami implemented this natively
(±0.05× fine, ±0.50× coarse with Start held) sometime after the original
mod's 20250805 base. The mod is functionally redundant on the current
version. See idea-honing.md Q9 for the full record.

(3) **Replace Flare Clear Banner With Clear/Combo Lamps** — On the
results screen, swap the flare-clear banner for clear-lamp colors
(MFC=white FLARE EX, PFC=gold FLARE IX, etc.). Refer to
`docs/binary_modpack_research.md §15`.

## New Mod 3: PowerUserStatistics (per-player, options-gated)

(1) **Timing Statistics During Gameplay** — Per-step ms-error stats
(Max/Mean/AbsMean/Current). Unlike the original mod which overwrote the
EVENT MODE banner, this should create its own text widget(s) shown only
during scene 28 (GAMEPLAY).

(2) **Pacemaker → MsError Switch** — Per-player toggle that swaps the
pacemaker readout for ms-error. Driven by in-game options (no operator-menu
dependency). Probable shape: main ON/OFF toggle, with a child option (visible
when the main is ON) for the white-pacemaker-zone threshold.

(3) **Export Step Data (CSV)** — Per-song step-by-step error data export.
Output to a folder next to the DLL (e.g. `./step_data_exports/`) instead of
the original mod's `/dev/nvram/` location.

## Constraints

- Mods MUST resolve all addresses via AOB scan / RTTI walk; no hardcoded
  offsets. Verify each AOB on at least 20250805 and 20260421.
- Hot paths (judgeNotes, render hooks) must keep callbacks tight (<1ms).
- Per-player options for Mod 3 must persist via the custom options framework
  (network round-trip + JSON local cache).
- The mod menu must remain openable from anywhere after the gating change.

## Reference Material

- `docs/binary_modpack_research.md` — RE notes for each ported mod, with
  cross-version AOB anchors verified on 20250805 and 20260421.
- `.spec/steering/*.md` — project conventions, allocator rules, hook
  patterns.
- `~/Desktop/Projects/bemani-buddy` — user's backend server (for confirming
  score-save behavior under Premium Free).
