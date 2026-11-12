# Plan — source-rows-and-preview
Status: Approved 2026-09-30 (auto — verified upstream approval chain)
No new host tests (the decisions are in options_logic; this is the shell). Validation: check/fmt/build/lint + the design's §7.3 cabinet checklist.
Implementation: per design §4.5; `register_one(id, spec, max, display_name, description)` applies the common tail (keeps the lint's `.display_name(`/`.description(` next to `register_option(`); observer subscribed once (`MIRROR_SUBSCRIBED`).
