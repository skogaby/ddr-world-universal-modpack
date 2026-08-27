# Research Pointer

The reverse-engineering research for this feature was completed *before* the
PDD phase (ad-hoc RE session, 2026-07-12) and lives in the repo's durable RE
docs rather than this folder:

- **`docs/gameplay_overlay_elements_research.md`** — element inventory
  (ComboActor / NoteResultActor / CMovieClip pool), the scale + opacity
  injection mechanisms, hook design recommendation, AOB signatures, and the
  two-build cross-version address table (20260616 + 20260324).

Supporting existing docs:

- `docs/afp_system.md` — AFP id types, libafp API signatures, BM2D pool
  (`bm2d_pool_iter` AOB, stride 0x240).
- `.agents/summary/components.md` → `bm2d_api` — the AFP-layer wrapper set
  (`layer_set_scale` via `afp_layer_set_matrix`, `layer_set_position`
  composition — cabinet-proven by the background-preview feature).
- `src/mods/power_user_statistics/pacemaker_swap.rs` — existing patch inside
  the same NoteResultActor message handler (case 0x1036) this feature's
  pacemaker element belongs to (coexistence note: PUS patches an 11-byte site
  *inside* `FUN_18007af00`; this feature detours different functions —
  `CMovieClip::Create` / wrapper SetColor — no overlap).
