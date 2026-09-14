# Multiplayer Bot — Detailed Design

Status: Approved 2026-09-13 (revised 2026-09-13 — no-Boo judge correction, one-judgement-per-frame, gauge/scoring facts, `tools/bot_sim` replaces the synthetic harness; revised 2026-09-14 — §4.6 skill model retuned to a lean + two-regime jitter shape after the first cabinet playtest, targets set by the maintainer in conversation)

## 1. Overview

**Multiplayer Bot** lets a single player on a DanceDanceRevolution World cabinet play a
2-player VERSUS session against a computer-controlled opponent. The player enables
**BOT OPPONENT (1P ONLY)** in the in-game options menu (or the 0-0-0 overlay menu's
PLAYER SETTINGS tab) and picks a **BOT LEVEL** from 1 to 10. From the next song's start
through its stage results screen, the game runs as a genuine 2P versus session: the
empty pad's lane, HUD, name plate (`BOT LV7`), gauge, judgements, combo, and results pane
all exist and are driven by the game's own code; the bot's play quality is controlled by
the level — a level-10 bot has a real chance of a Marvelous Full Combo, a level-1 bot has
a real chance of failing the song.

The design rests on two facts established by reverse engineering the game binary:

1. **The game decides "how many players" from one byte per side.** The stage loader copies
   `PlayerWork[side]+0x4` (the "entered" flag) into the `DancePlaySequence` constructor's
   per-side struct, and the sequence creates a `GamePlayActor` for every side whose byte is
   non-zero. The versus HUD/results layouts key off a second word, `GameWork+0x0`. Flipping
   those two values for the duration of one song (song-select commit → results exit) makes
   the game build the entire 2P experience natively. The game already models a cardless
   entered side (its BPL "guest join" helper produces exactly that PlayerWork shape).
2. **The game's autoplay is an input object, not a flag.** `AutoFootPanel` implements the
   `IFootPanel` interface the judge queries every frame (`isHeld`, `wasJustPressed`,
   `getPressAge`, `consumePress`), and the judge grades the synthesized presses exactly as
   it grades a human's. A DLL-owned `IFootPanel` whose `getPressAge` returns
   `current_music_count − planned_event` places every graded event precisely where the bot's
   skill model decided — early, late, or missed — on every supported game build.

Everything else (gauge drain, death, combo, judgement effects, results, graphs, BPL frame)
is stock game behaviour operating on a second, computer-driven player.

## 2. Detailed Requirements

Consolidated from the accepted decision register. "Human" = the entered side; "bot" = the
other side.

### 2.1 Functional

| ID | Requirement |
|----|-------------|
| R1 | A bool option **`bot_opponent`** ("BOT OPPONENT (1P ONLY)") and a child scalar option **`bot_opponent_level`** ("BOT LEVEL", 1..=10, step 1, default 5) are registered per player, shown in both the in-game options menu and the overlay PLAYER SETTINGS tab; the child is visible only while the parent is ON (`ShowWhen::Equals`). Both persist with `PersistMode::Full` (wire `mod_bot_opponent` / `mod_bot_opponent_level`, JSON cache). Rows are listed in `option_menu_settings` under the training header right after `autoplay`. |
| R2 | The bot **engages** for a song iff, at the song-select → stage transition: exactly one side is entered, the session style is SINGLE (`GameWork+0x4 == 0`), it is not a course (`GameWork+0x70 == 0`), the event mode is 0, the session is not already versus (`GameWork+0x0 == 0`), and the entered side's `bot_opponent` is ON. The bot takes the **non-entered side**. The rows stay visible in every session; the gate is runtime-only (the "(1P Only)" label carries the constraint). |
| R3 | The bot plays **the human's exact chart**: same song, style, and difficulty. |
| R4 | While engaged, the game presents a 2P versus session: two `GamePlayActor`s, versus HUD layout, READY panel for both sides, both results panes on the stage results screen (0-idx scene 30). |
| R5 | The bot's name plate reads **`BOT LV<n>`** wherever the game draws a player name (HUD, BPL frame, results). |
| R6 | The bot's play quality follows a **level-indexed skill model** (§4.6): a per-song early/late lean, a two-regime (pocket / loose) Gaussian jitter, a per-note miss probability and a per-song form factor, with a fresh random seed every play (quick restarts and in-place resets re-roll), decisions fixed per note for the note's life. Freeze arrows are held whenever their head was hit; shock arrows are always avoided. |
| R7 | The bot uses the **human's lane options** (speed, arrow skin, cut options, etc.) except **gauge type = NORMAL**. |
| R8 | The impersonation **ends** at the first scene change out of the play window {26, 27, 28, 29, 30}: TOTAL RESULTS, the stage-bump wait, EAM exit, and any quick-fail/limbo path run as a stock 1P session. |
| R9 | The **human's saves are untouched**: their per-stage and logout saves proceed as stock. The bot side must never reach the server: its side carries the autoplay taint (per-stage save suppressed, logout save sanitised) on top of the game's own no-card gating. |
| R10 | The **extra-stage grant** must not consider the bot: the game's grant check iterates entered sides; the bot's entered flag is cleared around that check. If the check cannot be located, the stock rule applies and a WARN says so. |
| R11 | The **2P BPL Mode** frame (if that mod is enabled) is allowed to engage against the bot — no special casing. |
| R12 | Option edits apply at the **next song-select commit**. Turning the parent OFF makes the next song plain 1P. |
| R13 | Quick restart re-reads the flags (bot respawns with a fresh seed); quick fail follows R8's restore. |
| R14 | Never engage in the attract demo, course/dan modes, event modes, doubles, or real 2P. |

### 2.2 Non-functional

| ID | Requirement |
|----|-------------|
| N1 | **Zero new detours for the impersonation and the controller.** The only new hook is the optional extra-stage guard (R10). The judge-side swap reuses the existing shared `judge_hook` dispatcher slots the autoplay mod already occupies. |
| N2 | **No hardcoded addresses.** Every game object is reached through already-derived services (`stage_records`, `player_option_offset`, `judge_hook::foot_panel_offset`, RTTI vtables) or one new AOB (`extra_stage_grant`). |
| N3 | **Fail-open everywhere.** Any missing derivation, unreadable pointer, or refused gate ⇒ the song is a stock 1P song, one WARN. Hook callbacks are panic-free (no `unwrap`/index panics; `catch_unwind` where the dispatcher does not already provide it). |
| N4 | **Hot-path budget**: the per-frame filler must stay well under 1 ms (the stock `update` walks the same result vector; ours walks a bounded window from a cursor). |
| N5 | **Build coverage**: 20250805, 20260224, 20260721, 20260825 — the controller must not depend on the AutoFootPanel's press-time clock or stride, which differ between the 2025/early-2026 builds and the mid-2026 builds. |
| N6 | The autoplay mod keeps working unchanged for the human (including when the human also has `autoplay` ON), and the bot's side ignores a cached `autoplay = ON` (bot controller wins). |

