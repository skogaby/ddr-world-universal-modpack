# Summary — Custom Options Row Order

## What this is

A feature spec (Prompt-Driven Development) for adding an operator-authored `row_order`
array under `custom_options` in `mod-config.json` that controls the display order of the
modpack's custom option rows on the in-game MODS tab.

## Artifacts created

```
.agents/planning/20260723-custom-options-row-order/
├── rough-idea.md                     # the original request
├── idea-honing.md                    # 12 requirement decisions (all confirmed)
├── research/
│   └── existing-mechanism.md         # how row order works today; config plumbing; id list
├── design/
│   └── detailed-design.md            # full design + code sketches + rules + alternatives
├── implementation/
│   └── plan.md                       # 6-step single-pass plan + checklist + validation matrix
├── progress.md                       # live resume point (repo convention)
└── summary.md                        # this file
```

## Design in one paragraph

Row order today is implicit registration order, driven solely by the `handles` iteration
in `custom_options::builder_hook::builder_detour_body`. The feature adds
`row_order: Option<Vec<String>>` to `CustomOptionsConfig`, a new `ordering` leaf module
(configured-order store + pure `compute_order` + warn-once `display_order_for`), reads the
config once at `custom_options::init()`, and reorders the `handles` snapshot before row
injection. Because `rows::ROWS` and the scroll driver are built from that same iteration,
visual and scroll order follow for free. The `registry::STATE.options` Vec and
`OptionHandle` indices are never reordered, so handles stay valid. Absent/empty `row_order`
⇒ identity ⇒ zero behavior change.

## Rules

- Listed ids render first, in listed order (case-insensitive match, duplicates → first wins).
- Unlisted registered options fall to the end, in current registration order.
- An id matching no registered option ⇒ one WARN + ignore (never fatal).
- Cabinet-wide; read at boot (relaunch to apply); `ShowWhen` visibility unaffected.

## Implementation plan (single pass)

1. Add `row_order` field to `CustomOptionsConfig`.
2. Add `ordering.rs` (store + pure `compute_order` + `display_order_for`).
3. Read config in `custom_options::init()` → `set_configured_order`.
4. Reorder the `handles` snapshot in `builder_hook`.
5. Docs (README, AGENTS.md, summary docs).
6. Build gates + on-cabinet validation matrix.

## Next steps

- Give the go-ahead to implement (per plan Step 1→6 in one pass), or request design tweaks.
- During implementation, keep `progress.md` current and validate on-cabinet at the end.

## Possible refinements (flagged, not blocking)

- Whether `ordering` should be its own file or folded into `builder_hook` (design leans
  separate file).
- Whether to add a tolerant deserializer so a wrong-typed `row_order` degrades to `None`
  instead of the existing whole-file config fallback (deferred).
