# Progress — Step 5 Task 01: option-row label asset

- [x] `("assist_tick", "ASSIST TICK")` added to `scripts/gen_option_labels.py` `LABELS` (mod-toggles group)
- [x] Regenerated with the mise-managed Python 3.12.5 (has Pillow 11.2.1)
- [x] `seop_item_assist_tick.png` committed-to-tree: 176×16 RGBA, matches the family
- [x] Installed into `$DDR_WORLD_INSTALL/data_mods/custom_options/.../tex/`

## Deviations

- The regeneration also rewrote 5 pre-existing PNGs with different bytes (Pillow-version /
  rendering drift vs. whoever generated them last: `seop_item_arrow_opacity`, `seop_item_arrow_scale`,
  `seop_item_perspective`, `seop_op_hallway`, `seop_op_overhead`). **Reverted** — only the new
  label is kept, per the task's "no sibling label's bytes changed" criterion. Worth knowing for
  Step 6: a full regeneration on this machine will not be byte-stable against the committed set.

AC2 (in-game label after one relaunch) is the maintainer's end-of-step manual pass.

Status: Complete
