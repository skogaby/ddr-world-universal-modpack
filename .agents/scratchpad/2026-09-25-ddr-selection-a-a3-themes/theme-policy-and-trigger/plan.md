# Plan — theme-policy-and-trigger

Status: Approved 2026-09-25 (auto mode; verified approval chain in context.md stands in)

## Test scenarios (written first; must fail before the implementation)

trigger:
- `row_label(7|8|9)` = `DDR A` / `DDR A3 (White)` / `DDR A3 (Gold)`; `row_label(10)` = None; every
  label ≤ 15 bytes ASCII for 0..=9.
- `clamp_row(9)` = 9, `clamp_row(10)` = OFF.
- Explicit rows 7..=9 → skins 6..=8, Source::Explicit (extends `explicit_eras_apply_to_every_series`).
- Dev knob 8 → skin 8 DevKnob; 9 → ignored.
- `auto_skin` table unchanged (17 → 5, 18..=20 → 0).

policy:
- `theme(6..=8)` suffixes `_v0` / `_v2` / `_v1`; `theme(5)` / `theme(9)` None; `is_era` / `is_theme`.
- `tex_number`: 1..=5 → itself, 6..=8 → 0. `engine_skin`: same mapping.
- `skin_name(s)` == `trigger::row_label(s + 1)` for 1..=8; None for 0 and 9.
- For skins 6/7/8 with every adapter: judge → `dance_judge0000_{v0,v2,v1}`, fast_slow, fullcombo
  likewise; game_over → `dance_game_over0000_v0`; danger → `dance_danger0000_v0` (adapter None,
  also with an empty AdapterSet); score_compare → `dance_score_compare0000_v0`; stage →
  `dance_stage_frame0000_vN` (needs StageFrame); song_info → `dance_song_info0000_vN` (needs
  SongInfoPanel); record skin = the theme skin.
- gauge / combo / score / option / message / common / effect / filter / cover / bpm → Stock for 6..=8.
- `decide(_, 9, _)` Stock.
- Invariants extended to 1..=SKIN_MAX: no bare `0000`, theme names end `_vN`, rows never overlap,
  skin-0 bit never set, masks within 1..=SKIN_MAX.
- The fixed-arc list for eras unchanged.
marker_keys: `root_name(6|7|8)` = `dance_common0000_{v0,v2,v1}`; `root_name(9)` None.
song_info_logic: `mode_for_skin(6..=8)` = Panel; 9 → None.

## Implementation
- trigger: constants, labels, dev knob range via a local `SKIN_MAX` literal? No — trigger stays
  independent of policy in the real crate? Both are siblings, `super::policy::SKIN_MAX` works in
  both mounts; use it.
- policy: as design §4.3 / §5.2–§5.3.
