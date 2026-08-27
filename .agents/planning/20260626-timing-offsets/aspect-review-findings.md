# Aspect-Review Findings — Timing Offsets

6-aspect parallel review (Opus 4.8) of the shipped feature, run 2026-06-27 after Step 10.
Files reviewed: `src/mods/timing_offsets.rs`, `src/mods/mod_menu.rs`,
`src/services/input_manager.rs`, `src/core/signatures.rs` (the timing landmark/derivation).

**Aspect ratings:** Correctness *adequate* · Concurrency *adequate* · FFI/unsafe **strong** ·
Error handling *adequate* · Conventions **strong** · Maintainability *adequate (→strong)*.

**Decision (maintainer):** address ALL findings, but in a **separate follow-up session** (this
doc + the handoff prompt are the input to that session). The feature is validated working on
the cabinet; these are hardening/cleanup, not blockers.

> ✅ **ALL 12 RESOLVED (2026-06-27 hardening session), shipped in commit `cf73d23`.** P1 (F1–F4)
> + F8, then F11 (+F9/F10/F12), then F5/F6/F7. Both `./build.sh` and `./build_win7.sh` build
> clean; no new clippy warnings in the touched files. Per-finding detail is inline below;
> maintainer redeploy confirmed no render-path regression. Notable choices:
> F4 derived a new `timing_config_map_global` signature (verified in Ghidra on both builds);
> F6 used a non-breaking default trait method `Mod::is_active()` (not a signature change) to
> avoid an 18-mod sweep; F7 was accepted-and-documented (no quiescence machinery).

Line numbers are approximate (pre-fix) — re-grep before editing.

---

## P1 — genuine bugs / safety (fix first)

### F1. `scroll_offset` not re-clamped after a visibility collapse → usize underflow
- **Where:** `mod_menu.rs` `rebuild_visible` (clamps `selected_index` but not `scroll_offset`);
  `refresh_slots` computes `visible_idx = selected_index - scroll_offset` (~`:903`).
- **Repro:** scroll down into the child scalar rows (advances `scroll_offset`), toggle the
  master OFF (children vanish, `selected_index` clamps small, `scroll_offset` stays high) →
  `selected_index - scroll_offset` underflows (usize). No panic (feeds an `as f32` multiply),
  but the cursor renders off-screen.
- **Fix:** in `rebuild_visible`, after clamping `selected_index`, also clamp `scroll_offset`
  (e.g. call `adjust_scroll` or `scroll_offset = scroll_offset.min(selected_index)`).
- Source: Correctness #4.

