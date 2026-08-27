# Plan: store-layer

Status: Approved 2026-08-17 (inherited from approved plan/design chain, auto mode)

## Test scenarios (from the task's acceptance criteria)

1. `wire_round_trip` — map {puty:11, neg:-100, zero:0} → encode sorted stable
   → decode → identical map; two encodes byte-equal.
2. `empty_string_semantics` — side with entries + `apply_server_string("")` →
   empty map; `encode_side` of empty map = "".
3. `malformed_wire_tolerance` — `"puty|11|bad|xx|aaaa|999|dangling"` →
   {puty:11, aaaa:100}, stats: skipped=1 (bad pair), clamped=1, dangling
   dropped (skipped counts it).
4. `merge_semantics` — baseline {A:5} (both sides), edit B on side 0, server
   "C|3" on side 0 → side0 == {C:3}, side1 == {A:5};
   `reset_to_baseline(0)` → {A:5}.
5. `unknown_code_preservation` — server string with code absent from baseline
   round-trips through encode.
6. `cap_enforcement` — 2001 entries: encode keeps exactly 2000
   (deterministic prefix of sorted order); decode of a 2001-entry string
   keeps 2000 + stats.truncated.
7. `decision_helpers` — truth table for `arm_decision` (entered × course ×
   code known × entry exists), incl. entry value 0; `row_seed` returns (1, v)
   with entry (incl. v=0), (0, 0) without.
8. `armed_gate` — fresh store `is_armed() == false`; after `load_baseline`,
   true.

## Implementation shape

- `DecodeStats { skipped: u32, clamped: u32, truncated: bool }`.
- `pub const MAX_ENTRIES: usize = 2000;`
- `Store` methods per the task; encode: collect → sort_unstable by code →
  truncate(MAX) → fold into String with `|` separators.
- Decode: `split('|')` iterator consumed two-at-a-time; code token empty or
  containing nothing → skip pair; offset parse i64 → clamp (count) or skip
  (count).
- Helpers `row_seed` / `arm_decision` as free-standing methods on `Store`.
- Global: `static STORE: OnceCell<Mutex<Store>>` — actually simpler:
  `static STORE: Mutex<Store>` via `Mutex::new(Store::new())` const — use
  `once_cell::sync::Lazy` if const Mutex+HashMap unavailable; HashMap::new()
  is const-stable? No. Use `Lazy<Mutex<Store>>` (crate already depends on
  once_cell). Accessor `pub fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R`.
  NOTE: harness mounts store.rs standalone — it must not depend on the
  once_cell crate there. Gate the global behind `#[cfg(not(test))]`? The
  harness builds a lib with no deps; `once_cell` unavailable. Resolution: use
  `std::sync::OnceLock` (std, stable) — dependency-free.

## Risks
- Harness has no crate deps → stick to std only (`OnceLock`, `Mutex`,
  `HashMap`). Documented above.
