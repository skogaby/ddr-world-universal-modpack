# Plan — extract-foot-panel-swap-service

Status: Approved 2026-09-13 (auto mode — upstream plan/design approval stands in; see context.md)

## Test scenarios (host, `layout.rs` `#[cfg(test)]`)

| # | Scenario | Input | Expected |
|---|---|---|---|
| T1 | Bot outranks Perfect | `effective_controller(true, true)` | `Controller::Bot` |
| T2 | Bot alone | `(true, false)` | `Bot` |
| T3 | Perfect alone | `(false, true)` | `Perfect` |
| T4 | Neither | `(false, false)` | `Off` |
| T5 | Object size covers the largest stock object | `PANEL_OBJECT_SIZE` | `== 0x58` and `>= 0x40` |
| T6 | Actor offsets pinned | constants | `ACTOR_SIDE == 0x84`, `ACTOR_RESULTS_BEGIN == 0xB0`, `ACTOR_CUR_BEAT == 0x168` |
| T7 | `BotPanelFlags::default()` is all-zero | default | every array zero (the service copies it verbatim into the panel object) |

These fail against the absent module (compile error) and pass only once `layout.rs` exists with
the specified semantics. No stubs.

Engine-facing behaviour (AC1–AC3, AC5) is cabinet-validated per the repo's rules; the code
review checklist in `progress.md` covers the fail-open paths by inspection.

## Implementation shape

1. `src/services/foot_panel_swap/layout.rs` — pure: `Controller`, `effective_controller`,
   `PANEL_OBJECT_SIZE`, `ACTOR_SIDE`, `ACTOR_RESULTS_BEGIN`, `ACTOR_CUR_BEAT`, `BotPanelFlags`
   (`#[derive(Clone, Copy, Default)]`, `#[repr(C)]`), tests.
2. `src/services/foot_panel_swap/mod.rs`:
   - statics: `AVAILABLE`, `FOOT_PANEL_OFFSET: AtomicUsize`, `STOCK_PANEL: AtomicPtr<u8>`,
     `STOCK_UPDATE: AtomicPtr<()>` (fn ptr), `PERFECT: [AtomicBool; 2]`, `BOT_ARMED:
     [AtomicBool; 2]`, `BOT_FILL: [AtomicPtr<()>; 2]`, `BOT_UNWIRED_WARNED: [AtomicBool; 2]`,
     `ORIGINAL_FOOT_PANEL: [AtomicPtr<u8>; 2]`. All atomics ⇒ no `static mut` needed at all.
   - `init`: resolve → alloc → register pre/post → `AVAILABLE = true`; on any miss WARN + false
     (unregister a half-registered pair).
   - `swap_in(actor, mc)` / `swap_out(actor, mc)` per the task's R3.
   - API: `is_available`, `set_perfect`, `arm_bot`, `disarm_bot`, `controller`, `BotFillFn`.
3. `src/services/mod.rs` — declare module (alphabetical: after `custom_options_persistence`,
   before `game_audio`).
4. `src/lib.rs` — init after judge_hook (step "6b0").
5. `src/mods/autoplay.rs` — slim per R6; update module docs.
6. `mine_render.rs` comment retarget.
7. Gates: cargo check → fmt → build.sh → temp-crate host test of `layout.rs` → greps.

## Risks

- Behaviour drift in the Perfect path: mitigated by porting the callback bodies line-for-line
  and keeping priorities identical; cabinet AC2 is the oracle.
- `AtomicPtr` ↔ fn-pointer casts: the `update` fn is stored as `*mut ()` and transmuted back
  only when non-null — same shape autoplay used via `Option<AutoUpdateFn>` in a `static mut`,
  now without `static mut`.
