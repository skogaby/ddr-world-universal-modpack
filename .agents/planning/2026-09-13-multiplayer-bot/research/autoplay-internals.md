# Autoplay Internals (Step 2 orientation sub-report)

Source: codebase read + disassembly of the four supported `gamemdx` builds. Addresses are
file-relative to `0x180000000`. Repo paths cite `file:line`.

## The mod

| Piece | Path |
|---|---|
| Mod | `src/mods/autoplay.rs` |
| Shared judge dispatcher | `src/services/judge_hook.rs` |
| Score taint | `src/services/score_guard.rs` |
| Signatures | `src/core/signatures.rs:4306-4338` (`auto_foot_panel_vtable`, `auto_foot_panel_update`, `judge_notes`) |
| Prior RE | `docs/autoplay.md` (build 20260324) |

- Row `autoplay` (`RegisterSpec::bool_toggle` ⇒ `PersistMode::Full`, wire `mod_autoplay`) — `autoplay.rs:405-411`.
- Per-side `AUTOPLAY_ENABLED: [AtomicBool; 2]` (`:52`), read LIVE on every `judgeNotes` call (`:95`, `:127`) — no scene latch.
- Side from `*(actor+0x84)` (`:66, :89`); doubles: left slot owns both pads (`:62-65`).
- Refuses to enable without `score_guard::is_available()` (`:380-385`).
- Taint: `autoplay_on_change` → `score_guard::set_autoplay_taint(side, enabled)` (`:74`; `score_guard.rs:691-696`). Level-written, not cleared by `reset_song_taint` (`score_guard.rs:764-774`). `is_stage_suppressed(side)` ORs it (`score_guard.rs:812-822`).
- Watermark armed on `scene == GAMEPLAY ∧ an ENTERED side has autoplay on` (`:208-211`, uses `stage_records::side_entered`).

## Mechanism (per judge frame)

Pre-callback `Priority::Late` (`:388`) / post-callback `Priority::Early` (`:389`):

1. Pre (`autoplay_pre_judge`, `:83-116`): stash `*(actor+fp_offset)` → `ORIGINAL_FOOT_PANEL[side]`; write mod-owned `AUTO_PANEL` into the slot (`:104`); call `AutoFootPanel::update(AUTO_PANEL, actor+0xB0, *(actor+0x168), music_count)` (`:106-114`).
2. Original `judgeNotes` dispatches every panel query through the swapped object's vtable.
3. Post (`:118-139`): restore the pointer.

`AUTO_PANEL` = ONE process-wide `static mut` (`:45`), `VirtualAlloc`'d `AUTO_PANEL_SIZE = 0x40` (`:40, :360`), stock vtable ptr at +0 (`:365`).

Foot-panel slot: `0x270` (≤20260224) / `0x278` (20260324+), detected by scanning `judgeNotes` (`judge_hook.rs:36-40, 177-194`).

## GamePlayActor fields

| Offset | Meaning |
|---|---|
| +0x84 | play side |
| +0x88 | play style (1 = DOUBLE) |
| +0xB0/+0xB8 | Results vector begin/end (0x40 stride) — `src/types/game_note.rs:96-142` |
| +0x168 | current beat position (passed as r8 to `update`; used for the freeze-hold path) |
| +0x1DC | live combo |
| +0x1E8 | byte ≠0 ⇒ misses not marked |
| +0x1E9 | byte ≠0 ⇒ `judge_submit` skipped |
| +0x270/+0x278 | `IFootPanel*` slot |

Result record (0x40): `+0x00` note ptr, `+0x08` judge timestamp (−1 unjudged), `+0x0C` grade (0xFF invalid), `+0x10` visible, `+0x11` missed. Note record (0x60): `+0x04 beat_count`, `+0x08 music_count`, `+0x1C..+0x3C state[8]` (1 TRG, 4 REP), `+0x3C..+0x5C length[8]`.

## What the game's autoplay actually is

**Not a judge flag.** `AutoFootPanel` is an `IFootPanel` with a 7-slot vtable (`[1] update, [2] wasJustPressed, [3] isHeld, [5] getPressAge, [6] consumePress`).

`AutoFootPanel::update(this, &results, cur_beat, mc)` (20260825 `+0x22D00`; 20260721 `+0x22D30`; 20260224 `+0x226A0`; 20250805 `+0x22820`):
- zeroes `isHeld[8]` (+0x08) and `wasJustPressed[8]` (+0x10);
- walks Results; breaks when `mc < note.mc − 8`;
- freeze path: `cur_beat < note.beat + max(length[])` ⇒ `wasJustPressed[i]=1` for `state[i] ≥ 2`;
- unjudged entries: shock ⇒ `wasJustPressed[i] = (state[i] != 1)` (steps on everything except the shock); else for each `state[i] ∈ {1,4}`: `isHeld[i]=1; wasJustPressed[i]=1; pressTime[i] = <stamp>`.

**Stamp by build family:**

