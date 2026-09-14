# Task: Extract the foot-panel swap into `services/foot_panel_swap` (autoplay becomes a client)

## Description

Move the `judgeNotes` pre/post foot-panel swap, the stock `AutoFootPanel` object and its three
signature dependencies out of `src/mods/autoplay.rs` into a new shared service
`src/services/foot_panel_swap/` that owns a per-side controller `Controller { Off, Perfect,
Bot }`. Autoplay becomes a thin client (`set_perfect(side, on)`); the `Bot` variant is declared
and arbitrated now (Bot > Perfect > Off) and wired to a real panel object in a later plan step.
Autoplay's observable behaviour must not change.

## Background

Today `src/mods/autoplay.rs` registers the judge_hook callbacks at pre `Priority::Late` / post
`Priority::Early`, allocates a 0x40-byte buffer with the stock `AutoFootPanel` vtable, and in the
pre callback stashes `*(actor+fp_off)`, writes its object into the slot and calls the game's
`AutoFootPanel::update(obj, actor+0xB0, *(actor+0x168), music_count)`; the post callback restores
the stash. The Multiplayer Bot needs the same seam for a second kind of panel object, and the
judge dispatcher requires ONE owner for the swap (same-priority order is registration order and
must not be relied on). Design §4.1/§4.2 specify the service and the slimmed autoplay.

Two latent details to fix while moving:
- The stock object is 0x58 bytes on 20260721+ (0x40 on 20250805); autoplay under-allocates.
  Allocate `PANEL_OBJECT_SIZE = 0x58`.
- `autoplay.rs` names `actor+0x168` `NOTE_COUNT`; the RE (`update(this, &results, cur_beat, mc)`)
  shows it is the current beat position. Name it `ACTOR_CUR_BEAT`.

## Reference Documentation

**Required:**
- Design: `.agents/planning/2026-09-13-multiplayer-bot/design/detailed-design.md` — §4.1
  (service API + internals), §4.2 (slimmed autoplay), §5.2 (object sizes), §6 (error handling).

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-13-multiplayer-bot/research/bot-controller-re.md` §1–§2 (the
  `AutoFootPanel` vtable/`update` contract the Perfect controller preserves).
- `.agents/planning/2026-09-13-multiplayer-bot/research/autoplay-internals.md` (anatomy of the
  code being moved).
- `src/services/judge_hook.rs` (dispatcher API: `register_pre`/`register_post`/`unregister`,
  `foot_panel_offset()`, `is_available()`).
- `src/mods/two_player_bpl_mode/logic.rs` (the pure-module + host-harness convention the new
  `layout.rs` follows).

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. New module `src/services/foot_panel_swap/` declared as `pub mod foot_panel_swap;` in
   `src/services/mod.rs`, with:
   - `layout.rs` — PURE (no `crate::` imports, no `unsafe`): `pub enum Controller { Off, Perfect,
     Bot }` (`Copy, Eq, Debug`), `pub fn effective_controller(bot_armed: bool, perfect: bool) ->
     Controller`, constants `PANEL_OBJECT_SIZE: usize = 0x58`, `ACTOR_SIDE: usize = 0x84`,
     `ACTOR_RESULTS_BEGIN: usize = 0xB0`, `ACTOR_CUR_BEAT: usize = 0x168`, and a `#[cfg(test)]`
     module.
   - `mod.rs` — engine-facing: `pub fn init(signatures: &SignatureStore) -> bool`,
     `pub fn is_available() -> bool`, `pub fn set_perfect(side: usize, on: bool)`,
     `pub type BotFillFn = fn(side: usize, actor: *mut u8, music_count: i32, out: &mut
     BotPanelFlags)`, `pub struct BotPanelFlags { pub is_held: [u8; 8], pub was_just_pressed:
     [u8; 8], pub event_mc: [i32; 8] }` (may live in `layout.rs`), `pub fn arm_bot(side: usize,
     fill: BotFillFn) -> bool`, `pub fn disarm_bot(side: usize)`, `pub fn controller(side: usize)
     -> Controller`. Re-export `Controller` from the module root.