### 2.3 Assumptions

- `PlayerWork+0x4/+0x5/+0x8/+0xC/+0x18/+0x1C/+0x50/+0x54/+0x5C` and `GameWork+0x0/+0x4/+0x8/+0xC/+0x70/+0xD0` have identical offsets on all supported builds (only the record base and the Option offset vary, and both are already derived).
- A never-entered side's `PlayerWork` has `+0x8 = −1` (payment none) and `+0x1C = 0` (never saves) — the shape `PlayerWork::reset` produces.
- The sound-manager "pan SEs by side" byte (`+0x20C4`) is cosmetic; the design treats it as optional polish.
- The human's per-stage save will carry the wire scalars `/data/mode` = `/data/battle_mode` = 1 during a bot session (the marshal copies `GameWork+0x0`). Neither known server (bemani-buddy, bemaniutils) reads them; accepted as a documented, honest side effect.

## 3. Architecture Overview

```mermaid
flowchart LR
  subgraph Options["custom_options (existing)"]
    ROW1[bot_opponent bool]
    ROW2[bot_opponent_level 1..10]
  end

  subgraph Mod["mods/multiplayer_bot"]
    LC[mod.rs<br/>lifecycle + rows]
    IMP[impersonation.rs<br/>flip / restore state machine]
    ELG[eligibility.rs PURE]
    SKL[skill.rs PURE<br/>level → σ, p_miss, per-note plans]
    PLN[planner.rs PURE<br/>frame planner: notes+plans+mc → flags/events]
    FIL[filler.rs<br/>reads GamePlayActor results, calls planner]
    XSG[extra_stage_guard.rs<br/>GenericDetour on extra_stage_grant]
  end

  subgraph Svc["services/foot_panel_swap (NEW, extracted from autoplay)"]
    SW[judge pre(Late)/post(Early) swap]
    CTL[per-side Controller: Off / Perfect / Bot]
    OBJ[Perfect: stock AutoFootPanel + stock update<br/>Bot: BotFootPanel + cloned vtable]
  end

  subgraph Game["gamemdx.dll"]
    SEL[Song select commit]
    LDR[Stage loader → DPS ctor<br/>reads PlayerWork+0x4]
    GPA[GamePlayActor ×2<br/>judgeNotes → IFootPanel]
    RES[ResultSequence<br/>GameWork+0 == 1 → 2 panes]
    XS[extra-stage grant]
  end

  ROW1 & ROW2 --> LC
  LC --> IMP --> ELG
  IMP -- "scene 25→26: PW[bot]+0x4=1, GameWork+0=1,<br/>mirror chart, copy Option, name" --> LDR
  IMP -- "scene ∉ 26..30: restore" --> RES
  LDR --> GPA
  GPA -- "vtable calls" --> OBJ
  SW -- "swap *(actor+fp_off)" --> GPA
  CTL --> OBJ
  FIL --> PLN --> SKL
  FIL -- "flags + event_mc per frame" --> OBJ
  LC -- "arm_bot(side)" --> CTL
  XSG -. "clear PW[bot]+0x4 around original" .-> XS
  GPA --> RES
```

Three layers:

1. **`foot_panel_swap` service** (new, extracted from the autoplay mod): the single owner of
   the `judgeNotes` pre/post callbacks that swap a side's `IFootPanel` pointer. It holds a
   per-side controller (`Off | Perfect | Bot`) and the two kinds of DLL-owned panel objects.
   The autoplay mod becomes a thin client (`set_perfect(side, on)`); the bot mod is the
   second client (`arm_bot(side, fill_fn)`). Bot outranks Perfect for a side.
2. **`multiplayer_bot` mod**: option rows, the impersonation state machine (driven by
   `scene_manager::on_scene_change`), the skill model, the per-frame filler, and the
   extra-stage guard.
3. **Pure cores** (`eligibility`, `skill`, `planner`) with no engine dependencies, host-tested.

### 3.1 Session timeline

```mermaid
sequenceDiagram
  participant P as Player
  participant G as Game (scenes)
  participant M as multiplayer_bot
  participant S as foot_panel_swap
  participant J as judgeNotes (per side, per frame)

  P->>G: song select: pick song/diff, confirm (scene 25)
  G->>G: commit writes both sides' record headers (bot diff = stale cursor)
  G-->>M: scene change 25 → 26
  M->>M: eligibility(entered, style, course, event, versus, option) → bot = !human
  M->>G: PW[bot]: +0x50/+0x54/+0x5C ← human; rec[bot]+0x04/+0x08 ← human;<br/>Option ← human (gauge=NORMAL); name "BOT LV7"; +0x4 = 1; GameWork+0 = 1
  M->>S: arm_bot(bot, fill)
  M->>M: taint(bot) on; new seed
  G->>G: scene 27 READY panels (both), scene 28 loader: DPS struct[bot].entered = 1
  G->>G: DPS creates GamePlayActor[human], GamePlayActor[bot]
  loop every frame, each actor
    J->>S: pre(Late): side = actor+0x84
    alt side == bot
      S->>M: fill(actor, mc) → flags + event_mc
      S->>J: *(actor+fp_off) = BotFootPanel
    else side == human with autoplay ON
      S->>J: *(actor+fp_off) = stock AutoFootPanel (after stock update)
    end
    J->>J: original judgeNotes: isHeld/wasJustPressed/getPressAge via vtable → grade
    J->>S: post(Early): restore original pointer
  end
  G->>G: song end → result commit for both sides → scene 29 → 30 (two panes)
  G-->>M: (results window-out) extra_stage_grant(0)
  M->>G: guard: PW[bot]+0x4 = 0 around the original, then 1
  G-->>M: scene change 30 → 31
  M->>G: restore: PW[bot]+0x4 = 0, name bytes, GameWork+0 = 0
  M->>S: disarm_bot(bot); taint(bot) off
  G->>G: scene 31 stage bump → 24 → 25 (stock 1P) or 32 TOTAL RESULTS (stock 1P)
```

## 4. Components and Interfaces

### 4.1 `services/foot_panel_swap.rs` (new)

Extracted from the autoplay mod. Owns everything the game sees through the foot-panel slot.

