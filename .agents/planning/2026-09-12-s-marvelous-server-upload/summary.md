# Summary — S-Marvelous Server-Side Awareness

Planning completed 2026-09-12. Design and plan both `Approved 2026-09-12`.

## Artifacts

| File | Purpose |
|---|---|
| `rough-idea.md` | The maintainer's idea + the 2026-09-12 scope additions (FAST/SLOW, backend in scope, echo-back, codegen re-sync, `'8'`) |
| `research/orientation.md` | DLL save/load injection surface; `ReflectSavePlayerData` and `GetGhostData` decompiles (ghost = every slot `'0'+grade`, wire clearkind `rec+0x54` — corrected from a first `+0x270`/`folder` misread after deploy #1); bemani-buddy and bemaniutils survey (highest-`stagenum` rule, DAO/codegen state); header-card lamp decompile (unchecked pointer-table index by clearkind) |
| `idea-honing.md` | 21-decision register, all Accepted/Assumed; `Readiness Confirmed 2026-09-12` |
| `design/detailed-design.md` | Self-contained design for both repos (wire contract, components, data models, error handling, testing) — Approved |
| `implementation/plan.md` | 9-step plan with checklist — Approved |

## Design in brief

**Upload.** On each forwarded per-stage save (`savekind == 2`) of an armed
side, the DLL appends one `void` node `/data/s_marv` (sibling of `<result>`,
describing the stage the marshal just serialised) carrying only what changes
with S-Marvelous awareness: `mcode/style/difficulty` (identity echo),
`window_ms`, `judge_smarv`, exclusive `judge_marv`, `fastcount/slowcount` with
the loose-Marvelous share, `clearkind` (11 = S-MFC), `ghostsize` + `ghost`
(stock index space, `'8'` at S-Marv slots). Values are recomputed from the
stage record streams — the results screen's inputs — through a pure,
host-tested builder. Every stock byte is untouched; any refusal omits the node.
bemani-buddy parses it on kind-2 saves, cross-checks identity against the
highest-`stagenum` result, and stores seven nullable `smarv_*` columns on both
the PB row and the attempt row; PB replacement adds an equal-points
higher-`smarv_count` tie-break so an S-MFC supersedes an MFC.

**Echo-back.** bemani-buddy's load adds `option/smarv_scores`
(`mcode:chart:clearkind|…` for rows where the S-Marv clear kind differs). The
DLL reads it via the existing string-field registry into a per-side S-MFC set
(also fed locally when it emits `clearkind == 11`) and, from a post-original
detour on the song-select header-card refresh, re-binds `fullcombo_<n>p_usr`
to a net-new violet `muca_card_fc_smfc` texture. Stock load fields —
`clearkind` included — are never rewritten: the stock lamp lookup indexes a
pointer table by clearkind with no bounds check.

**Hygiene.** bemani-buddy's protocol model JSON is re-synced with two
hand-added fields and regenerated to a zero diff before any load field is
added.

## Plan in brief

1. bemani-buddy model re-sync (codegen no-op) →
2. DLL pure builder + raw stream reader →
3. DLL producer registry + emission (first packets on the wire) →
4. bemani-buddy migration/models/DAO →
5. bemani-buddy save parse →
6. bemani-buddy load field →
7. DLL S-MFC set + codec →
8. DLL lamp (signature, detour, texture) →
9. docs, learnings, final cabinet pass.

## Next steps

1. Run the `code-task-generator` sop against
   `.agents/planning/2026-09-12-s-marvelous-server-upload/implementation/plan.md`,
   one step at a time starting with Step 1 (tasks under
   `.agents/tasks/2026-09-12-s-marvelous-server-upload/step01/`).
2. Run the `code-assist` sop on each task in order. Skip its commit step
   (maintainer commits manually); record `Status: Complete (uncommitted)`.
3. Maintain `progress.md` in this directory per the repo's PDD tracking
   convention — the cabinet deploy log for Steps 3, 5 and 8 is the feature's
   real validation record.

## Assumptions and areas to watch during implementation

- **Void-node creation** through ordinal 163 with kbin type 1 is verified at the
  first Step 3 deploy; the fallback (omit node + WARN) is designed in.
- **`stage_counter()` == the marshal's stage argument** is assumed as the
  persistence trampoline already does; the identity echo makes a violation
  visible on the backend as a `warn!`.
- **Step 8 is the only step with design-invalidation risk**: the header-card
  refresh signature must hit once on all four builds and its `this+0xD0`
  layer field / lamp block must be byte-stable (`validate_signatures.sh` +
  `shape_diff.py`). If a build diverges, add a `_vN` alternate rather than
  widening the AOB.
- **Current-chart source** for the badge is `PlayerWork+0x54/+0x5C` +
  `GameWork+0`; if a cabinet run shows the lamp lagging the wheel by a
  selection, switch to the wheel model's highlighted `music::Info` (the
  song-length mod's path).
- **Course/Dan** is deliberately out of v1 (kind-2 saves marshal the course
  record; the backend handles courses only on kind 3).
- **Other lamp surfaces** (wheel jackets, score popup, rival list) are a later
  pass; v1 badges the header card only.
- The shared codec/payload **test vectors** between the two repos are the
  contract's regression guard — keep them identical when either side changes.
