# Plan — bot-controller
Status: Approved 2026-09-13 (auto mode — upstream approval stands in)

Tests (host, layout.rs): T1 offsets/size of BotFootPanel; T2 bot_vtable_image (COL, 0–4 verbatim,
5/6 replaced); T3 apply() copies all 24 values; T4 side masks (`& 7`) helper if extracted.
Engine-facing (mod.rs Bot arm, filler, self_test): cabinet AC2–AC4 per the task.

Order: layout tests → layout impl → service Bot arm → filler → self_test → mod.rs/lib.rs/config → gates.
