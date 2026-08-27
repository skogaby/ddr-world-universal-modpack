# Rough Idea — Custom Options Row Order

Add a new key under `"custom_options"` inside `mod-config.json` called `row_order`.
The value is an array of strings, each string being the id of an option row that
appears in the in-game custom options menu (the MODS tab) provided by the modpack's
custom-options framework. Example:

```json
"custom_options": {
  "row_order": ["premium_free", "customize_background", "customize_lanecover_single", "..."]
}
```

The order of the ids in this array determines the order in which the option rows are
presented in-game on the MODS tab through the custom-options framework.

## Motivating rationale

Most users are opinionated about which options should appear at the top vs. the
bottom of the list. Rather than the modpack prescribing the "best" order, make it a
tunable, operator-authored value.

## Explicit requirements from the request

- If an option is **not present** in `row_order`, it falls back to the **end** of the
  list (not the beginning).
- If `row_order` contains a string that **doesn't belong to any real option**, log a
  **warning** but **ignore it** — do not fail because of it.
