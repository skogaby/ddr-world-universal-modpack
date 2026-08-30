# Progress — task-02 real-template-edit-demo

Status: Complete (uncommitted — maintainer commits manually)

## Checklist
- [x] `ap2check edit-demo <afp> <bsi> <out_afp> <out_bsi>` subcommand:
  remapped variant (add_shape → re-resolve path → segment source-id remap →
  clone → serialize → re-parse → assert clone references the new shape id)
  + identity variant written out with the string table RE-SCRAMBLED
  (bemaniutils' SWF parser always runs the cipher) and a 0-byte BSI
- [x] Leg C in `scripts/validate_s_marvelous.sh` (env-gated like A/B):
  edit-demo on the real `dance_judge` → parseafp cross-check → render
- [x] Cross-check: bemaniutils independently parses the edited file and
  reports `in_smarvelous @ frame 600`
- [x] Render preview: cloned segment (frames 601..638, 1-based) rendered via
  the bemaniutils AFPRenderer wired from the stock IFS with the edited SWF
  substituted; stops neutralized on the parsed render copy (root timeline
  carries stop() bytecode — in-game the clip is label-jump driven).
  Output: `$TMPDIR/s_marvelous_preview/in_smarvelous_identity.gif` —
  **visually verified: shows the stock "Marvelous!!!" word art** (identity
  remap ⇒ correct expected result)
- [x] All legs green (exit 0); Leg A 76/76 + Leg B 3/3 unchanged; synthetic
  suite unchanged; cargo check clean; no repo pollution

## Real-template facts captured
- dance_judge section with `in_marvelous`: label at frame 562-ish? →
  reported: in_smarvelous lands at frame 600 of 638 after the 38-frame
  clone append ⇒ the in_marvelous segment is 38 frames long
- The marvelous word placement references source character id 8 (remapped
  to new shape id 54 in the structural variant)
- The dance_judge root timeline carries stop() bytecode; free playback ends
  ~frame 34 — irrelevant in-game (label jumps) but render previews must
  neutralize DoAction bytecodes

## Deviations
- Fixed mid-task: `afp/afplist.xml` sits in the IFS afp/ namespace — the
  render loader must skip `.xml` (KeyError on its nonexistent BSI otherwise).
- Commit step skipped per repo AGENTS.md git rules.

## Step 3 sibling status
- task-01 editing-primitives: Complete
- task-02 real-template-edit-demo: Complete (this)
→ Step 3 checklist item ticked in the source plan.