```rust
pub enum Controller { Off, Perfect, Bot }

/// Fills the bot panel for one judge frame. Called on the game thread inside the
/// pre-judge callback for a side whose controller is `Bot`. Must be panic-free.
pub type BotFillFn = fn(side: usize, actor: *mut u8, music_count: i32, out: &mut BotPanelFlags);

pub fn init(signatures: &SignatureStore) -> bool;   // needs judge_notes, auto_foot_panel_vtable,
                                                     // auto_foot_panel_update, judge_hook::foot_panel_offset()
pub fn is_available() -> bool;
pub fn set_perfect(side: usize, on: bool);           // autoplay's request (ignored while Bot is armed)
pub fn arm_bot(side: usize, fill: BotFillFn) -> bool;
pub fn disarm_bot(side: usize);
pub fn controller(side: usize) -> Controller;        // effective controller (Bot > Perfect > Off)
```

Internals:

- Registers `register_pre(Priority::Late, swap_in)` and `register_post(Priority::Early,
  swap_out)` once at `init` (the exact slots autoplay uses today, so
  `per_song_judgement_offsets` (Early) and `power_user_statistics` (Normal) keep their
  order). Callbacks read `side = (*(actor+0x84) == 1) as usize`.
- `swap_in`: match `controller(side)`:
  - `Off` → nothing.
  - `Perfect` → stash `*(actor+fp_off)`, write the shared stock `AutoFootPanel` object,
    call the game's `AutoFootPanel::update(obj, actor+0xB0, *(actor+0x168), mc)` — today's
    autoplay behaviour, unchanged. The object is allocated at **0x58 bytes** (fixes the
    latent 0x40 under-allocation on 20260721+).
  - `Bot` → stash, `CURRENT_MC[side] = mc`, `fill(side, actor, mc, &mut flags)`, copy
    `flags` into `BOT_PANEL[side]` (`is_held`, `was_just_pressed`, `event_mc`), write
    `BOT_PANEL[side]` into the slot.
- `swap_out`: restore the stashed pointer if non-null (as today).
- **Bot vtable**: a 7-slot clone built once at `init` from the RTTI-resolved
  `auto_foot_panel_vtable`, COL copied to `[-1]` (the `two_player_bpl_mode` /
  `custom_options` vtable-clone shape): slots 0–4 copied verbatim (2/3 are the byte-array
  readers at `+0x10`/`+0x08`, identical on every build; 4 is a thunk that tail-calls slot 5
  through *our* table), **slot 5 = `bot_get_press_age`**, **slot 6 = `bot_consume_press`**:
  ```rust
  unsafe extern "C" fn bot_get_press_age(this: *mut BotFootPanel, panel: i32) -> i32 {
      // event = mc − age  ⇒  age = mc − event
      CURRENT_MC[side_of(this)] − (*this).event_mc[panel as usize & 7]
  }
  unsafe extern "C" fn bot_consume_press(this: *mut BotFootPanel, panel: i32) {
      (*this).event_mc[panel as usize & 7] = 0;
  }
  ```
  `side_of(this)` compares `this` against the two static objects. Arguments are masked to
  0..7 so a hostile `panel` can never index out of bounds.
