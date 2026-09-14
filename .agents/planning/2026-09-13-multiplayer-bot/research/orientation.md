# Orientation: Multiplayer Bot

Blind-spot pass over the codebase + RE notes before any decision is taken. Every claim
cites `file:line` (repo-relative). Ghidra addresses are file-relative to
`gamemdx.dll` `0x180000000`, build noted where relevant.

## 1. What the idea actually requires

Reduced to mechanisms, "play a 2P versus session against a bot" is FOUR independent
problems, each with a different existing building block:

| # | Problem | Existing building block | Gap |
|---|---------|-------------------------|-----|
| P1 | An option row (bool parent + 1–10 scalar child) in the 9-options menu and the 0-0-0 PLAYER SETTINGS tab | `custom_options` framework (`RegisterSpec::bool_toggle` / `::scalar`, `ShowWhen::Equals`) — exact precedent `assist_tick` + `assist_tick_volume` (`src/mods/assist_tick.rs:1091-1133`, `:1531-1558`) | Textures via `scripts/option_strings.py` + `gen_option_labels.py`; nothing new in the framework |
| P2 | Make the game spawn a SECOND `GamePlayActor` (P2 lane, HUD, results pane) in a session where only P1 entered | Nothing does this today. Closest analogues: the attract demo (`DemoPlaySequence`, 0-idx 16) runs TWO actors with nobody entered (`docs/premium_free_stale_record_bug.md:217-219`); `two_player_bpl_mode` flips `GameWork+0` scope-guarded around a stock call (`src/mods/two_player_bpl_mode/mod.rs:710-757`) | **The main RE problem.** See §3 |
| P3 | Drive the P2 actor as a computer player | `autoplay` mod's foot-panel swap — per-actor, keyed on `actor+0x84` (`src/mods/autoplay.rs:83-139`); the game's own `AutoFootPanel::update` synthesizes presses | Needs (a) a per-side controller so P1 can be human while P2 autoplays, (b) imperfection |
| P4 | Imperfection scaled 1–10 (from "likely fails" to "may MFC") | The DLL OWNS the `AutoFootPanel` buffer it hands the judge, so after `update()` it can mutate `pressTime[panel]` (early/late) or clear `isHeld/wasJustPressed` (miss) — no new detour (`autoplay.rs:104-114`) | Build-dependent `pressTime` stride (dword vs qword, §4); the buffer is 0x18 bytes too small on 20260721+ (`autoplay.rs:40` vs the 0x58-byte object) |