2. `init` resolves `judge_notes`, `auto_foot_panel_vtable`, `auto_foot_panel_update` via
   `signatures.get_address` (soft — WARN + `false` on a miss, never `require_address`), requires
   `judge_hook::foot_panel_offset()`, allocates the stock-shaped object with
   `memory::alloc_zeroed(PANEL_OBJECT_SIZE)` and writes the vtable pointer at `+0x00`, and
   registers `judge_hook::register_pre(Priority::Late, swap_in)` +
   `judge_hook::register_post(Priority::Early, swap_out)` exactly once. Both registrations must
   succeed or the service reports unavailable (unregister the one that succeeded).
3. `swap_in` / `swap_out` are the moved autoplay callbacks: `side = (*(actor + ACTOR_SIDE) == 1)
   as usize`; `match controller(side)`: `Off` ⇒ return; `Perfect` ⇒ stash `*(actor+fp_off)` into
   `ORIGINAL_FOOT_PANEL[side]`, write the stock object, call `update(obj, actor +
   ACTOR_RESULTS_BEGIN, *(actor + ACTOR_CUR_BEAT), music_count)`; `Bot` ⇒ for THIS task, log one
   rate-limited WARN ("bot controller armed but no panel object yet") and behave as `Off` (the
   real branch lands in plan Step 3). `swap_out` restores a non-null stash for that side (as
   today). Callbacks are panic-free: no indexing with an unmasked value, no `unwrap`.
