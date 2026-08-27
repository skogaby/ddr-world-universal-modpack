# Plan — `services::game_audio` (Step 2, task 02)

**Status: Approved 2026-07-25** (inherited from the approved `implementation/plan.md` +
`design/detailed-design.md`; see `context.md` → "Upstream approval chain")
**Date:** 2026-07-26

## Verification approach (no test harness)

No harness exists for game-ABI code, so — per the plan's Step 2 and the feature's verification split
— each acceptance criterion is discharged either by a **log line the change itself emits** or by the
**maintainer's listening pass**. The table below is this task's substitute for a test list.

| AC | Evidence | Who |
|---|---|---|
| 1 — addresses resolve, service available | `GameAudio: initialized (se_play @ +0x…, audio_manager_global @ +0x…, xactengine2_10.dll present)` and `GameAudio started` from `lib.rs` | agent (log) |
| 2 — `init` calls no game function | source inspection: `init` does `get_address` ×3 + `GetModuleHandleA` + `transmute` only. Corroborated by a clean boot | agent |
| 3 — free slot **computed**, bank registers | `GameAudio: slot layout OK (se_normal slot 2 bank=…)`, `GameAudio: claiming free sound-bank slot 4 (of 6)`, `GameAudio: CreateInMemoryWaveBank hr=0x0`, `GameAudio: CreateSoundBank hr=0x0`, `GameAudio: bank 'asti' registered in slot 4` | agent (log) |
| 4 — slot `file_id` never written | source inspection: exactly one write, to `slot_bank_ptr`, with the reason at the site | agent |
| 5 — a cue is audible; bank survives song loads | one clap on the first judge frame of a song; a clap at the start of **every** song across a multi-song session, with `bank 'asti' registered` appearing **once** | **maintainer** (audible) + agent (log: single registration line) |
| 6 — missing bank files degrade | rename the installed `banks/` away → exactly one `GameAudio demo: bank file 'banks/tick.xwb' not found under data_mods (expected data_mods/assist_tick/banks/tick.xwb)`, no clap, game normal | agent (log) |
| 7 — corrupted sound bank degrades | flip one byte in the installed `tick.xsb`'s CRC-covered region → exactly one `GameAudio: CreateSoundBank failed hr=0x8AC70007 (…)`, no crash | agent (log) |
| 8 — no free slot degrades | force the search to find nothing (temporary local edit narrowing the scan range) → one `GameAudio: no free sound-bank slot`, nothing written | agent (log) |
| 9 — playback failure warns once | temporary local edit playing a nonexistent cue → `false` every call, exactly **one** `GameAudio: se_play(...) returned the failure sentinel` for the session | agent (log) |
| 10 — scaffolding unmistakable | source inspection: one contiguous `mod demo` block plus its single marked call site, both saying Step 3 deletes them; no detour | agent |
| 11 — build gates | `cargo check`, `cargo fmt`, `./build.sh` | agent |

AC7 and AC8 and AC9 are the negative paths the split assigns to the agent because they are visible
purely in the log. AC5's audible half is the maintainer's.

## Implementation approach

Three files touched: **new** `src/services/game_audio.rs`, plus one `pub mod` line and a doc bullet
in `src/services/mod.rs`, plus the `init` call and import in `src/lib.rs`.

### `src/services/game_audio.rs`

**Public API — exactly design §4.1**, no additions:

```
pub struct BankRequest { pub name: &'static str, pub xwb: Vec<u8>, pub xsb: Vec<u8> }
pub struct BankHandle  { slot: i32 }          // Clone + Copy, opaque
pub fn init(signatures: &SignatureStore) -> bool
pub fn is_available() -> bool
pub fn register_bank(req: BankRequest) -> Option<BankHandle>
pub fn play_cue(bank: BankHandle, cue: &CStr, pan: f32) -> bool
```

**Shape** follows `asset_loader`: `static AUDIO: Lazy<Mutex<Option<Inner>>>`, `unsafe impl Send for
Inner`, per-call lock, and an `Inner::manager()` that dereferences the *global* on every call rather
than caching the object (so a null global can never be missed).

`Inner` holds: `se_play: SePlayFn`, `manager_global: *const u8`, `named_bank_count_site: *const u8`,
and `banks: Vec<RegisteredBank { name: String, handle: BankHandle, sound_bank: *mut u8 }>`.

**Typed ABI aliases** — the one detail that would silently break:

```
type SePlayFn      = unsafe extern "system" fn(i32, *const c_char, f32) -> u32;   // pan in XMM2
type CreateBankFn  = unsafe extern "system" fn(*mut u8, *const u8, u32, u32, u32, *mut *mut u8) -> i32;
type GetCueIndexFn = unsafe extern "system" fn(*mut u8, *const c_char) -> u16;
```