Plus a fifth, cross-cutting one: **P5 — keep the bot invisible to scoring/credits/network**
(the bot side must never save, never expire a credit, never break P1's save).

## 2. The option layer (P1) — settled territory

- Registration shape, `PersistMode` table, `ShowWhen` mechanics and the texture checklist
  are fully documented in `research/option-framework.md` (Step 2 subagent output). Nothing
  here changes the idea.
- The maintainer's model "Center Arrows (1P Only)" (`src/mods/center_arrows_single.rs`)
  does NOT hide its row in 2P — "1P only" is a **runtime gate** (`single_player &&
  side == active_side && style == SINGLE`, `:338-358`) over presence read from
  `PlayerWork+0x4`. That is the right shape here too: the row is always visible; the
  effect engages only when exactly one side is entered and style is SINGLE.
- Label atlas is flushed ONCE at boot (`src/lib.rs:600-607`): a mod that ships
  default-OFF and is enabled from the menu has no in-game label until next launch
  (overlay row unaffected). Matters for the default-ON/OFF decision.

## 3. Making 1P look like 2P (P2) — the territory that changes the idea

### 3a. What decides "two GamePlayActors"?

Evidence chain (all from existing notes):

1. `createNextSequence` case 0x1d (0-idx 28) allocates the DPS; the stage loader
   (0-idx 27) hands the DPS ctor a **per-side 16-byte struct `{entered, is_main,
   is_double, pad, i32 difficulty, u64}`** with `difficulty = FUN_1801e89b0(wrapper) =
   PlayerWork+0x5C` (`docs/quick_restart_fail_speedup_research.md:857-861`;
   `.agents/learnings/learnings.md:1041-1044`).
2. `DancePlaySequence::onSetup` iterates "for each present side" (`docs/split_ssq_research.md:190-193`);
   `onUpdate` case 1 creates the GamePlayActors (`docs/in_shop_battle_local_versus_research.md:64-78`).
3. Cabinet-observed: a solo P1 session has NO P2 GamePlayActor (`learnings.md:740-741`).
4. `GameWork+0` (1 = versus) is consumed by HUD/results **layout selectors**, not by actor
   creation (`src/mods/two_player_bpl_mode/logic.rs:13`; `in_shop_battle…:108,141,148`;
   `docs/quick_logout_research.md:349`).

⇒ Actor count is decided by the loader's per-side `entered` byte; **`PlayerWork+0x4`
is the presumed source but the read is not pinned in any note** (unknown U1).

### 3b. Two candidate architectures

**A. "Fake entry" — set `PlayerWork[1]+0x4 = 1` at song select.** The game then runs
its native versus path everywhere. Problems: versus song select needs TWO cursors and
BOTH confirms (`FUN_180114ef0` versus confirm vs `FUN_18010d9e0` solo,
`docs/premium_free_stale_record_bug.md:253-262`); entry-flow/exit sequences iterate
entered sides — `EAmExitRootSequence::onSetup` calls `arkExpireCredit` for every side
with `PW+0x4 && PW+0x8 >= 0` (`quick_logout_research.md:280-283`) — a credit-accounting
hazard; `versus_mirror` would engage and mirror P1's options into the bot.

**B. "Windowed impersonation" — flip only for the play window.** At the song-select
COMMIT (scene 25 exit), write `PW[1]+0x4 = 1`, `GameWork+0 = 1`, mirror P1's chart
choice into P2 (`PW+0x50` style, `PW+0x5C` difficulty, `record[stage]` header), and
restore everything at the WaitSequence → select-loader transition (0-idx 31 → 24, i.e.
after TOTAL results are impossible… see U6). The game natively builds the two-actor DPS,
the versus HUD (`main_tag` layout), the two results panes. Song select stays solo
(one cursor, one confirm). Payment kind `PW[1]+0x8` untouched (stays whatever a
non-entered side has — needs confirming it is `< 0`, U4). This matches the maintainer's
wording "temporarily think we're playing a 2P session" and the BPL mod's scope-guard
precedent.

**C. "Loader-struct injection" — never touch `PW+0x4`.** Detour/patch the loader's
per-side struct build so P2 reads `entered=1`, flip `GameWork+0` for the window. Avoids
every `PW+0x4` reader, but every OTHER `PW+0x4` reader in the play window (READY panel
per side — `premium_free_stale_record_bug.md:247-248`; TotalResult name string;
results pane visibility, U7) would treat P2 as absent → likely half-drawn P2 UI. More
detours for less coverage.

Recommendation going into the register: **B**, pending RE on U1–U7. A is rejected on
credit/UX grounds; C is a fallback if B trips a `PW+0x4` reader with side effects.

### 3c. The results screen

- `ResultSequence` (0-idx 30) creates **two `WindowActor`s (one per side) unconditionally**;
  each tab carries `+0x130 side`, `+0x134 versus`, `+0x148 record side`, `+0x14C stage`
  (`.agents/planning/2026-08-29-s-marvelous-judgement/research/display-side-re.md:34-46`).
  The DLL's own results detours bail on `side_entered(side) == Some(false)`
  (`src/mods/s_marvelous/results_score.rs:105-108`) — under B they would populate the
  bot pane too (desirable: the S-Marv sheet/graph would show for both).
- What HIDES the non-entered pane in stock 1P (`tab+0x134` from `GameWork+0`, or
  `PW+0x4`) is unknown (U7) — determines whether B alone makes the P2 pane appear.
- Tab kind 1 "Simple" (`loop_guest`) vs 6 "Details" (`loop_registered`)
  (`src/mods/s_marvelous/results_score.rs:4-6`) — keyed on `PW+0x5`/`PW+0x18` (U7). A bot
  is a guest ⇒ simple pane, which is fine.
- `TotalResultSequence` (0-idx 32): reads `GameWork+0` ("both sides visible") and
  `PW+0x4` (name string), `PW+0x5 == 0` zeroes deltas (`quick_logout_research.md:345-370`).
  The restore point decides whether the bot appears on TOTAL results (D-decision).

### 3d. Scoring / network / credits (P5)

- Per-stage saves: `SavePlayerDataActor(side, stage)` is built per side after results
  (`.agents/planning/20260610-suppress-score-submission/research/score-submission-re.md:171-186`),
  but the ess `save_sender` fires "once per carded-in side" (`:46-59`) and a side that
  never entered stays at ark scene 0 (`quick_logout_research.md:339-344`). Which gate
  actually suppresses a cardless side's `savekind==2` is undecoded (U5).
- **DLL backstop already exists**: a side whose actor autoplays is tainted
  (`autoplay.rs:72-74` → `score_guard::set_autoplay_taint`) ⇒ `savekind==2` suppressed,
  `savekind==3` sanitised (`src/services/custom_options_persistence.rs:970-1007`). The bot
  side can ride the same taint bit — and P1's side must NOT be tainted (P1 played for
  real).
