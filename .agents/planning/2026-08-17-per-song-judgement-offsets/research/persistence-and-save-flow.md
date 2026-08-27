# Research: Persistence, save flow, and the str wire channel

## 1. Property-node APIs (libavs ordinals, resolved in `custom_options_persistence.rs:487-547`)

| Ordinal | Role | Signature as used |
|---|---|---|
| 162 | find child (`property_search`) | `fn(unused: i32, parent: *mut u8, name: *const i8) -> *mut u8` |
| 163 | add child with value (`property_node_create`) | `fn(ctx, parent, kbin_type: i32, name: *const i8, value) -> *mut u8` — value slot is **variadic**: by-value for scalars, **pointer to NUL-terminated string for str** |
| 164 | remove node (non-fatal resolve) | `fn(node) -> i32` (≥0 success) |
| 175 | get context from tree root | `fn(root) -> *mut u8` |
| 176 | read value by name (`property_node_refer`) | `fn(ctx, parent, name, kbin_type, dest, dest_size) -> i32` (<0 = not found) |

### str conventions — CONFIRMED in Ghidra (ess.dll 20260324)

ess itself round-trips the `ghost` str through these exact functions:

- **Emit** — `sys_ghostdata_save_sender` region (`FUN_180029e70`, str emit at
  `0x18002b7a4-18002b7d8`): calls the same IAT slot used for every s32/u64/bool
  add, with `R8D = 0xb` (kbin str type) and the 5th arg (`[RSP+0x20]`) =
  `LEA RCX,[staging + 0xacc]` — a **pointer to the NUL-terminated string**, not
  the bytes by value. Same function/ordinal, second typed transmute needed
  DLL-side: `fn(*mut u8, *mut u8, i32, *const i8, *const i8) -> *mut u8`.
- **Read** — `sys_ghostdata_load_receiver` (`FUN_18002d650`, decompiled):
  `XCnbrep70000af(ctx, node, "ghost", 0xb, dest_buffer, 0x2001)` — identical
  shape to the s32 read; dest is a byte buffer, 6th arg is its capacity,
  negative return = not found. (ess uses 0x2001 for a 0x2000-byte payload —
  capacity includes the NUL.) Overflow behavior (truncate vs fail) unverified;
  mitigation: size our read buffer above the client-side soft cap.

kbin type ids match the DLL's own kbin table (`avs_layeredfs/kbin/types.rs`):
s32 = 6, u64 = 9, str = 11 (0xb), bool = 0x34.

### Editing the built tree (belt-and-braces `<timing_music>` fix)

No set-value-in-place ordinal is resolved, but remove + re-create works with
what exists: 162-find → 164-remove (proven by `strip_league_node`,
`custom_options_persistence.rs:940-961`) → 163-add type 6 with the stock value
(proven by `emit_network_children`). The re-added node lands at the end of
`<option>`'s children; bemani-buddy reads by name so ordering is immaterial.

## 2. Save-pipeline ordering — DEFINITIVE

**At `save_sender_trampoline` entry, the option values are already fixed.**
A PlayerWork memory restore inside the trampoline does NOT affect the outgoing
save. Evidence (`.agents/planning/20260610-suppress-score-submission/research/score-submission-re.md`):

1. `ReflectSavePlayerData` (gamemdx) is a ~10 KB marshaller that fills a
   per-side ess staging buffer (`DAT_1804cff.. + side*0xbed8`); an **async
   poller later** ships the buffer via ess `sys_playerdata_save_sender`.
2. The sender's input is the staging buffer (`savedata = *(job+0x10)`), never
   PlayerWork — ess is a frozen generic library with no PlayerWork knowledge.
3. The shipped trampoline already reads `savekind`/`playside` from that buffer
   *before* `original.call` (cabinet-validated for score_guard's whole history)
   — the buffer is populated pre-trampoline.
4. The XML tree does NOT exist at trampoline entry; it is built inside
   `original.call`. Post-call tree edits (strip_league / emit_network_children)
   are the only trampoline-time levers.

**Consequence:** the memory restore is a **scene-timing** problem. The
trampoline can only host the post-call tree fix (162/164/163 above).

## 3. Scene timing of savekind=2 and the restore point

- Song end: scene 28 (GAMEPLAY) → 0-idx 29 (post-song loader) → 0-idx 30
  (ResultSequence). The per-stage `SavePlayerDataActor(side, 2, stage)` fires
  in ResultSequence's first frames — an entire loader scene after leaving 28.
- `scene_manager` callbacks fire synchronously inside the `createNextSequence`
  detour, before the next scene object is even constructed — the quick-logout
  sanitiser relies on exactly this property at scene 34.
- **Restore gate must be `prev == GAMEPLAY (28)`, not `next == 29`**: redirects
  (quick restart/fail fallbacks 29→28 / 29→24) rewrite `next` before callbacks
  fire; fast paths exit 28→27 / 28→24. Gating on `prev == 28` fires on every
  exit shape.
- Quick restart/fail fast paths and redirected fallbacks skip ResultSequence
  entirely → **no savekind-2 save**. The full natural tail fires it normally.
  In-place restart (`song_reset`) never leaves scene 28 → no save; an override
  surviving the reset is correct behavior (same song).
- savekind=3 (logout, scene 34 EAM_EXIT) also marshals `/option`; the
  `prev == 28` restore has already run by then in every reachable path, and the
  post-call tree fix covers any leak.

## 4. Load-side str application

- `load_receiver_trampoline` cannot resolve side at receive time (PlayerWork
  +0x18 populates only after the load completes) — hence the deferral pattern.
- The existing `PENDING_LOADS` buffer is s32-shaped
  (`PendingLoad { ddrcode: i32, values: Vec<(String, i32)> }`); a str payload
  needs a **parallel buffer** keyed by ddrcode, drained by the same SONG_SELECT
  entry callback via the reusable `side_from_ddrcode` helper. Direct in-file
  precedent: `PENDING_RATE_RESETS` (a second ddrcode-keyed deferred buffer).
- ddrcode read: `*(job+0x18)+0x48` in the receiver.
- The DLL's kbin parser (`avs_layeredfs/kbin/`) is NOT usable at the receiver
  seam (the trampoline sees the parsed AVS property tree, not raw kbin bytes);
  ordinal 176 with type 11 is the correct path.

## 5. bemani-buddy backend (from orientation research)

- Native option field is `timing_music` — parsed on save (playdata.rs:689) for
  savekinds 1/2/3 and echoed on load (playdata.rs:329). Confirms the clobber
  hazard.
- Add-a-field anatomy (from commit 072bcf8, 9 files): migration + model field +
  DAO (macro/SET/binds) + protocol struct (with `skip_serializing_if`) + JSON
  model + handler save/load/new-player lines + tests + `.sqlx` regen.
- kbin `str` nodes are u32-length-prefixed — no protocol size limit; `TEXT`
  column (64 KiB) is the practical bound. Precedent: `scores.ghost TEXT`.
- Known desync: `models/ddr_world/playdata_3.json` is missing the 015 field
  (`mod_training_progress_pos`) that was hand-edited into `playdata_3.rs` —
  backfill when touching these files.
- Hard rule: nullable, no default, field omitted (never empty) when NULL —
  un-hooked-client safety.