**Constants** (each named, each with the offset's meaning at its definition): slot count 6, slot
array base `0x08`, stride `0x10`, `file_id` at `+0x00`, bank pointer at `+0x08`, `se_normal` slot 2,
engine pointer at `mgr+0x00`, engine vtable `+0x48`/`+0x50`, sound-bank vtable `+0x00`, cue-not-found
`0xFFFF`, play sentinel `0xFFFFFFFF`, expected named-bank count 4, engine module name.

**`init`** — addresses only, no game call:
1. `get_address` for `se_play`, `audio_manager_global`, `audio_named_bank_count_site`; any missing ⇒
   one warning naming which, return `false`.
2. `GetModuleHandleA("xactengine2_10.dll")` must be non-null — a different engine version means the
   vtable indices are unverified, so the service disables rather than calling through them.
3. Store `Inner`, log the three module-relative offsets.

**`register_bank`** — design §4.1's order, exactly, each stage logged:
1. Idempotence: a matching `name` returns the existing handle with no engine call.
2. Null-check the manager global **and** the object behind it (guard G2 — the game's own code
   dereferences it unchecked).
3. Guard G1: read the byte at `named_bank_count_site`; not `4` ⇒ decline (a build added a named bank,
   so no slot is reliably free).
4. Slot-layout sanity: the `se_normal` slot's bank pointer must be non-null — proof that both the
   layout assumption and normal boot hold. (§4.1 step 2.)
5. **Compute** the free slot: first `s` in `0..6` with `file_id == -1 && bank == null`. Never
   hard-coded. None free ⇒ decline with one warning.
6. Engine pointer from `mgr+0x00`, null-checked.
7. `Box::leak` **both** buffers — before the create calls, because the engine retains both pointers
   and they must be stable. Reason at the site.
8. `CreateInMemoryWaveBank(xwb)` **first** (the wave bank must exist and is prepared synchronously),
   then `CreateSoundBank(xsb)`. Log each HRESULT; either `< 0` ⇒ one warning carrying the HRESULT
   (with a plain-language gloss for the two codes worth naming) and decline.
9. Write **only** `slot_bank_ptr`. A comment at that single line explains that leaving `file_id` at
   `-1` is what stops the game's slot destroyer — a linear `find_if(file_id == …)` — from ever
   matching our slot, and that "fixing" it would break the feature silently.
10. Record the bank (name, handle, sound-bank pointer) and log the chosen slot.

**`play_cue`**:
1. Null-check the manager global (guard G2) — skip and warn once if null.
2. On the first play of a given cue name, resolve `GetCueIndex` through the sound-bank vtable and log
   the index at **info** (`0xFFFF` = not found). This is requirement 12's "cue index resolved"; it is
   info, not warn, so AC9's single-warning contract holds.
3. Call the **public** `se_play(slot, cue, pan)` — design §4.1 chose the public entry deliberately to
   keep the game's own AVS lock semantics; §6 records switching to the already-resolved
   `se_play_inner` as the documented mitigation if the SE mute filter turns out to veto our bank.
4. `0xFFFFFFFF` ⇒ return `false` and warn **once per session** (an `AtomicBool`), because the game
   *leaks* a cue rather than crashing on handle-table exhaustion, making the return value the only
   signal. No rate limiting here — that is FR-4 in Step 4.

**Scaffolding** — one contiguous `mod demo` at the bottom of the file, banner-commented as Step-2
scaffolding that Step 3 deletes wholesale, plus its single marked call from the tail of `init`:
- `install()` (init thread): read `banks/tick.xwb` / `banks/tick.xsb` via
  `mod_paths::find_first_modfile`, stash the bytes, `judge_hook::register_pre(Priority::Normal, …)`.
  **No detour.** File IO stays off the game thread — and this is also where design §4.2 puts the real
  mod's load, so the scaffolding rehearses the final shape.
- the callback: a one-shot `AtomicBool` that calls `register_bank` then `play_cue(b"asti")` on the
  first dispatch. Registration is idempotent, so the guard is belt-and-braces.

### `src/services/mod.rs` / `src/lib.rs`

`pub mod game_audio;` + a doc bullet in the module header's service list. In `lib.rs`, a new numbered
step `6b1` immediately after `judge_hook`, matching the surrounding `let ok = …; if ok { log_info } else { log_warn }`
shape, with a comment stating that the placement-after-judge_hook is the scaffolding's requirement
and not a real dependency.

## Risks and how they are contained

| Risk | Containment |
|---|---|
| Writing `file_id` would make the game's destroyer target our slot (silent loss of the bank mid-session) | one write, one comment at that line; the plan, the design and the research all say it; AC4 is a source-inspection criterion |
| Hard-coding slot 4 would collide silently if a build adds a fifth named bank | slot computed from the live `{file_id, bank}` pair, plus guard G1 on the named-bank count |
| `pan` declared as an integer would pass garbage | `extern "system" fn(i32, *const c_char, f32)` — the float lands in XMM2 |
| A guessed `IXACT2Cue` index would be undefined behaviour | only `+0x48`/`+0x50` on the engine and `+0x00` on the sound bank are used; no cue method at all |
| Freeing the buffers would hand the engine a dangling pointer | both leaked, before the calls, with the reason at the site |
| Calling the engine from the init thread would crash | `init` resolves addresses only; every engine call happens on the game thread at the first judge dispatch |
| Sound-bank corruption fails **silently** in the engine's own loader | the HRESULT is checked and logged here (unlike `gamemdx`'s own `soundbank_create`, which ignores it) |
| Per-tick log spam | the cue-index line is once per cue name; the failure warning is once per session |

## Maintainability notes

Every layout offset appears exactly once, as a named constant with the meaning of the field it
indexes, so a future reader can re-derive it from the research note without re-reading the
disassembly. The `unsafe` blocks are scoped to the individual raw read/write or vtable dispatch.