- Open: does a versus session change what the marshal writes for P1 (any "vs" field)?
  Not covered by any note (U8).
- `PW[1]+0x18` ddrcode is 0 for a guest; the persistence service's ddrcode→side routing
  never matches side 1 (`custom_options_persistence.rs:1464-1471`) — good, no accidental
  P2 load/save routing.

## 4. Driving the bot (P3/P4) — what the game's autoplay really is

Verified by the Step 2 subagent disassembling all four supported builds (details in
`research/autoplay-internals.md`):

- Autoplay is **not a judge flag**. `AutoFootPanel` is an `IFootPanel` implementation
  whose `update(this, &results, cur_beat, mc)` synthesizes `isHeld[8]` (+0x08),
  `wasJustPressed[8]` (+0x10) and `pressTime[8]` (+0x18…) for every unjudged note within
  8 ms; the stock judge then computes `event = mc − getPressAge(panel)` and grades
  `|note.mc − event|` exactly as for a human.
- **The stamp differs by build family** — the crux for jitter:
  - 20250805 / 20260224 (and 20260324): `pressTime = timeGetSystemTime().ms` (NOT
    back-dated), `dword[8]` at +0x18..+0x38 (object 0x40).
  - 20260721 / 20260825: `pressTime = ord45_now − (mc − note.mc)` (back-dated so the
    event lands exactly on `note.mc`), `qword[8]` at +0x18..+0x58 (object **0x58**).
  - `autoplay.rs:40` allocates 0x40 — 0x18 short on new builds, harmless only because
    `memory::alloc_zeroed` is page-granular VirtualAlloc. Must be fixed regardless.
- **Seam for imperfection (no new detour):** in the pre-judge callback right after
  `update()`, per (note, panel): `pressTime[i] += d` (late) / `−= d` (early) in ms;
  miss = clear `isHeld[i]`/`wasJustPressed[i]` every frame until the note ages out at
  `note.mc + 160 ms` (the judge then marks the miss and submits 0x102D). Realistic
  "late" additionally needs the press suppressed until `mc ≥ note.mc + d − 8`,
  otherwise the note is judged on the stock frame. Decisions must be stable across
  frames (keyed on note ptr + panel) and stay inside the Boo window when not missing,
  or a stray press re-associates with the NEXT note on that panel.
- **One-detour rule / ordering:** the autoplay mod owns the pre-`Late` / post-`Early`
  judge callbacks. A second subscriber at the same priority would depend on
  registration order, which `judge_hook` forbids relying on (`src/services/judge_hook.rs:11-12`).
  ⇒ Promote the foot-panel swap into a shared service with a per-side controller
  (`Off | Perfect | Imperfect{level, seed}`) consumed by both the autoplay mod and the
  bot. The AutoFootPanel buffer must become per-side (today one process-wide static,
  `autoplay.rs:45` — sides are judged sequentially on one thread so it *works*, but
  jitter state is per side).
- Stride derivation: from the `getPressAge` getter's SIB byte (`rbx*4` vs `rbx*8`),
  published via `SignatureStore::publish_value` — the same convention as
  `player_option_offset`.

## 5. Interactions with other mods (found, not yet decided)

