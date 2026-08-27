# Progress — step09 task-02 removals-and-ribbons

- [x] Removed register_offset_rows() + its enable() call + the Axis/set_offset
      machinery + save_json_key wiring + the mod_menu import (2.9 KB excised;
      no orphaned code — check clean). init()-time config reads (offset_x/y/
      spacing/scale + defaults) and the per-frame layout consumers untouched.
- [x] STOCK_RIBBONS += seop_op_left/seop_op_right (asset_gen.rs, with rationale
      comment)
- [x] Cabinet: forced atlas-REBUILD boot (option atlas caches cleared) — the 6
      seop_op_left/right WARNs are GONE (log shows only the 6 pre-existing
      Series Expansion baseline WARNs); grep confirms no mwsl-offset rows
      registered
- [x] Gates: harnesses 3/3, check 0 warnings, fmt, build.sh

Status: Complete (uncommitted — maintainer commits manually)
