# Task: Extra-stage guard — `extra_stage_grant` signature + `GenericDetour`

## Description

Implement design R10 / D21 (§4.9, §4.10, A.4): the game's extra-stage grant iterates every
side with `PlayerWork+0x4 != 0` and requires each to have AAA'd — so a low-level bot that
does not AAA would block the human's extra stage. Add the `extra_stage_grant` prologue AOB to
`src/core/signatures.rs` (soft consumer) and a small `GenericDetour` in
`src/mods/multiplayer_bot/extra_stage_guard.rs` that clears the bot's entered byte around the
original call while an impersonation is active, then restores it. Fail-open: a missing AOB
leaves the stock rule in place with one WARN.

## Background

`FUN_1801ddcd0(int arg)` on 20260825 (research §8): gated on `GameWork+0x59 == 0`, `arg == 0`,
`GameWork+0x70 == 0`, `GameWork+0x4 != 1`, `max_stage + 1 == 3`; then for every entered side
requires `record[0]+0x50 >= 0xF`, `PlayerWork+0x1710 == 0`, gauge ∈ {0, 0xC},
`record[0]+0x270 != 7`; on success `GameWork+0x59 = 1`. Called from `ResultSequence::onUpdate`
case `0x16` (results window-out) when the stage counter is 0 — i.e. INSIDE the play window
while `PW[bot]+0x4 == 1`. The prologue AOB is unique on 20250805 (`0x1801c6970`), 20260224
(`0x1801ca7e0`) and 20260825 (`0x1801ddcd0`); the offline sweep attests 20260721. Step 4
exposed `impersonation::active_bot_side()` as a lock-free read for this callback.

## Reference Documentation

**Required:**
- Design: `.agents/planning/2026-09-13-multiplayer-bot/design/detailed-design.md` — §4.9
  (callback shape), §4.10 (signature table), A.4 (AOB + semantics), §6 (missing AOB row),
  §7.2 (sweep), §7.3 item 6.

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-13-multiplayer-bot/research/versus-impersonation-re.md` §8.
- `src/mods/announcer_mute.rs` — the `static mut Option<GenericDetour<_>>` +
  `hooks::install_enabled` + `addr_of!` callback + passthrough-on-disable shape.
- `src/core/signatures.rs` — `SignatureDefinition` entries (the `// ── 2-Player BPL Mode`
  block at the end of `SIGNATURES` is the style reference); `get_address`.
- `scripts/sig_harness/report.py` — consumer classification (`get_address(...)` = soft).
- `src/mods/multiplayer_bot/impersonation.rs` (`active_bot_side`), `src/services/stage_records.rs`
  (`player_work`), `src/core/memory.rs` (`is_readable`, `read_u8`, `write_u8`).

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. `src/core/signatures.rs`: append to `SIGNATURES` (new `// ── Multiplayer Bot` block after the
   BPL block) `SignatureDefinition { name: "extra_stage_grant", pattern: "48 83 EC 38 48 8B 05
   ?? ?? ?? ?? 48 8B 10 80 7A 59 00 0F 85 ?? ?? ?? ?? 85 C9 0F 85 ?? ?? ?? ?? 48 83 7A 70 00 0F 85
   ?? ?? ?? ?? 83 7A 04 01 0F 84", description: … }` — the description names the function
   (`FUN_1801ddcd0` on 20260825), its gates, the caller, the consumer (`multiplayer_bot`'s
   extra-stage guard detours the MATCH address; nothing is read at `match+N`), and the
   per-build addresses. NOT in any `required_signatures`.