### F2. Lock poisoning → panic across FFI (mod_menu / input_manager `.lock().unwrap()`)
- **Where:** `mod_menu.rs` and `input_manager.rs` use `.lock().unwrap()` on `MOD_MENU_STATE` /
  `INPUT_MANAGER` throughout. Reachable from the input dispatch inside `poll()` → called from
  `wrapper_render_hook` (`widget_renderer.rs:~95`), an `extern "C"` detour **not** wrapped in
  `catch_unwind`. A single panic while a lock is held poisons it; the next frame's `.unwrap()`
  re-panics and can unwind into game code (CLAUDE.md rule #1 = UB).
- **Caveat:** largely **pre-existing** in those files; this change ADDED new `.unwrap()` lock
  sites on that path (the row-model functions, the repeat thread).
- **Fix (full):** convert render-path/FFI-reachable `MOD_MENU_STATE`/`INPUT_MANAGER` locks to
  the graceful `if let Ok(...) / let Ok(..) = .. else return` pattern that `timing_offsets.rs`
  already uses. Cheaper blanket alternative: wrap `input_manager::poll()` in `catch_unwind` at
  the `wrapper_render_hook` call site (protects ALL callbacks, not just mod_menu). Maintainer
  chose "everything" → do the graceful-lock conversion across both files (incl. pre-existing).
- Source: Concurrency #2, Error-handling #1.

### F3. Repeat-thread lifecycle: double-spawn + panic-wedge
- **Where:** `mod_menu.rs` `start_repeat_thread`/`stop_repeat_thread` (`REPEAT_THREAD_RUN` bool).
- **Issues:** (a) close→reopen within the 16ms poll window can spawn a 2nd repeat thread while
  the 1st is still alive (no stop handshake) → double-fires `activate_selected`. (b) a panic in
  the thread body (e.g. poisoned lock) silently kills it and leaves `REPEAT_THREAD_RUN` stuck
  `true` → no repeat for the rest of the process.
- **Fix:** wrap the loop body in `catch_unwind`; add a stop handshake (join handle, or a
  generation counter so a stale thread exits when it sees a newer generation). Pairs with F2
  (graceful locks make the panic path moot, but the double-spawn guard is still wanted).
- Source: Concurrency #3.

### F4. MAP_READY boot-seed fallback gap
- **Where:** `timing_offsets.rs` — `MAP_READY` is only set inside `set_int_hook`; `push_to_map`
  gates on it. If the game's boot publisher ever runs BEFORE our hook installs, the hook never
  fires for the boot write, `MAP_READY` stays false, `push_all_configured()` no-ops, and
  configured values won't apply until the next settings re-publish.
- **Reality:** works today (enable runs well before subsystem init — cabinet-validated). The
  design (R4 "Install timing") called for a post-init re-set fallback that the code can't
  currently perform (no independent way to know the map is live).
- **Fix:** derive/observe the config-map global (`DAT_1806ebcf0` analog) and let `push_to_map`
  check it directly (or set `MAP_READY` once the global is observed non-null), so the seed
  fallback can fire even if ordering is ever violated. Optional belt-and-suspenders given it's
  validated, but maintainer wants it.
- Source: Correctness #1.

---

## P2 — document / confirm (low risk; mirror existing patterns)

### F5. `IN_MODPACK_POLL` is process-global, not thread-local
- **Where:** `input_manager.rs:~99` + the set/clear around the modpack's poll. Correct only if
  `poll()` and all game-side ark-getter calls run on the **same (render) thread**. If a getter
  is ever called off-thread concurrently with the poll, suppression briefly misclassifies.
- **Status:** NOT a safety bug; mirrors the pre-existing `get_10key` pattern; all three
  reviewers agree it's fine under the single-thread assumption.
- **Fix:** add a one-line comment at the `IN_MODPACK_POLL` definition documenting the
  single-render-thread invariant; consider `thread_local!` only if that invariant could break.
- Source: Correctness #8, Concurrency #4, FFI #1.

### F6. Registry `enabled` flag vs. mod self-disable mismatch
- **Where:** `mod_trait.rs:~151` (`ModRegistry::enable` sets `entry.enabled = true`
  unconditionally after `mod_impl.enable()`), regardless of an internal self-disable.
- **Effect:** if timing-offsets self-disables (setter unresolved), the overlay master toggle
  still renders `[ON]` and the `visible_when` gating reveals the 4 child rows under an inert
  master; adjusting them stores/persists values that never apply. (`register_overlay_rows` is
  NOT reached on self-disable, so rows don't actually register — but the master row still shows
  ON.) Pre-existing registry design issue; timing-offsets is the first child-row mod to expose it.
- **Fix options:** (a) have `Mod::enable()` return a success bool the registry honors; (b) the
  mod hides its master implications when self-disabled; (c) document as a known limitation.
  Cross-mod change — scope carefully (affects all mods / the trait).
- Source: Correctness, Error-handling #2.

### F7. Unsynchronized detour teardown (SETTER_HOOK take()/disable vs game-thread call_original)
- **Where:** `timing_offsets.rs` `remove_hook()` does `take()`+`disable()` on the disable
  thread while `set_int_hook`/`call_original` may read `SETTER_HOOK` on the game thread; `retour`
  `disable()` doesn't drain in-flight callers.
- **Status:** tiny window (disable is operator-driven; setter fires rarely), but the one place
  the "valid for process lifetime" justification doesn't fully cover. Same shape exists for the
  input detours but those are install-once/never-removed so effectively immutable post-init.
- **Fix:** acknowledge in a comment; a full fix (quiescence/refcount before teardown) is
  probably overkill given the window. Confirm acceptable.
- Source: Concurrency #6.

---

## P3 — maintainability / cleanup

### F8. Stale module-header comment (MISLEADING — fix early)
- **Where:** `timing_offsets.rs:~27-30` still says "being built incrementally per plan.md…
  Step 2 = scaffold (trait + registration, inert)." The file is now fully implemented.
- **Fix:** delete/replace that note. (Quick, do it with the P1 batch.)
- Source: Maintainability verification note.

### F9. Dead code: `mod_menu::set_scalar_value`
- **Where:** `mod_menu.rs` — `pub fn set_scalar_value` has **zero callers** (the mod seeds rows
  via `register_scalar_row`'s `initial`). Its "if open, mirror into live rows" branch is dead.
- **Fix:** either wire it (so a config-driven external change reflects into an open menu) or
  delete it.
- Source: Maintainability #2, Correctness #2.

### F10. Duplicated comment block
- **Where:** `input_manager.rs:~56-62` — the "arkmdxbio2's Get* functions return
  already-debounced state…" comment is pasted verbatim twice. Pre-existing. Delete one copy.
- Source: FFI nit, Maintainability #3.

### F11. The 8 parallel const arrays in timing_offsets.rs (the bigger refactor)
- **Where:** `FIELD_KEYS`, `FIELD_KEYS_CSTR`, `FIELD_JSON_KEYS`, `FIELD_DEFAULTS`,
  `FIELD_LABELS`, `FIELD_HINTS`, `FIELD_ROW_KEYS`, `FIELD_ON_CHANGE` — all `[T; 4]` indexed by
  the same implicit `0..4`. Adding a field means editing 8 arrays + `KEY_HASHES`/`DIAG_LOGGED`;
  a mis-ordered entry is a silent, compiler-invisible bug.
- **Fix:** collapse into a single `struct FieldDef { engine_key, cstr, json_key, default,
  label, hint, row_key, on_change }` + `const FIELDS: [FieldDef; FIELD_COUNT]`, so each field's
  data is co-located. Also `FIELD_ON_CHANGE`'s 4 shim fns can collapse to
  `Arc::new(move |v| set_offset(i, v))` since `RowChangeCallback = Arc<dyn Fn(i32)>` can capture.
  `FIELD_KEYS_CSTR` is just `FIELD_KEYS` + `\0` — at minimum tie them together.
- Source: Maintainability #1, #4.

### F12. Naming nit: `parent_mod_id` vs `parent_key`
- **Where:** `mod_menu.rs` — `ScalarRowSpec.parent_mod_id` maps into `visible_when`'s generic
  `parent_key`. The mechanism is generic over any row key; the `_mod_id` suffix is slightly
  misleading for a future scalar-under-scalar parent. Rename to `parent_row_key` (or document).
- Source: Maintainability #5.

---

## Suggested fix order for the follow-up session
1. **P1 batch** (F1 scroll clamp, F2 graceful locks both files, F3 repeat-thread guard+catch_unwind,
   F4 MAP_READY fallback) + **F8 stale comment** — the safety/correctness wins.
2. **F11 FieldDef refactor** (+ F9 dead code, F10 dup comment, F12 rename) — cleanup; do after P1
   so the refactor lands on already-hardened code.
3. **F5/F6/F7 document-or-decide** — F5 is a comment; F6 is a cross-mod design call (scope it);
   F7 is an acknowledge-or-accept.
4. `cargo check` + `./build.sh` after each batch; redeploy once to re-confirm no regression
   (esp. the lock-handling change on the render path and the repeat-thread guard).
