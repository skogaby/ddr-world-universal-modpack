# Summary — FPS Unlock (PDD)

A runtime hook-DLL mod that overrides DDR World's fullscreen display-refresh target
(stock 60) with an operator-chosen value (presets 60/120/144/165/240/360, config-extensible),
giving smooth high-refresh gameplay. 64-bit port of `patches.js` Hack 5
(`docs/hex_edit_porting.md`), with **all RE re-verified fresh this session** — which
corrected two errors in the prior doc and **simplified the feature** (dropped the
speculative per-scene-switching milestone).

## Artifacts

```
.agents/planning/20260627-fps-unlock/
├── rough-idea.md                       # concept (Hack 5) + re-verification mandate
├── idea-honing.md                      # Q1–Q9 requirements + RE-research conclusions + checkpoint
├── research/
│   ├── r1-fps-target-site.md           # AOB CONFIRMED unique on 3 builds; imm32 @ match+4 in onBoot
│   ├── r2-target-flow-and-liveness.md  # consumed-once-at-boot; apply via early_apply; 2 prior-doc errors fixed
│   ├── r3-menu-speedup-reality.md      # World is dt-based → no speedup → Milestone 2 dropped
│   └── r4-enum-overlay-infra.md        # RowKind::Enum is a small, self-checking addition
├── design/
│   └── detailed-design.md              # two-part design (Enum infra + the mod)
├── implementation/
│   └── plan.md                         # 8-step plan + checklist (Enum-first, one final test phase)
└── summary.md                          # this file
```

## Design in brief

**Two parts:**

- **Part I — reusable `RowKind::Enum`** in `mod_menu`: a labeled pick-list row
  (`{ index, values, labels }`) + `register_enum_row(EnumRowSpec)`, mirroring the existing
  `Scalar` row the timing-offsets feature added. Reuses the i32 `on_change` and
  `visible_when` parent-gating. General-purpose; FPS is its first consumer.
- **Part II — the `fps-unlock` mod**: AOB-resolve the FPS immediate; in the **`early_apply`**
  boot phase capture the stock byte and byte-patch it to the configured `selected` value
  (precedent: `song_limit_expansion`); typed `fps_unlock` config (`presets` + raw `selected`,
  in-memory normalized, only `selected` written back); register the `FPS TARGET` enum row;
  two-tier graceful degradation (apply lever load-bearing → self-disable if AOB misses;
  overlay row optional → config-only fallback).

**Key decisions:** static cabinet-wide value (no per-scene); enum-of-presets with
config-defined entries; `selected` stored raw; OFF reverts to captured stock (60 fallback);
changes **apply on next launch** (value is latched into the D3D device at boot — RE-confirmed);
default `selected = 60` (mod-on is a no-op until the operator picks higher); label
`FPS TARGET`, neutral hint, `"<n>fps"` entries, hidden when master OFF.

## Research outcome (all RE re-verified fresh; 3 builds)

- **r1:** AOB `C7 44 24 ?? 3C 00 00 00 75 08 C7 44 24 ?? 4B 00 00 00` — **unique single match
  on 20260324, 20260526, AND 20250805** (the last never checked by the prior doc),
  byte-identical. Patch target = `0x3C` imm32 at **match+4**, inside `Application::onBoot()`.
- **r2:** full data flow re-traced: `onBoot` → `FUN_1801eda10` (writes global `DAT_1806ea488`)
  → `Renderer:initGs` (**sole reader, at boot** → configures the **D3D device**). **The value
  is consumed once and never re-read per frame.** Two prior-doc errors corrected: target lands
  at struct **+0x1C** (not +0x14), and the real consumer is the `FUN_1801eda10`→`initGs` chain
  (not `FUN_1801f0030`, which doesn't read the struct). Apply lever = AOB byte-patch via
  `early_apply`; boot-timing is the one empirical risk; documented fallback ladder
  (detour → last-resort on-disk patch; no ban risk on unofficial nets).
- **r3:** engine is overwhelmingly delta-time based (`DAT_1806ea714`, ~100 readers, per-frame
  clamp); sampled animation path scales by dt. With the maintainer's live World test showing
  **no menu speedup**, the prior doc's speedup premise appears to be **older-DDR behavior
  carried into World by assumption.** → per-scene switching **dropped entirely** (also
  infeasible via this lever per r2).
- **r4:** `RowKind::Enum` is a small, low-risk, compiler-self-checking addition (~6 match arms
  + a `register_enum_row` API), modeled on the existing `Scalar` row.

## Implementation plan (8 steps; Enum-first, single final test phase)

Enum variant + match handling → enum registration API → FPS signature → typed config +
normalization → mod scaffold + registration → `early_apply` byte-patch + stock capture +
revert (config-only feature complete here) → register the enum row + persist + degradation →
**one consolidated cabinet-validation phase** + docs. Each step is `cargo check`-gated and
left wired-in; per maintainer, **all cabinet testing is deferred to Step 8** (one deploy
exercises the whole feature), with `./build.sh` only before that final deploy.

## Next steps

1. Review `design/detailed-design.md` and `implementation/plan.md`.
2. Begin **Step 1** (add `RowKind::Enum` + handle the exhaustive match sites in `mod_menu`).
3. Proceed through the checklist; the single cabinet validation + docs land in Step 8.

## Areas that may need refinement (carried to implementation)

- **Boot-race confirmation (Step 6/8):** the one genuine empirical unknown — does
  `early_apply` beat `onBoot`'s FPS line? Validated in the Step-8 deploy; fallback ladder
  ready (r2). Static analysis can't prove boot ordering.
- **Enum cycle clamp vs. wrap:** recommend clamp (matches Scalar); confirm in impl.
- **`presets` sanity bound** for normalization (e.g. `[1,1000]`): finalize in impl.
- **Doc corrections:** Step 8 must fix the two RE errors in `docs/hex_edit_porting.md` Hack 5
  (offset +0x1C; real consumer chain) and mark it IMPLEMENTED / Milestone-2-dropped.
```
