# Progress — FPS Unlock implementation (code-assist)

Mode: **auto** (per CLAUDE.md — work the plan in one session, surface `cargo check` per step).
Authoritative plan/design/research live in `.agents/planning/20260627-fps-unlock/`.
No test harness in this repo → "validation" = `cargo check --target x86_64-pc-windows-msvc`
per step + the consolidated Step-8 cabinet deploy. Build logs in `logs/`.

## Checklist (mirrors implementation/plan.md)

- [x] **Step 1** — Part I: `RowKind::Enum` variant + all exhaustive match sites
- [x] **Step 2** — Part I: `EnumRowSpec` + `register_enum_row` (adjust/render/repeat landed in Step 1)
- [x] **Step 3** — Part II: `fps_target_imm32` signature (AOB) — added to `signatures.rs`, check clean
- [x] **Step 4** — Part II: `FpsUnlockConfig` typed section — `config.rs` (normalization lives in the mod, Step 5)
- [x] **Step 5** — Part II: mod scaffold + registration (`fps_unlock.rs`, `mod.rs`, `lib.rs`)
- [x] **Step 6** — Part II: `early_apply` byte-patch + stock capture + revert
- [x] **Step 7** — Part II: enum row + on_change persist + degradation/is_active
- [x] **Step 8** — README/docs done; cabinet validation PASSED (see below) + 2 UX fixes applied

## Step 8 — cabinet test PASSED + UX follow-ups (2026-06-28)

Maintainer deployed and tested on-cabinet: **FPS unlock works** (boot-race resolved fine via
`early_apply` — the patch lands before `onBoot` reads it; higher refresh confirmed in
exclusive fullscreen). Two UX notes fixed:

1. **Child row mis-ordered** — the `FPS TARGET` enum row appeared at the *end* of the overlay
   list instead of directly under the `fps-unlock` master toggle. Root cause: `rebuild_rows`
   pushed all registry Boolean rows first, then appended *all* contributed rows — so a child
   of an early-registered mod stranded at the end (timing-offsets only looked right because
   it registers last). **Fix:** `rebuild_rows` now emits each registry toggle immediately
   followed by its matching contributed children (matched via `visible_when` parent key),
   with a fallback append for any unparented/orphaned contributed rows. Order-independent now
   — also makes timing-offsets robust regardless of registration order.
2. **Hint didn't mention reboot** — `FPS TARGET` hint changed to
   `"Display refresh target. Restart the game to apply."`

Both: `cargo check` + `./build.sh` clean.

## Steps 5–7 — DONE (cargo check + `./build.sh` + clippy all clean)

Wrote `src/mods/fps_unlock.rs` (single-file mod) covering scaffold + apply lever + overlay +
persistence + degradation in one pass (the plan split these but they're cohesive in one file):
- **Apply lever:** `early_apply` reads `fps_target_imm32` from `EarlyContext.signatures`
  (it's a linear AOB → already resolved by `resolve_all`, no manual re-scan needed, cleaner
  than song_limit's self-scan). Validates stock==60, captures it, writes `selected` as u32 at
  match+4 when ≠ stock. `init` re-resolves if early_apply was config-gated off.
- **Config:** `normalize()` (range-filter [1,1000] / sort / dedupe / auto-add selected /
  fallback to defaults if empty). Keeps `original_presets` separate for Q9 write-back fidelity.
- **State:** global `STATE: Lazy<Mutex<FpsState>>` (timing_offsets pattern) so the enum row's
  `Arc<dyn Fn>` `on_change` can persist without `&mut self`.
- **Overlay:** `register_enum_row` (`FPS TARGET`, hint "Display refresh target.", `Nfps`
  labels, parent `fps-unlock`). `on_change` → `set_selected` → persist + "applies next launch".
- **Degradation:** patch site load-bearing (`is_active()` = `patch_site.is_some()` → [OFF]
  rendering if AOB missed); overlay row optional. `disable` removes row + reverts patch.
- **Wiring:** `pub mod fps_unlock;` (alphabetical in mod.rs); registered in lib.rs right after
  song_limit (both early_apply byte-patch mods).

**Builds:** `cargo check` exit 0; `./build.sh` links clean (dll 8.0 MB); `cargo clippy` — zero
new hits in `fps_unlock.rs` / `mod_menu.rs` (other-mod warnings pre-existing).

Design open-items resolved in code: #3 clamp (not wrap); #4 sanity range [1,1000];
#2 default selected=60.

## Step 1 — DONE (cargo check clean, exit 0)

Added `RowKind::Enum { index, values: Vec<i32>, labels: Vec<String> }` to `mod_menu.rs`.
The adjust/render/repeat behavior is wired in this step (it was natural to do alongside the
match-handling rather than split to Step 2); Step 2 is now just the public registration API.

Sites handled:
- `RowKind` enum + module doc comment updated.
- `row_value` — changed `match r.kind` → `match &r.kind` (Enum's `Vec` isn't `Copy`); Enum
  returns `values[index]`.
- `clone_row` — `match &r.kind`; Enum clones the two vecs.
- `activate_selected` — new Enum arm: Left/Right cycles `index` ±1, **clamped at ends (no
  wrap)** matching Scalar; coarse (Start-held) is a deliberate no-op for enums; fires
  `on_change(values[new_index])`, mirrors via `set_row_value_and_refresh`.
- `set_row_value_and_refresh` writer — Enum arm sets `index` from value via new
  `enum_index_of` helper (leaves index unchanged if value absent).
- `refresh_slots` render — changed `match row.kind` → `match &row.kind`; Enum shows
  `labels[index]` (fallback `"?"`), white text.
- repeat gate `selected_is_scalar` → renamed `selected_repeats` (scalar **or** enum), so
  enums cycle on hold; updated call site + thread doc comment.
- New helper `enum_index_of(values, value) -> Option<usize>` (caller arrives in Step 2;
  crate-wide `#![allow(dead_code)]` covers the interim).

Decision: clamp-vs-wrap = **clamp** (design open-item #3 resolved → clamp, matches Scalar).

## Notes / deviations
- Plan listed Step 1 = "variant + match handling (inert)" and Step 2 = "API + behavior".
  In practice the adjust/render/repeat behavior IS the match-handling, so it landed in Step
  1. Step 2 reduces to the public `EnumRowSpec` + `register_enum_row` (+ value→index
  resolution at registration). No functional gap; just a cleaner split.