2. `src/mods/multiplayer_bot/extra_stage_guard.rs`:
   - `type GrantFn = unsafe extern "C" fn(i32);` `static mut GRANT_HOOK: Option<GenericDetour<GrantFn>>`;
     `static INSTALLED: AtomicBool`, `static ENABLED: AtomicBool` (passthrough flag).
   - `pub fn init(signatures: &SignatureStore) -> bool`: `signatures.get_address("extra_stage_grant")`
     → store the target in an `AtomicPtr`; `None` ⇒ one WARN `MultiplayerBot: extra_stage_grant
     signature missing -- the extra-stage grant will consider the bot (stock rule)` and `false`.
   - `pub fn enable()`: sets `ENABLED`; installs the detour ONCE (`hooks::install_enabled`) when a
     target is known; install failure ⇒ one WARN, stays uninstalled (stock rule).
   - `pub fn disable()`: clears `ENABLED` (detour stays installed as a passthrough).
   - `pub fn is_installed() -> bool`.
   - `unsafe extern "C" fn grant_hook(arg: i32)`: `let Some(hook) = (&*addr_of!(GRANT_HOOK)).as_ref()
     else { return };` if `ENABLED` and `impersonation::active_bot_side()` is `Some(bot)` and
     `stage_records::player_work(bot)` is `Some(pw)` with `memory::is_readable(pw, 0x8)` and
     `read_u8(pw+0x4) != 0`: write `0`, call `hook.call(arg)` through a scope guard that writes
     the original byte back on drop (so an unwinding original cannot leave it cleared), one INFO
     `MultiplayerBot: extra-stage grant evaluated without the bot (side N)`; otherwise
     `hook.call(arg)`. Body panic-free (no `unwrap`/indexing).
3. `mod.rs`: `pub mod extra_stage_guard;`; `init` calls `extra_stage_guard::init(ctx.signatures)`
   (its result does NOT gate `CAPABLE`); `enable` calls `extra_stage_guard::enable()` after
   `register_rows()`; `disable` calls `extra_stage_guard::disable()`. Module doc: Step 5 state.
4. Sweep: `./scripts/validate_signatures.sh ~/Desktop/ddr_modules` must be ALL GREEN with
   `extra_stage_grant` hitting exactly once on all four builds (the harness prints `[+]` per build;
   `report.py` must classify the consumer as soft). `shape_diff.py` not required.

## Dependencies

- Step 4 `impersonation::active_bot_side()`; `core::hooks::install_enabled`; `retour::GenericDetour`;
  `stage_records::player_work`; `core::memory`.
- Offline: `~/Desktop/ddr_modules` (20250805 / 20260224 / 20260721 / 20260825 gamemdx builds).

## Implementation Approach

1. Signature entry → `./scripts/validate_signatures.sh ~/Desktop/ddr_modules` (attest 4/4 before
   writing the consumer).
2. `extra_stage_guard.rs` + `mod.rs` wiring.
3. `cargo check` → `cargo fmt` → `./build.sh` → `./scripts/validate_multiplayer_bot.sh` → sweep again
   (consumer graph now includes the `get_address`).

## Acceptance Criteria

1. **Signature attested on all builds** — Given the sweep runs over the four builds, When it
   finishes, Then `extra_stage_grant` is `[+]` on every build, the report shows no HARD/soft loss,
   exit 0.
2. **Guard clears only the bot, only while active** — Given an impersonation is active on side N and
   the grant fn is called, When the detour runs, Then `PW[N]+0x4` is 0 during the original and back
   to its prior value after, one INFO; Given no impersonation (or the mod disabled), When called,
   Then the original runs untouched and nothing is logged.
3. **Fail-open** — Given the AOB is missing or the install fails, When the mod inits/enables, Then one
   WARN, the mod still enables and the bot session works (stock extra-stage rule).
4. **Cabinet (maintainer, §7.3 item 6)** — On a 3-stage setting with a low-level bot, the human who
   AAAs still gets EXTRA STAGE; the guard INFO appears on the results window-out.
5. **Build gates** — `cargo check` clean, `cargo fmt`, `./build.sh` clean, harness green, sweep green,
   no local paths.

## Metadata
- **Complexity**: Low
- **Labels**: signatures, detour, multiplayer-bot
- **Required Skills**: Rust unsafe FFI, this repo's `signatures`/`hooks` conventions, the offline sweep
- **Generated By**: code-task-generator 2026-09-13
- **Source Plan**: `.agents/planning/2026-09-13-multiplayer-bot/implementation/plan.md`
- **Plan Step**: Step 5: Extra-stage guard — `extra_stage_grant` signature + detour, signature sweep green
