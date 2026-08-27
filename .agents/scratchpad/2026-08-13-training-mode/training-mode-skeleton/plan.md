# Plan — task-03 training-mode skeleton + demo knob

Status: Approved 2026-08-13 (via approved upstream plan/design — code-assist
auto mode; see context.md approval chain)

## Implementation approach

1. **Bind-time initial mapping (task-02 addendum, host-tested)**
   - `Binding::ms_to_blocks(ms)` — floor onto the main entry's block grid.
   - `BindContext.initial_mapping_ms: (u64, u64)` — applied via
     `set_content_mapping` between `prepare_binding` and registry
     publication in `bind_for_create` (before any engine read can observe
     the binding).
   - runtime.rs: packed `INITIAL_MAPPING_MS` atomic +
     `set_initial_content_mapping_ms(shift_ms, lead_ms)`; `create_hook`
     threads it into `BindContext`.
2. **Mod skeleton** `src/mods/training_mode/mod.rs`
   - id `training-mode`, no required signatures, no detours/widgets.
   - enable: gate on `song_rate::runtime::integration_ready()` (else WARN +
     self-disable); `set_training_arm(true)`; TEMPORARY demo knob read
     (`DDR_TRAINING_TEST_SHIFT_MS` → initial mapping with
     `TRAINING_LEAD_MS = 2500`).
   - disable: `set_training_arm(false)` + clear the initial mapping.
   - `is_active()` honest.
3. **Registration**: `pub mod training_mode;` in mods/mod.rs; instance
   appended to lib.rs `mods_to_register`.

## Test scenarios

| # | Criterion | Test |
|---|---|---|
| 1 | Bind-time pre-shift lands before publication | wavebank_hook_tests: identity arm + `initial_mapping_ms (1000, 500)` on the 8 kHz fixture ⇒ live binding's `content_mapping() == (62, 31)` (floor of ms×8000/1000/128); zero mapping default unchanged elsewhere |
| 2 | ms→block conversion | covered by #1 (exact floor values asserted) |
| 3 | Mod behavior (enable gates, arm request, knob, zero footprint) | engine-facing — cabinet demo (design §7 model); `cargo check` for the glue |

## Cabinet demo checklist (after deploy)

- (a) mod enabled, 100 % song, no knob → logs show identity arm + commit at
  100 %, audio normal, score submits.
- (b) `DDR_TRAINING_TEST_SHIFT_MS=60000` → 2.5 s silence then content at
  ~1:00; true beginning never audible; notes/clock unadjusted (Step 2/3).
- (c) mod disabled → stock (no arm lines).

## Risks

- The initial-mapping application must precede `registry.publish` (reads
  begin the instant the binding is visible) — placed immediately after
  `prepare_binding` returns.
- Timeline log noise per song while enabled (known, accepted for Step 1).
