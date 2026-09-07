# Progress — Steps 4 + 5: root-7 re-canvas, overlay rows, README; letterbox present policy

Status: Implemented — awaiting cabinet checkpoint #2 (uncommitted; maintainer commits manually)

## Checklist
- [x] `canvas_fix.rs` — on_frame probe, identity-gated (`+0x50/+0x54 == output`), vtable slot-1 `set_size(1280,720)`,
      600-frame timeout WARN, "already 1280x720" short-circuit
- [x] `rows.rs` — RESOLUTION (presets + unlisted current, SD label) and RENDER SCALE (`= OUTPUT`/75%/50%/1280x720)
      enum rows under the mod toggle; whole-section `save_json_key("resolution", …)`
- [x] `letterbox.rs` — `letterbox_rect_fn` detour, `plan::present_mode` remap, one INFO on first remap;
      no install for Stock / SD crop; ForceLetterbox install failure ⇒ OUTPUT set rolled back
- [x] `mod.rs` wiring (`enable` registers rows, `disable` removes; early_apply installs letterbox + arms canvas fix)
- [x] `plan::GATES.letterbox_policy = true` (Tier A unlocked: 16:9 output with 1280x720 render); T17 updated
- [x] README: Highlights entry, feature-table row, `resolution` config row (incl. spice2x flag conflict + recovery)
- [x] Gates: fmt · harness 24/24 · `cargo check` · `./build.sh` clean (signature set unchanged since the sweep)
- [ ] CABINET CHECKPOINT #2 — see feature `progress.md`

## Deviations
- Steps 4 and 5 shipped as one checkpoint (the SD re-canvas and the Tier-A 1080p path are both cheap to test
  in one session; both need the same build).
- `canvas_fix` also accepts "root already 1280x720" after the first frame as done (a loader/engine variant that
  never sizes root 7 to the screen would otherwise WARN after 600 frames for no reason).