4. Arbitration state: `BOT_ARMED: [AtomicBool; 2]`, `PERFECT: [AtomicBool; 2]`, `BOT_FILL:
   [AtomicPtr / Mutex<Option<BotFillFn>>; 2]` (implementer's choice; must be lock-free or
   `try_lock` on the judge path). `arm_bot` while `PERFECT[side]` ⇒ one INFO ("bot controller
   takes precedence over autoplay on side N"). `disarm_bot` clears the fill fn and the flag.
   `set_perfect` never touches `BOT_ARMED`.
5. `src/lib.rs`: call `foot_panel_swap::init(&signatures)` immediately after step 6b
   (`judge_hook::init`), logging `FootPanelSwap started` / `FootPanelSwap unavailable -- autoplay
   and multiplayer-bot inert`, plus `profiling::tick("foot_panel_swap")` in the established
   style.
6. `src/mods/autoplay.rs`: remove `AUTO_PANEL`, `AUTO_UPDATE`, `FOOT_PANEL_OFFSET`,
   `ORIGINAL_FOOT_PANEL`, `NOTE_LIST_PTR`, `NOTE_COUNT`, `AUTO_PANEL_SIZE`,
   `ACTOR_PLAY_SIDE_OFFSET`, `AutoUpdateFn`, both callbacks, the `judge_notes_addr` /
   `pre_handle` / `post_handle` fields and the judge registrations/unregistrations;
   `required_signatures() -> &[]`; `init` returns `foot_panel_swap::is_available()` (WARN naming
   the reason on `false`); `autoplay_on_change` stores nothing locally except what the watermark
   needs — call `foot_panel_swap::set_perfect(side, enabled)` and keep
   `score_guard::set_autoplay_taint(side, enabled)`; keep the fail-closed
   `score_guard::is_available()` gate, the option row and the watermark thread unchanged;
   `side_autoplay_engaged(side)` becomes `foot_panel_swap::controller(side) ==
   Controller::Perfect && stage_records::side_entered(side).unwrap_or(true)`; `disable` calls
   `set_perfect(0, false)` / `set_perfect(1, false)` and stops the watermark as today. Update the
   module doc comment to describe the client role.
7. judge_hook priorities stay pre `Late` / post `Early` — they are load-bearing for
   `per_song_judgement_offsets` (Early) and `power_user_statistics` (Normal).
8. Repo conventions: logging only via `log_info!`/`log_warn!`; narrow `unsafe` blocks;
   `static mut` only for the detour-style slots that must be reachable from plain `fn`
   callbacks (`std::ptr::addr_of!` reads); no absolute/local paths in any comment.

## Dependencies

- `src/services/judge_hook.rs` — dispatcher (unchanged).
- `src/core/memory.rs` — `alloc_zeroed`, `write_ptr`, `read_i32`, `read_ptr`.
- `src/core/signatures.rs` — `SignatureStore::get_address` (unchanged; the three signatures
  already exist).
- `src/services/score_guard.rs`, `src/services/stage_records.rs` — autoplay's existing gates
  (unchanged).

## Implementation Approach

1. Create `src/services/foot_panel_swap/layout.rs` with the enum, arbitration fn, constants and
   tests; add the `pub mod` line in `src/services/mod.rs` (alphabetical position).
2. Create `src/services/foot_panel_swap/mod.rs`: statics, `init`, the two callbacks, the public
   API; port the callback bodies from `autoplay.rs` line-for-line, substituting the named
   constants and the controller match.
3. Wire `init` into `src/lib.rs` after `judge_hook::init`.
4. Slim `src/mods/autoplay.rs` per requirement 6; fix imports (`Controller`, drop unused
   `judge_hook`/`memory`/`CallbackHandle`/`Priority`).
5. `cargo check --target x86_64-pc-windows-msvc`; `cargo fmt`; `./build.sh`; run the pure tests
   through a temp-crate `#[path]` mount of `layout.rs` (the `scripts/validate_two_player_bpl.sh`
   shape — a throwaway invocation is fine; the permanent harness arrives in plan Step 2).
6. Grep for stale references (`AUTO_PANEL`, `autoplay_pre_judge`, `NOTE_COUNT`) and for
   `/(Users|home)/` in the diff.

## Acceptance Criteria

1. **Service initialises and owns the swap**
   - Given a boot where `judge_notes`, `auto_foot_panel_vtable`, `auto_foot_panel_update` resolve
     and `judge_hook::foot_panel_offset()` is `Some`
   - When `lib.rs` runs `foot_panel_swap::init`
   - Then the log shows `FootPanelSwap started`, `is_available()` is true, and exactly one
     pre/post pair is registered on the judge dispatcher (grep: no `register_pre`/`register_post`
     calls remain in `autoplay.rs`).

2. **Autoplay behaviour unchanged (Perfect controller)**
   - Given the human turns the `autoplay` option ON
   - When a song plays
   - Then every note is judged Marvelous, the "Autoplay Enabled" watermark bounces, the log shows
     `Autoplay: side=N ON`, and no per-frame log lines are emitted by the swap.

3. **Autoplay OFF is silent**
   - Given both sides' `autoplay` OFF
   - When a song plays
   - Then `controller(side) == Off` for both sides, the callbacks return immediately, no stash
     is written, and no swap-related log line appears.

4. **Arbitration**
   - Given `PERFECT[side]` is set
   - When `arm_bot(side, f)` is called
   - Then `controller(side) == Bot`, one INFO names the precedence, and after `disarm_bot(side)`
     `controller(side) == Perfect` again. `effective_controller` truth table is host-tested
     (Bot > Perfect > Off, all four input combinations).

5. **Fail-open on a missing prerequisite**
   - Given any of the three signatures or the foot-panel offset is missing
   - When `init` runs
   - Then it logs one WARN naming the missing item, returns `false`, `is_available()` is false,
     and `AutoplayMod::init` returns `false` (the mod is skipped with the registry's standard
     "failed to initialize" line) — no panic, no `require_address`.

6. **Layout constants and object size**
   - Given `layout.rs`
   - When its tests run on the host
   - Then `PANEL_OBJECT_SIZE == 0x58` (≥ the largest stock object), `ACTOR_SIDE == 0x84`,
     `ACTOR_RESULTS_BEGIN == 0xB0`, `ACTOR_CUR_BEAT == 0x168`.

7. **Build gates**
   - Given the completed change
   - When `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`, `./build.sh` run
   - Then all are clean; `git grep -nE "/(Users|home)/[^/ ]+/" -- . ':!target'` adds no new hits.

## Metadata
- **Complexity**: Medium
- **Labels**: refactor, service-extraction, judge-hook, autoplay, multiplayer-bot
- **Required Skills**: Rust (unsafe FFI hooking patterns), this repo's judge_hook/memory
  conventions, reading Ghidra-derived layout notes
- **Generated By**: code-task-generator 2026-09-13
- **Source Plan**: `.agents/planning/2026-09-13-multiplayer-bot/implementation/plan.md`
- **Plan Step**: Step 1: Extract the foot-panel swap into `services/foot_panel_swap` (autoplay becomes a client)
