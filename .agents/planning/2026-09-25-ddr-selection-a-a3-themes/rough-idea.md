# Rough idea

Extend DDR SELECTION (`src/mods/ddr_selection/`) with three more selectable gameplay themes, built
from data the stock World install already ships:

- **DDR A** — the DDR A generation (`*0000_v0`, `dance_message_v0`, `common_choice_v0`,
  `common_shutter_v0`).
- **DDR A3 (White)** — A3's own UI as a non-gold cabinet showed it (`*0000_v2` / `_v2`).
- **DDR A3 (Gold)** — A3's own UI as the gold cabinet showed it (`*0000_v1` / `_v1`).

Maintainer decisions carried in from the feasibility pass
(`docs/ddr_selection_a3_themes_research.md`, 2026-09-25):

- Two separate A3 values (White, Gold), as outlined in the research doc.
- AUTO lumps A20 and A20 PLUS together with A3.
- ~~The DDR A through A3 options get the same era cut-in that 2013-A has.~~ Withdrawn by the
  maintainer: the new themes have **no era cut-in**; only the original five eras keep theirs
  (this is also what A3 did for its own UI).
- The existing `2013-A` value is relabelled `2013-2014` (already done in `trigger.rs`), since DDR A
  becomes its own entry.
- `_sel` background movies stay off for the new themes (A3's rule).
