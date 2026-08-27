# Plan — tabbed-shell-integration

Status: Approved 2026-08-24 (auto mode; verified approval chain per context.md)

## Test scenarios

Model behavior is already host-tested (task-01, 11 tests). This task is the impure
shell; validation:

1. Compile gates: `cargo check` 0 warnings → `cargo fmt` no churn → `./build.sh`;
   `./scripts/validate_mod_menu.sh` still green.
2. Autonomous boot + keypad-injected walkthrough (spice2x-cli): triple-0 open; log
   shows open; inject 3/1 tab switches, 2/8 navigation, 6/4 toggles on a safe row
   (hello-world) and value adjustments (FPS TARGET); verify via log + config file
   (`mod-config.json` mods map + `fps_unlock.selected`) that toggle/adjust/persist
   still work; triple-0 close; zero panics/new WARNs.
3. Screenshot capture for the maintainer handoff (no agent visual verdicts).
4. Maintainer visual sign-off = the step demo gate.

## Implementation order

mod.rs state swap → tabs.rs → rows.rs surgery → input.rs → render.rs → gates →
autonomous walkthrough → maintainer handoff.