| Mod | Interaction under architecture B |
|-----|-----------------------------------|
| `two_player_bpl_mode` | Gates on `GameWork+0 == 1 ∧ both side_entered ∧ event ∉ {1,2} ∧ !course ∧ network idle` (`logic.rs`) — a bot session satisfies all ⇒ the BPL score-margin frame appears vs the bot. Probably desirable; decide |
| `versus_mirror` | Engages at the first SONG_SELECT frame with both entered — B flips at song-select EXIT, so it never engages. Good (nothing to mirror into a bot) |
| `song_rate::lifecycle::classify_scene26` | Scene 26 classification reads both entered flags — under B they read (true,true) ⇒ P1 governs, `participant_mask 0b11`. Fine, as long as the flip precedes scene 26 |
| `autoplay` | Must keep working for P1 independently; both use the shared controller |
| `s_marvelous` results detours / `power_user_statistics` | Populate/show for both sides once `side_entered(1)` is true — fine |
| `quick_restart_or_fail` | Restart = fresh DPS re-reads PW ⇒ bot respawns. Fail predicate unaffected. Stale-record virginise runs for both sides — must not fight the bot's record header write |
| `premium_free` | `effective_freeze` re-resolved on scene change with P1 governing — unaffected |
| `training_mode` | 2P training exists (P1 governs, mask 0b11). Bot + training = a shared-timeline session with a bot; harmless but pointless — recommend excluding |
| `score_guard` | Bot side needs the autoplay taint; P1 side must stay clean |

## 6. Traps already on record (from `.agents/learnings/learnings.md`)

- `:731-750` per-side option values OUTLIVE the player — gate on `side_entered`.
- `:1025-1064` the stage bump is a save-integrity boundary; two writers of chart identity
  (`PW+0x5C` unguarded cursor vs guarded record prepare).
- `:1313-1319` `PlayerWork+0x54` is the COMMITTED song, not the wheel highlight.
- `:1239-1250` panel-getter u64 out-args are ord-45 timestamps.
- Never hardcode PlayerWork record base / Option offset / GamePlayActor ≥0x208 fields.
- Probe `memory::is_readable` before dereferencing pointers read from unpinned offsets.

## 7. Unknowns that need Ghidra (I have `gamemdx_20260825` + `20260224` + `20250805` open)

| ID | Question | Why it matters |
|----|----------|----------------|
| U1 | Where the stage loader (0-idx 27 / case 0x1d) reads the per-side `entered` byte for the DPS ctor — is it `PW+0x4`? | Decides whether B works at all |
| U2 | Who writes `PW+0x4`, and what else is written at that site (`PW+0x5`, `PW+0x8`) | Tells us what a "real" entered side carries that our flip does not |
| U3 | What `SelectStyleSequence`'s commit computes "(2 players)" from; whether anything RE-derives `GameWork+0` from the flags later | Whether writing `GameWork+0 = 1` at the flip is sufficient/safe |
| U4 | Value of `PW+0x8` on a never-entered side; every `arkExpireCredit`/PASELI reader of `PW+0x4` in scenes 26–31 | Credit-accounting safety |
| U5 | Which gate stops a cardless side's `savekind==2`; whether `FUN_1800b6670`'s per-side loop is gated on `PW+0x4` or `PW+0x5` | Whether the bot side could emit a save request (backstop = taint) |
| U6 | The song-select commit for the non-entered side: what does it write into `PW[1]` `record[0]` / `+0x54` / `+0x5C`? | What our flip must fix up for the bot to play P1's chart |
| U7 | What hides the second results pane in 1P (`tab+0x134`, `PW+0x4`, or `PW+0x5`); how the DPS/HUD decides P2's READY panel and lane | Whether B alone yields a complete P2 UI |
| U8 | Does a versus session alter P1's save marshal output (`GameWork+0` readers in `ReflectSavePlayerData`)? | P1 score integrity |
| U9 | Attract demo (`DemoPlaySequence`) — how it obtains a two-actor DPS with nobody entered | Closest analogue; may reveal a cleaner seam than the loader |
| U10 | `AutoFootPanel` pressTime stride boundary build + judge grade windows table | Jitter implementation |
| U11 | `ddr::player::Option` layout — which fields are cosmetic (speed mod, arrow color) vs judge-affecting (`+0x24` timing) | Whether to copy P1's options into the bot lane |

## 8. Proposed sequence

1. **Clarify now** the user-visible decisions that do not depend on the RE (option
   naming/persistence/default, bot identity on screen, difficulty model, which sessions
   are eligible, what happens at TOTAL results, interaction with BPL frame, score policy).
2. **Research sprint** on U1–U11 in Ghidra (20260825 primary, cross-check 20260224 +
   20250805 for the layout-boundary builds), writing `research/versus-impersonation-re.md`
   and `research/bot-controller-re.md`.
3. **Finalize mechanism decisions** (architecture B vs C, flip/restore points, jitter
   model constants) and close the register.
4. Design → plan.