- Arbitration: `controller(side) = if BOT_ARMED[side] { Bot } else if PERFECT[side] { Perfect } else { Off }`.
  When `arm_bot` is called while `PERFECT[side]` is set, log one INFO ("bot controller
  takes precedence over autoplay on side N").

### 4.2 `mods/autoplay.rs` (slimmed)

Keeps: option row `autoplay`, `autoplay_on_change` (→ `foot_panel_swap::set_perfect(side,
v != 0)` + `score_guard::set_autoplay_taint`), the `score_guard::is_available()` fail-closed
gate, the watermark. Loses: the `AUTO_PANEL`/`AUTO_UPDATE` statics, the swap callbacks, the
judge registrations, and the three `required_signatures` (now owned by the service —
autoplay's `init` returns false when `foot_panel_swap::is_available()` is false).
Watermark condition becomes `foot_panel_swap::controller(side) == Perfect &&
stage_records::side_entered(side).unwrap_or(true)` so a bot session never shows "Autoplay
Enabled" unless the human has it on.

### 4.3 `mods/multiplayer_bot/mod.rs`

`MultiplayerBotMod` (id `multiplayer-bot`, name "Multiplayer Bot", default ON; not in
`DEFAULT_OFF_MODS`).

- `required_signatures()` → `&[]` (graceful; everything is checked at `init`).
- `init`: requires `foot_panel_swap::is_available()`, `stage_records::is_available()`,
  `stage_records::player_option_offset().is_some()`, `scene_manager::is_available()`,
  `score_guard::is_available()`; resolves `extra_stage_grant` (optional, WARN if missing).
- `enable`: register rows (parent on `custom_options::is_available()`, child on
  `row_injection_available()`; `Duplicate` = re-enable → reseed atomics from `get_value`);
  register the scene callback; subscribe `song_reset::on_song_reset` (re-roll); install the
  extra-stage guard detour (once).
- `disable`: `set_option_available` both rows false; if impersonation Active → restore;
  remove the scene callback; guard stays installed as a passthrough flag.
- `is_active` → `true` when `init` succeeded (the mod CAN work).

Option callbacks store into `OPTION_ON: [AtomicBool; 2]` and `LEVEL: [AtomicI32; 2]`
(clamped 1..=10; `persist_transform` load-side clamps too).

### 4.4 `mods/multiplayer_bot/eligibility.rs` (pure)

```rust
pub struct Inputs { pub entered: [bool; 2], pub style: i32, pub course: bool,
                    pub event_mode: i32, pub versus: i32, pub option_on: [bool; 2],
                    pub level: [i32; 2] }
pub enum Refusal { NotExactlyOneEntered, NotSingle, Course, EventMode, AlreadyVersus, OptionOff }
pub struct Plan { pub human: usize, pub bot: usize, pub level: u8 }
pub fn evaluate(i: &Inputs) -> Result<Plan, Refusal>;
```

### 4.5 `mods/multiplayer_bot/impersonation.rs`

State machine driven from the scene callback `(prev, next)`:

```rust
enum State { Idle, Active { bot: usize, snap: Snapshot } }
struct Snapshot { entered_byte: u8, name: [u8; 9], versus_word: i32 }
const PLAY_WINDOW: [i32; 5] = [26, 27, 28, 29, 30];
```

- `on_scene_change(prev, next)`:
  - `Idle` ∧ `prev == 25` ∧ `next ∈ {26, 27, 28}` → gather `Inputs` from `stage_records`
    (`side_entered`, `game_work()+0x4/+0x0/+0x70`, `event_mode`) and the option atomics;
    `evaluate` → on `Ok(plan)`: `apply(plan)`; on `Err(r)` when the entered side's option is
    ON: one INFO naming the refusal.
  - `Active` ∧ `next ∉ PLAY_WINDOW` → `restore()`.
  - `Active` ∧ `next == 28` (GAMEPLAY entry, incl. quick-restart re-entry) → tell the filler
    to reset plans/cursor (new seed).
- `apply(plan)` (all writes probed with `memory::is_readable` first; any failure ⇒ undo what
  was written, WARN, stay `Idle`):
  1. `pw_h = player_work(human)`, `pw_b = player_work(bot)`, `stage = stage_counter()`,
     `rec_h/rec_b = stage_record(side, stage)`.
  2. Snapshot `*(pw_b+0x4)`, `pw_b+0xC..+0x15`, `*(gw+0x0)`.
  3. Mirror chart identity: `pw_b+0x50 ← pw_h+0x50`, `+0x54 ← +0x54`, `+0x5C ← +0x5C`;
     `rec_b+0x04 ← rec_h+0x04`, `rec_b+0x08 ← rec_h+0x08` (only if `rec_b+0x00 == rec_h+0x00`
     — the commit already prepared both records for this song; otherwise WARN and refuse).
  4. Copy the human's Option fields `+0x08..=0x6C` (0x68 bytes) into the bot's Option at
     `pw + player_option_offset()`; write `Option+0x18 = 0` (NORMAL gauge). Never copy the
     vtable pointer at `+0x00`.
  5. `pw_b+0xC = "BOT LV<n>\0"` (8 chars max: `BOT LV10` fits).
  6. `*(pw_b+0x4) = 1`; `*(gw+0x0) = 1`.
  7. `score_guard::set_autoplay_taint(bot, true)`; `foot_panel_swap::arm_bot(bot, filler::fill)`;
     `filler::start_song(bot, level, seed)`.
  8. INFO: `multiplayer-bot: side N impersonated as "BOT LVn" (σ=…ms, p_miss=…%, seed=…)`.
- `restore()`: `*(pw_b+0x4) = snap.entered_byte`, name bytes back, `*(gw+0x0) =
  snap.versus_word`; `disarm_bot`; taint off; INFO with the bot's grade tally from the
  filler (planned Marv/Perf/Great/Good/Miss counts); `State::Idle`. Idempotent.

### 4.6 `mods/multiplayer_bot/skill.rs` (pure)

(Revised 2026-09-14 after the first cabinet playtest — the zero-mean Gaussian of the approved
design is replaced by a **lean + two-regime jitter** model; targets and the reason below.)

```rust
pub struct Params { lean_l1_ms, lean_knee_ms, lean_knee_level, lean_l10_ms, tight_l1_ms,
                    tight_l10_ms, drift_ratio, loose_l1_ms, loose_l10_ms, pocket_l1,
                    pocket_exp, p_miss_l1, p_miss_exp, form_sd, late_bias_sd, sign_stickiness }
pub const DEFAULT: Params;                        // the shipped anchors
pub struct Curve { lean_ms, tight_ms, drift_ms, loose_ms, p_tight, p_miss, form_sd,
                   late_bias_sd, sign_stickiness }
pub fn curve(level: u8) -> Curve;                 // = curve_from(&DEFAULT, level); level clamped 1..=10
pub fn curve_from(p: &Params, level: u8) -> Curve;// the simulator's what-ifs go through the SAME fn
pub struct Rng(u64);                              // xorshift64*; seeded per play
pub struct Form { p_late, sign: ±1, drift, factor }   // per-song bias + current side, drift, form
impl Form { pub fn new(rng: &mut Rng, c: &Curve) -> Self; }
pub enum Plan { Hit { d_ms: i32 }, Miss }
pub fn decide(rng: &mut Rng, form: &mut Form, c: &Curve) -> Plan;
pub fn grade_for_offset(d_ms: i32) -> u8;         // 0..=3 by the game's GRADED windows, 5 beyond ±124 (never 4)
```

Per note: Miss with probability `p_miss · form.factor`; else `d = round(lean + jitter)` where
`lean = sign · (lean_ms + drift)` and the jitter is `N(0, tight_ms)` with probability
`p_tight` ("in the pocket") or `N(0, loose_ms · form.factor)` otherwise; `|d| > 124 ⇒ Miss`.
The **side** `sign` is a two-state Markov chain: each note keeps the previous side with
probability `sign_stickiness` (0.7 ⇒ runs of a few notes — rushing one phrase, dragging the
next) or redraws it from the song's `p_late` (drawn once per song as `clamp(0.5 +
late_bias_sd·z, 0.1, 0.9)`, so a typical song is 35–65 % late, an occasional one 20/80 — never
100/0). The **magnitude** drift is AR(1) (ρ 0.97/note, stationary sd `drift_ms`), independent
of the side, so a tight stretch stays tight across a side flip. `form.factor` is log-normal
(sd `form_sd`, median 1), drawn once per song, scaling BOTH the loose σ and `p_miss` (a bad
day is wilder and flubbier — it turns the NORMAL gauge's sharp miss-rate knee into a smooth
per-level fail ramp). The `Form` lives in the planner's `SongState`, so a song reset re-rolls it.
Grades depend on |d| only, so the side chain does not move the grade mix (host-tested: every
bucket within 1 % of a frozen-side run).

**Why a lean.** The S-Marvelous band `|d| ≤ 12` is 24 ms wide, the exclusive-Marvelous shell
`12 < |d| ≤ 17` only 10 ms; ANY zero-centred unimodal error distribution — however wide — puts
≥ 2.4× more Marvelous-tier hits in the S-Marv band than in the shell, so "Marvelous more common
than S-Marvelous" is unreachable by widening σ. Centring the tight core ON the shell (a
consistent early/late lean — the uncalibrated-dancer shape) is the only way; the lean is
largest at L1 (17 ms — a beginner's pocket hits spill into Perfect), 14.5 ms at the knee (L8)
and 11.7 ms at L10 where S-Marvelous takes over.

Curves (`DEFAULT`, `curve_from`): lean linear 17 → 14.5 over L1..8, then linear to 11.7 at L10;
tight σ geometric 3.5 → 2.5; drift sd = 0.6 · tight; loose σ geometric 60 → 12; pocket share
`1 − 0.65 · ((10−L)/9)` (35 % → 100 %); `p_miss = 0.015 · ((10−L)/9)^1.4`; `form_sd` 0.35;
`late_bias_sd` 0.15, `sign_stickiness` 0.7 (level-independent). Corpus SLOW share of the
stock FAST/SLOW counters: mean 50 % at every level with a per-song sd of 9 % (L1) → 31 % (L10 —
few counted judgements per song, so its split is coarse).

Corpus outcome (6,621 SINGLE charts × 3 seeds — the tuning targets set by the maintainer after
playtesting the 2026-09-13 constants, which gave L10 71 % MFC, L1 60 % fail, ≥ 50 % S-Marv from
L6 and EX% saturated 96.6/99.0/99.9 over L8–10):

| L | S-Marv % | Marv % | Perf % | Great % | Good % | Miss % | EX % | FC % | PFC % | MFC % | fail % |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | 15.3 | 18.3 | 28.3 | 24.1 | 7.6 | 6.4 | 62.4 | 3.5 | 0 | 0 | 11.6 (2 b … 20 C) |
| 4 | 20.7 | 31.8 | 30.4 | 14.0 | 1.8 | 1.3 | 78.6 | 16.6 | 0.1 | 0 | 0.1 |
| 7 | 26.0 | 46.3 | 23.2 | 4.0 | 0.1 | 0.4 | 89.7 | 44.4 | 2.8 | 0 | 0 |
| 8 | 27.9 | 50.6 | 19.3 | 2.0 | 0 | 0.2 | 92.4 | 61.0 | 10.4 | 0 | 0 |
| 9 | 42.2 | 48.1 | 9.1 | 0.6 | 0 | 0.1 | 96.7 | 80.8 | 34.4 | 0.3 | 0 |
| 10 | 60.9 | 36.8 | 2.3 | 0 | 0 | 0 | 99.2 | 98.0 | 98.0 | 9.8 (28 b … 1 E/C) | 0 |

— L10 ≈ 10 % MFC corpus-wide (MFC is `P(Marv)^notes`, so short Beginner charts carry most of
it: 28 % Beginner, 10 % Basic, 3 % Difficult, 1 % Expert/Challenge), L1 fails ≈ 12 %,
exclusive Marvelous > S-Marvelous through L9 (S-Marv ≈ ¼ of steps at L7, dominant at L10 only),
EX% climbing ≈ 4–5 points per level with no plateau, fail ramp 12 / 4 / 1 / 0 % over L1–4.

- Game windows (inclusive, ms): Marvelous ±17, Perfect ±34, Great ±84, **Good ±124 — the
  outermost graded window. DDR World has no Boo**: the judge's table still carries a ±160
  row but its accept test is `grade < 4`, so a 125..160 ms event is matched, rejected
  (note unjudged, press kept) and Missed once `mc > note.mc + 160`. A planned |d| > 124 is
  therefore a Miss.

Seed: `splitmix64(qpc_now ^ (mcode << 32) ^ (difficulty << 8) ^ level)`.

### 4.7 `mods/multiplayer_bot/planner.rs` (pure)

The per-frame algorithm, expressed over plain data so it is host-testable:

```rust
pub struct NoteView { pub idx: usize, pub kind: i8 /* 0 tap/shock, 2 freeze tail */,
                      pub music_count: i32, pub beat_count: i32,
                      pub state: [i32; 8], pub length: [i32; 8], pub unjudged: bool }
pub struct BotPanelFlags { pub is_held: [u8; 8], pub was_just_pressed: [u8; 8], pub event_mc: [i32; 8] }
pub struct SongState { pub plans: Vec<Option<Plan>>, pub blocked_until: [i32; 8],
                       pub last_event: [i32; 8], pub cursor: usize, pub tally: [u32; 6] }
pub const LOOKAHEAD_MS: i32 = 8;          // the stock update's lookahead
pub const MAX_EARLY_MS: i32 = 200;        // walk window past the stock cutoff
pub const GOOD_WINDOW_MS: i32 = 124;      // outermost GRADED window — a floored E beyond it is a Miss
pub const MISS_WINDOW_MS: i32 = 160;      // the judge's Miss mark; a Miss blocks its panels until +161

pub fn plan_frame(notes: &[NoteView], st: &mut SongState, rng: &mut Rng, c: &Curve,
                  mc: i32, cur_beat: i32, out: &mut BotPanelFlags);
```

`plan_frame`:

1. Zero `out`.
2. For `i` from `st.cursor`: stop when `mc < note.music_count − LOOKAHEAD_MS − MAX_EARLY_MS`;
   advance `cursor` past leading judged notes.
3. **Freeze body** (identical to the game's `update`): if `cur_beat < beat_count +
   max(length[..])`, set `was_just_pressed[p] = 1` for every panel with `state[p] >= 2`.
4. For an **unjudged** note:
   - **Shock** (all four states of a pad == 1): `was_just_pressed[p] = (state[p] != 1)` —
     identical to the game (the bot steps on nothing on that pad).
   - Otherwise decide once (`plans[idx]` from `decide`), then:
     - `Miss` → press nothing; for each arrow panel `blocked_until[p] = music_count +
       MISS_WINDOW_MS + 1`.
     - `Hit{d}` → `E = music_count + d`; `E = max(E, blocked_until[p], last_event[p] + 1)` over
       the note's arrow panels (the judge attributes a press to the earliest unjudged note on
       that panel and has no window test at match time, so per-panel events must be
       monotonic IN NOTE ORDER and a decided Miss must not be "rescued" by the next note's
       press); if `E > music_count + GOOD_WINDOW_MS` treat as `Miss` (never gradable). The
       decision is resolved ONCE, the first frame the note enters the walk, and is stable.
       If `mc >= E − LOOKAHEAD_MS` and no earlier note owns one of its panels this frame
       (one live event per panel per frame — a blocked note still RESERVES its panels): for
       each arrow panel `is_held[p] = 1; was_just_pressed[p] = 1; event_mc[p] = E`.
     - Kind-2 freeze tails are never decided or pressed (only their body hold applies).
5. Tally: the first frame a note becomes judged, `tally[grade]++` (from
   `grade_for_offset`; Miss → 5).

Properties the tests pin: per-panel events strictly increase; a Miss blocks the following
note on that panel until `mc + 161`; jumps share one `E`; freeze bodies hold after the head;
shocks never press their pad; the walk never touches notes beyond the window; determinism
for a fixed seed.

### 4.8 `mods/multiplayer_bot/filler.rs`

The engine-facing adapter registered as the `BotFillFn`:

- `start_song(side, level, seed)` / `reset(side)` / `tally(side)`.
- `fill(side, actor, mc, out)`: reads `cur_beat = *(actor+0x168)`, the result range
  `(actor+0xB0, actor+0xB8)` (via `types::game_note::for_each_result`, which already
  null-guards note pointers) into a reusable `Vec<NoteView>` (capacity retained; entries
  built from `GameNote` fields and the result's `+0x08` timestamp / `+0x0C` grade), then
  `planner::plan_frame`. State per side lives in a `Mutex<SongState>` taken with `try_lock`
  (uncontended: everything runs on the game thread); on contention or poison the frame
  yields empty flags (one WARN, rate-limited).
- Guards: `memory::is_readable` on the actor fields once per song (first frame), result
  count sanity (`≤ 8192`), and `catch_unwind` around the body (the dispatcher already wraps
  callbacks; this keeps the filler independently safe if reused).

### 4.9 `mods/multiplayer_bot/extra_stage_guard.rs`

One `GenericDetour<unsafe extern "C" fn(i32)>` on the game's extra-stage grant function
(signature `extra_stage_grant`, prologue AOB — see Appendix A.4), installed once at
`enable` via `hooks::install_enabled`. Callback:

```rust
unsafe extern "C" fn grant_hook(arg: i32) {
    let Some(hook) = (&*addr_of!(GRANT_HOOK)).as_ref() else { return; };
    match impersonation::active_bot_side() {
        Some(bot) if guard_enabled() => {
            let pw = stage_records::player_work(bot);
            // clear the bot's entered byte around the original, restore after
            with_entered_cleared(pw, || hook.call(arg));
        }
        _ => hook.call(arg),
    }
}
```
If the AOB is missing: no detour, one WARN at `init` ("extra-stage grant will consider the
bot"). Disable = passthrough flag.

### 4.10 Signatures (`core/signatures.rs`)

| Name | Kind | Purpose |
|---|---|---|
| `extra_stage_grant` | AOB, function prologue (Appendix A.4); unique on 20250805 / 20260224 / 20260825 (20260721 to be attested by the offline sweep) | detour target for R10 |

No other new signature. The impersonation uses `stage_records` (GameWork / PlayerWork /
records / stage counter), `player_option_offset`, and `scene_manager`; the controller uses
`auto_foot_panel_vtable` (RTTI), `auto_foot_panel_update`, `judge_notes` and
`judge_hook::foot_panel_offset()` — all existing.

### 4.11 Option textures and menu placement

- `scripts/option_strings.py`: `LABELS["bot_opponent"]` = {en "BOT OPPONENT (1P ONLY)", ja, ko},
  `LABELS["bot_opponent_level"]` = {en "BOT LEVEL", ja, ko}; optional `PREVIEWS` entries
  (bool off/on split panel; level single panel). Regenerate with
  `python3 scripts/gen_option_labels.py`.
- `mod-config.json` `option_menu_settings`: insert `bot_opponent`, `bot_opponent_level`
  after `autoplay` (both menus).

## 5. Data Models

### 5.1 Game-side (read/written by the mod)

| Object | Offset | Type | Use |
|---|---|---|---|
| `GameWork` (`**game_work_global`) | `+0x0` | i32 | versus word: 0 solo/doubles, 1 versus — **written** 1 at flip, restored |
| | `+0x4` | i32 | style (0 single / 1 double) — gate |
| | `+0x8` | i32 | primary side — read only |
| | `+0xC` | i32 | stage counter — via `stage_records::stage_counter()` |
| | `+0x70` | u64 | course pointer — gate |
| | `+0xD0` | i32 | event mode — gate |
| `PlayerWork[side]` (`*table[side]`) | `+0x4` | u8 | entered — **written** 1 at flip, restored |
| | `+0xC..+0x14` | char[8+1] | name — **written** `BOT LV<n>`, restored |
| | `+0x50/+0x54/+0x5C` | i32 | style / committed mcode / selected difficulty — **mirrored** from the human |
| | `+player_option_offset()` | `ddr::player::Option` | `+0x08..=0x6C` **copied** from the human; `+0x18` gauge **forced 0** |
| `record[stage]` (`stage_record(side, stage)`) | `+0x00/+0x04/+0x08` | i32 | mcode / difficulty / style — `+0x04/+0x08` **mirrored** |
| `GamePlayActor` | `+0x84` | i32 | side |
| | `+0xB0/+0xB8` | ptr | results vector begin/end (0x40 stride) |
| | `+0x168` | i32 | current beat position (freeze-hold input) |
| | `+0x270/+0x278` | ptr | `IFootPanel*` slot (`judge_hook::foot_panel_offset()`) |
| Result entry | `+0x00` | ptr | `GameNote*` |
| | `+0x08` | i32 | judge timestamp (<0 unjudged) |
| | `+0x0C` | u32 | grade (0xFF unjudged) |
| `GameNote` | `+0x04/+0x08` | i32 | beat_count / music_count |
| | `+0x1C[8]/+0x3C[8]` | i32 | state (1 TRG, 4 REP, ≥2 freeze body) / length |

### 5.2 DLL-owned

```rust
#[repr(C)]
pub struct BotFootPanel {
    vtable: *const *const u8,        // +0x00  cloned AutoFootPanel vtable (slots 5/6 ours)
    is_held: [u8; 8],                // +0x08  read by stock slot 3
    was_just_pressed: [u8; 8],       // +0x10  read by stock slot 2
    event_mc: [i32; 8],              // +0x18  read by OUR slot 5, zeroed by OUR slot 6
    _reserve: [u8; 0x58 - 0x38],     // total 0x58 — matches the largest stock object
}
```
One `BotFootPanel` per side plus one stock-shaped `AutoFootPanel` object (0x58 bytes, stock
vtable) for the `Perfect` controller — all `VirtualAlloc`'d once at service init (RWX
pages; the vtable clone lives in the same allocation region, COL at `[-1]`).

Mod state: `OPTION_ON: [AtomicBool; 2]`, `LEVEL: [AtomicI32; 2]`, impersonation
`Mutex<State>`, filler `[Mutex<SongState>; 2]`, `CURRENT_MC: [AtomicI32; 2]`.

### 5.3 Persistence

| Key | Where | Semantics |
|---|---|---|
| `mod_bot_opponent` (0/1), `mod_bot_opponent_level` (1..10) | network save/load (`PersistMode::Full`) + `custom_options.p1/p2` JSON cache | per-player preference; backend needs a migration (`opt_mod_bot_opponent`, `opt_mod_bot_opponent_level`) — until then the JSON cache carries it |

No config section. No writes to `mod-config.json` by the mod.

## 6. Error Handling

| Situation | Behaviour |
|---|---|
| `foot_panel_swap::init` fails (missing `judge_notes` / vtable / update / fp offset) | Service unavailable; autoplay AND multiplayer-bot both refuse `init` (both show OFF). |
| `stage_records` unavailable or `player_option_offset()` None | Mod `init` returns false — never registers rows. |
| Eligibility refused (2P, doubles, course, event, versus) with the option ON | No flip; one INFO per song naming the reason. |
| A probed pointer is unreadable during `apply` | Undo writes made so far, WARN, remain `Idle` (stock 1P song). |
| `rec_b+0x00 != rec_h+0x00` at apply (commit did not prepare the bot record) | Refuse the flip, WARN. |
| Bot panel filler contention / poison | Empty flags for that frame (the bot misses) + rate-limited WARN. |
| `extra_stage_grant` AOB missing | No guard; WARN at init; stock rule (bot participates) applies. |
| Mod disabled mid-window | `restore()` immediately (bot actor keeps existing for the rest of the song with no controller → it will miss everything; results still render; the next song is 1P). |
| Scene limbo / crash paths | Restore triggers on any scene ∉ window; a 20 s watchdog after entering the window WARNs if no scene change arrives (diagnostic only). |
| Option row registration `Duplicate` (re-enable) | Treated as success; atomics reseeded from `get_value`. |
| `judge_hook` callback panics | Dispatcher `catch_unwind` (existing) + the filler's own guard; a panic permanently disables that frame's fill only. |

All hook callbacks are free of `unwrap`/`expect`/indexing; panel indices are masked to 0..7;
vector reads are bounds-checked against the result range; logs are one-shot or rate-limited.

## 7. Testing Strategy

### 7.1 Host tests and the offline simulator (`tools/bot_sim/`)

A second native crate (like `updater/`, zero dependencies, not a workspace member) mounts
the DLL's REAL pure files via `#[path]` — `services/foot_panel_swap/layout.rs`,
`mods/multiplayer_bot/{eligibility,skill,planner}.rs`, `core/ssq/{ssq_chunk,timing}.rs` —
so `cargo test` there runs their `#[cfg(test)]` suites (this is what
`scripts/validate_multiplayer_bot.sh` does) and the binary simulates the bot over a real
chart corpus:

```
scripts/bot_sim.sh <ssq-dir> [--out report.html] [--json out.json] [--levels 1-10]
                   [--diffs b,B,D,E,C] [--seeds 3] [--filter substr] [--fps 60] [--smarv-ms 12]
                   [--set <anchor>=<value> …]                           # skill::Params what-ifs
```

- `chart.rs` — SSQ → notes per SINGLE difficulty (taps/jumps, REP freeze heads with tick
  lengths, kind-2 tails, shocks; ticks → ms with the game's ×1000/TPS normalisation).
- `judge_model.rs` — the judge as decompiled (A.5): per-frame walk, earliest-unjudged
  attribution, grades 0..=3 only, rejected match keeps the press, one accepted note per
  frame, Miss at +160, shock window; freeze tail O.K. iff the head was hit; a permanent
  **planner-vs-judge self-check** (`mismatches`, expected 0 — the same check the DLL runs on
  cabinet).
- `gauge.rs` — the NORMAL gauge, exact integers, anchored by hand-computed cases.
- `scoring.rs` — `judge_submit` counters/combo/FC/EX/money score, S-Marv column, rank table.
- `report.rs` — ONE self-contained HTML (inline CSS/JS/JSON, offline): level overview,
  level × difficulty heatmaps, filterable DDR-styled scorecards (S-MARV·MARV·PERF·GREAT·
  GOOD·MISS·O.K.·N.G., score/EX/rank, FC badges, gauge bar + death time, timing histogram).

Unit tests pinned by the modules: eligibility table (either side is the human; every
refusal and every `Unavailable` input; level clamp), skill (monotone curves, 1e6-sample
endpoints at L10 (P(Marv) ≥ 0.997) / L1 (P(Marv) 0.17..0.22, P(Miss) 0.14..0.19), window boundaries ±17/±34/±84/±124 and Miss beyond, determinism),
planner (early/late/miss single note, Miss blocks the next same-panel note until +161,
floored event beyond ±124 ⇒ Miss, note-order monotonicity, one event per panel per frame,
jumps share E, freeze body, shocks, tails never pressed, cursor/lookahead, tally), chart
parser (tempo map + TPS normalisation, taps/jumps, freezes, shocks, DOUBLE skipped),
judge model (perfect bot MFCs, jumps, freeze O.K./N.G., shocks O.K., all-miss bot fails,
planned == judged across levels), gauge anchors, scoring formulas.

### 7.2 Signature sweep

`./scripts/validate_signatures.sh <dir-of-gamemdx-builds>` must stay green with the new
`extra_stage_grant` AOB on all four builds; `shape_diff.py` is not needed (the detour reads
nothing at `match+N`).

### 7.3 Cabinet checklist (the engine-facing validation)

1. **Autoplay regression**: human autoplay ON, no bot — identical behaviour to today
   (Marvelous every note, watermark shows).
2. **Bot L10, 1P on P1**: two READY panels; P2 lane scrolls at P1's speed/skin; name plate
   `BOT LV10`; bot judgements mostly Marvelous with occasional Perfects; two results panes;
   TOTAL RESULTS shows P1 only; no `save_sender` invocation for side 1 in the log (or a
   suppressed one); P1's per-stage save proceeds.
3. **Bot L1**: visible Greats/Goods/Misses; bot gauge drains; bot death handled by the
   game (FAILED on its side, song continues).
4. **Human on P2 pad**: bot takes P1.
5. **Quick restart / quick fail** mid-song and in the READY window: restart respawns the bot
   with a different pattern; fail returns to a 1P song select; toggling the option OFF makes
   the next song 1P.
6. **Extra stage**: on a 3-stage setting, a bot that does not AAA does not block the human's
   extra stage (guard log line present).
7. **2P real session**: option ON on both sides — no bot, no flip, one INFO.
8. **Doubles / course**: no flip.
9. **BPL Mode mod ON**: the battle frame appears against the bot with the bot's name.
10. **Old build (20250805 or 20260224)**: steps 2–3 repeat (controller build-independence).

## Appendix A — Reverse-engineering findings (inlined)

Addresses are file-relative to the game module base `0x180000000`; build 20260825 unless
stated. Globals (`game_work_global`, `player_work_table`) are derived at runtime and never
hardcoded.

### A.1 Actor count and the entered byte

`createNextSequence` case `0x1d` (0-idx scene 28) builds, per side, `{entered =
*(PlayerWork+0x4), is_main = (GameWork+0x8 == side), is_double = (GameWork+0x4 == 1),
difficulty = clamp(PlayerWork+0x5C), 0}` and passes both to the `DancePlaySequence`
constructor (`0x1800570a0`), which heap-copies them to `DPS+0xF0/+0xF8`.
`DancePlaySequence::onUpdate` (`0x180057e10`) case 1 creates a `GamePlayActor` for each side
with `entered != 0`, storing them at `DPS+0x100/+0x108`. Byte-identical shape on 20250805
(`0x18002fb03`).

### A.2 Versus word

`SelectStyleSequence::onUpdate` (`0x1800b0bc0`) writes `GameWork+0x0 = (two sides confirmed)`,
forcing `GameWork+0x4 = 0` when versus. Readers during the play window are display selectors:
`ResultSequence` build (`0x1800b9030`) shows side *i*'s info pane, profile and tab iff
`GameWork+0x0 == 1 || i == primary`; `createNextSequence` cases `0x16/0x24/0x2e/0x3b` copy
it to the sound manager's pan-by-side byte (`+0x20C4`); `ReflectSavePlayerData`
(`0x180018ee0`) copies it to staging `+0x44`/`+0x12C` (wire `/data/mode`, `/data/battle_mode`).

### A.3 Song-select commit

`0x1800fdc90` prepares BOTH sides' `record[stage]` with `(mcode, difficulty_from_that_side's_cursor,
GameWork+0x4)` when the mcode changed, then writes `PlayerWork+0x5C` for that side. The
non-entered side's cursor is unset in 1P — hence the mirroring in §4.5 step 3.

### A.4 Extra-stage grant

`0x1801ddcd0(int arg)`: gated on `GameWork+0x59 == 0`, `arg == 0`, `GameWork+0x70 == 0`,
`GameWork+0x4 != 1`, `max_stage + 1 == 3`; then for every side with `PlayerWork+0x4 != 0`
requires `record[0]+0x50 >= 0xF`, `PlayerWork+0x1710 == 0`, gauge option ∈ {0, 0xC},
`record[0]+0x270 != 7`, else returns; on success `GameWork+0x59 = 1`. Called from
`ResultSequence::onUpdate` case `0x16` when the stage counter is 0. Prologue AOB (unique on
20250805 @ `0x1801c6970`, 20260224 @ `0x1801ca7e0`, 20260825 @ `0x1801ddcd0`):

```
48 83 EC 38 48 8B 05 ?? ?? ?? ?? 48 8B 10 80 7A 59 00 0F 85 ?? ?? ?? ?? 85 C9 0F 85 ?? ?? ?? ??
48 83 7A 70 00 0F 85 ?? ?? ?? ?? 83 7A 04 01 0F 84
```

### A.5 `AutoFootPanel` and the judge

7-slot vtable on every build: `[1] update`, `[2] wasJustPressed → *(this+0x10+panel)`,
`[3] isHeld → *(this+0x08+panel)`, `[4] thunk → slot 5`, `[5] getPressAge`, `[6]
consumePress`. Slot 5/6 differ by build: 20260721+ use libavs ordinal 45 with a `qword[8]`
at `+0x18` back-dated so the event lands on `note.music_count`; earlier builds use
`timeGetSystemTime().ms` with a `dword[8]`. The stock `update` (`0x180022D00`) presses
panels whose state is 1 (TRG) or 4 (REP — freeze heads), holds `state ≥ 2` panels while
`cur_beat < note.beat + max(length)`, and avoids shocks by pressing the OTHER panels.

The judge (`judgeNotes`, `0x18005EC00`) computes `event = music_count − getPressAge(panel)`,
matches a held panel to the earliest unjudged kind-0 note carrying that arrow **without a
window test**, grades by the first window containing `event − note.music_count` from the
table at `0x18035B9E0` (Marvelous ±17, Perfect ±34, Great ±84, Good ±124, [±160]) — but
accepts only `grade < min(best_this_frame, 4)`: **the ±160 "Boo" row can never win (World
has no Boo)**, a rejected match leaves the note unjudged and the press unconsumed (so it may
match the NEXT note on that panel), and **exactly one note is accepted per actor per frame**
(the best grade, strict `<`). Miss when `music_count > note.music_count + 160`; walk cutoff
`music_count < note.music_count − 260`; jump panels must agree within 66 ms; shocks are
N.G. on a `wasJustPressed` inside `[−34, +84]`, else O.K. Nothing checks `event <=
music_count`. These facts make the cloned-vtable controller exact and build-independent,
and motivate the planner's per-panel monotonic-event, `blocked_until` and one-event-per-
panel-per-frame rules. Freeze judge, `judge_submit` bookkeeping (order: FC → combo → grade
code), EX / money score, and the NORMAL gauge's exact integer formula are transcribed in
`docs/gauge_and_judge_scoring_research.md` and reproduced by `tools/bot_sim`.

### A.6 `ddr::player::Option` (from the setters' debug strings)

`+0x08` SpeedType, `+0x0C` Hispeed, `+0x10` derived multiplier, `+0x14` ScrollSpeed,
**`+0x18` Gauge (0 = NORMAL)**, `+0x1C` ScrollDirection, `+0x20` TimingDisp, `+0x24`
TimingMusic, `+0x28` Visibility, `+0x2C` ConstantValue, `+0x30` LaneTransparency, `+0x34`
LaneCover, `+0x38` FastSlow, `+0x3C` Guideline, `+0x40` Stepzone, `+0x44..+0x54` draw
order/layout/notice/scroll-moving, `+0x58` ArrowPlacement, `+0x5C` ArrowColor, `+0x60`
ArrowDesign, `+0x64/+0x68/+0x6C` CutTiming/CutFreeze/CutJump, `+0x90` f64 BPM.

### A.7 Cardless entered side is a native shape

The BPL guest-join helper (`0x1800b2d10`) produces `PlayerWork+0x4 = 1` on a
`PlayerWork::reset` object (`+0x5 = 0, +0x8 = −1, +0x18 = 0, +0x1C = 0`). The per-stage
`SavePlayerDataActor` waits on `PlayerWork+0x1C` and the ark side-state check before sending;
the EAM-exit settle-up requires `PlayerWork+0x8 >= 0`. Neither fires for such a side.

## Appendix B — Alternatives considered

| Alternative | Why rejected |
|---|---|
| **Fake entry at song select** (set `PlayerWork+0x4` before scene 25) | Versus song select needs two cursors and two confirms; `versus_mirror` would copy the human's options into the bot; entry/exit flows iterate entered sides. |
| **Loader-struct injection** (detour the DPS ctor call, never touch `+0x4`) | Every other `+0x4` reader in the window (READY panel, name, extra stage) would treat the bot as absent → half-drawn UI; more detours for less coverage. Kept as a fallback if a `+0x4` reader with unwanted side effects ever surfaces. |
| **Edit `pressTime` after the game's `update`** | Depends on the per-build press-time clock and stride (dword vs qword) and on the exact stamp semantics; needs a derived stride value; the getter-cloning design removes the dependency entirely. |
| **Wrap only `getPressAge` (slot 5) on the stock object** | Can shift timing but cannot produce a Miss (the game's `update` always presses). |
| **Rewrite the `judge_submit` payload** | Fakes a grade after the judge decided; leaves the judge's internal state (consumed presses, combo) inconsistent; another mod owns that detour. |
| **Drive the bot through the ark IO panel-injection seam** (SMX path) | Frame-resolution timing unless the detour overwrites the press timestamp; single-slot provider owned by SMX; a real-input path for a side that has a GamePlayActor is strictly more work than the judge-side object. |
| **Independent bot chart difficulty** | Breaks the versus comparison and reintroduces the two-cursor problem. |
| **Keep the bot through TOTAL RESULTS** | Per-session aggregation of a side that existed for some songs only; `TotalResultSequence` indexes `GameWork+0x8` unchecked; the restore must also precede the logout flow. |