| Builds | Clock | Stamp | `pressTime` layout |
|---|---|---|---|
| 20250805, 20260224, 20260324 | `WINMM!timeGetSystemTime.ms` | `now` (not back-dated) | `dword[8]` at +0x18..+0x38 (getter `sub eax,[rdi+rbx*4+0x18]`); object 0x40 |
| 20260721, 20260825 | libavs ordinal 45 (game tick `T`) | `now − (mc − note.mc)` — event lands exactly on `note.mc` | `qword[8]` at +0x18..+0x58 (getter `sub eax,[rdi+rbx*8+0x18]`); object **0x58** |

`getPressAge` returns `(u32)(clock() − pressTime[panel])`. Judge (20260825 `+0x5EECC..` / `+0x5F067..`): `event = mc − age; diff = |note.mc − event|` vs per-grade window. Miss marked at `note.mc + 160 ms < mc` when `+0x1E8 == 0`; too-early break at `mc < note.mc − 0x104`.

⇒ On new builds autoplay is exactly on-time by construction. On old builds the event is the frame's `mc` (diff ∈ [−8, +frame]) — still Marvelous at 60 Hz.

**Bug on record:** `AUTO_PANEL_SIZE = 0x40` is 0x18 bytes short on 20260721+; harmless only because the allocation is page-granular VirtualAlloc.

## Seams for imperfection

**A — mutate the mod-owned buffer after `update`, before `judgeNotes` (RECOMMENDED).** In `autoplay_pre_judge` after `update_fn(...)` (`:106-114`):
- late by d: `pressTime[i] += d`; early: `−= d` (integer ms in either clock);
- miss: clear `isHeld[i]` and `wasJustPressed[i]` every frame until the note ages out;
- realistic late: also suppress the flags until `mc ≥ note.mc + d − 8`;
- decisions per (note ptr, panel), stable across frames (walk Results with `types::game_note::for_each_result`, `game_note.rs:153-175`);
- derive the stride (4 vs 8) from the getter's SIB byte, publish via `publish_value`;
- grow `AUTO_PANEL_SIZE` to ≥ 0x58;
- keep non-miss jitter inside the Boo window (an out-of-window press can match the NEXT note on that panel — `consumePress` zeroes `pressTime[i]` only after a successful judgement);
- per-side state (one shared buffer works because sides are judged sequentially on one thread, but jitter state is per side).
- Ordering: autoplay's pre is `Priority::Late`; a separate mod at the same priority would be registration-order dependent (`judge_hook.rs:11-12, 44-46`) ⇒ put the controller INSIDE the swap owner or expose a post-update mutator hook. Feasibility HIGH.

**B — vtable clone wrapping slot 5 (`getPressAge`).** Precedent `two_player_bpl_mode/logic.rs:134` `clone_vtable_image`. Early/late only; cannot produce misses. Subset of A.

**C — `judge_submit` tap rewrite** (`power_user_statistics/data_feed.rs:279-424`). Fakes a grade after the judge decided; inconsistent internal state. NOT recommended.

**D — real-input path via the ark IO vtable injection** (`input_manager.rs`): Up/Down/Left/Right impl slots `+0x310..0x328` (2026 arks) / `0x2F0..0x308` (20250805), derived per boot from the `arkMDXGetPanel*` export wrappers (`:317-394`), installed lazily (`:1390-1407`, `:921-1028`). Impl shape `u64 impl(this, player_i32, *state_u8, *trigger_u8, *press_ts_u64, *release_ts_u64)` (`:314-315`); body `panel_impl_body` (`:455-522`) ORs a provider's held bit, synthesizes the rising edge, backfills the ord-45 stamp only when the ark's reads 0. Single-slot provider owned by SMX (`:164-182`). To be ms-accurate the detour would have to OVERWRITE `*press_ts` on the injected edge (the game's own back-dating trick); provider needs a richer return; freeze holds need sustained `held`. Feasibility MEDIUM — works with autoplay OFF and for a side with no flag at all, but it is a bigger change to a cabinet-proven seam. The judge side (A) is strictly simpler for a bot whose actor exists.

**E — `+0x1E8/+0x1E9`**: suppression only, not a timing lever.

## Traps

- `learnings.md:731-750` — option values outlive the player; gate on `side_entered`.
- `learnings.md:1239-1250` — panel getter u64 out-args are ord-45 timestamps.
- Judge-hook contract: same-priority order = registration order, never rely on it; ≤ ~1 ms hot path; callbacks are `catch_unwind`-wrapped (`judge_hook.rs:129-131, 144-146`).
- The AutoFootPanel age is already mc-domain (`.agents/scratchpad/2026-09-08-frame-timing/clock-rate-correction/context.md:36-37`) — jitter edits in the panel's own clock stay consistent with the `song_rate` clock stub.

## Open items

- Exact boundary build of the `timeGetSystemTime/dword` → `ord45/qword` change (between 20260324 and 20260721); derive the stride from the SIB byte rather than a build table.
- The `0x22`/`0x43` constants and the per-grade window table in `judgeNotes` (`+0x5EE7B`, `+0x5F074`) bound how far an in-window jitter may go before re-association.
