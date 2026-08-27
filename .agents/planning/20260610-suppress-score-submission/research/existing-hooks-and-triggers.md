# Research: Existing Hooks & Trigger State

Audit of what we already hook on the save path, and how the two trigger mods
(`autoplay`, `quick_restart_or_fail`) currently expose their state. This is the
local-code half of the research; the binary RE half lives in
[`score-submission-re.md`](./score-submission-re.md).

## Version context (IMPORTANT)

- Ghidra has **`gamemdx_20260526.dll`** (current build, the score-commit RE target)
  and **`ess.dll` (20260324)** loaded.
- **ess.dll has NOT been updated client-side since 20260324** — so the existing
  `custom_player_options_research.md` ess.dll findings (save_sender/load_receiver
  addresses, savedata layout, `<option>`/`<result>` block offsets) **remain valid
  for the current build.** Only the gamemdx-side score-commit path needs fresh RE
  against 20260526.

## What we hook today (custom_options_persistence.rs)

Two `retour::GenericDetour`s on **ess.dll**, resolved by their unique log strings
(`resolve_by_log_string` → LEA-xref → walk-back-to-prolog):

| Hooked fn | ess.dll addr (20260324) | Signature | What our trampoline does |
|---|---|---|---|
| `sys_playerdata_save_sender` | `+0x29E70` | `u64 fn(job, kbin_ctx)` | calls original, then appends `<mod_{id}>` s32 children to `/data/option`; also writes offline JSON cache |
| `sys_playerdata_load_receiver` | `+0x25D70` | `u64 fn(job, kbin_ctx)` | calls original, then reads `<mod_{id}>` children back |

**Per-side derivation already implemented (save_sender trampoline):**
```
savedata = *(job + 0x10)
playside = *(savedata + 0x90)   // 0 = P1, 1 = P2
```
Comment cites "confirmed via Ghidra 20260324". Side is also derivable on the load
path from `*(job)` (playerdata_no ≤ 1). **This is exactly the per-player handle
Q2 needs** — IF the score upload rides this same save_sender.

## The load-bearing question

`save_sender` is the **playerdata profile save** encoder. From
`custom_player_options_research.md`, the save-side savedata struct contains, in ONE
buffer:
- `/common` block (`+0xF0..`)
- `<option>` block (`+0x118..+0x188`)
- `/lastplay`, `/filtersort`, `/checkguide`
- `/event` array (≤64)
- **`/result` array (≤5 stages, each 0x22B8 bytes** — `+0xA70..+0xB807`) — **this is
  where per-stage SCORE data almost certainly lives** (each stage has its own
  embedded `/option` sub-block at stage `+0x2AFC..+0x2B6C`).
- `/league`, `/brave`, `/grade`.

So the **score is plausibly a sub-block of the same `playerdata_save` upload** we
already hook. If confirmed, the entire per-play score travels through `save_sender`,
and Q1's "suppress the entire end-of-song save" == "make save_sender a no-op (return
without calling original) for a tainted side." This is the single cleanest
chokepoint and we ALREADY own the detour.

**BUT** — open sub-questions the RE must answer (see score-submission-re.md):
1. Is the per-play score actually in the `playerdata_save` `/result` block, or does
   the game send a **separate** score/ranking request (e.g. a distinct xrpc method
   like `score_*`, `usergamedata_*`, a `playerdata_save` variant) that does NOT go
   through this same `save_sender`?
2. World uses **multiple save triggers** (from the research doc's symbol dump):
   `PlayerDataSaveFirstRequest`, `PlayerDataSaveStageRequest` (per-song!),
   `PlayerDataSaveLogoutRequest`. **`PlayerDataSaveStageRequest` fires after each
   song** — is THAT the score upload, and does it route through the same
   `sys_playerdata_save_sender`, or a different sender? If all variants share the
   one `save_sender`, suppressing there kills all of them for the tainted side.
3. Does suppressing `save_sender` for one side cleanly skip just that side, given
   the game calls it once per carded-in player? (Per-side return.)

## Trigger-state plumbing (how each mod exposes "tainted")

### Autoplay (`src/mods/autoplay.rs`)
- Per-player enable flags already exist as a module static:
  ```rust
  static AUTOPLAY_ENABLED: [AtomicBool; 2] = [false, false];
  ```
  Written by `autoplay_on_change(side, val)` (custom-option change callback), read
  by the judge pre/post callbacks. **This is the per-side autoplay signal the guard
  needs at save time** — expose via a small `pub fn is_autoplay_enabled(side) ->
  bool` accessor (read at save time per Q4).
- Autoplay is a **custom option** (bool toggle on Mods/Assist tab), default off,
  per-player isolated. Its `required_signatures()` are `judge_notes`,
  `auto_foot_panel_vtable`, `auto_foot_panel_update`.
- **Q6 fail-closed wiring:** add the score-submission signature to autoplay's
  `required_signatures()` (or gate enable() on `guard::is_available()`) so autoplay
  refuses to enable if the save hook can't be installed.

### Quick Restart / Fail (`src/mods/quick_restart_or_fail.rs`)
- Triple-`3` → `trigger_fail()` → `fail_song(None, …)` → walks active
  GamePlayActors and `force_game_over()` on **all** of them. **Confirms Q2's
  assumption: quick-fail ends the song for every active GamePlayActor (both
  sides).** So the quick-fail taint is naturally session-wide.
- Triple-`1` → `trigger_restart()` → `fail_song(Some(GAMEPLAY), …)` with a one-shot
  STAGE_RESULT→GAMEPLAY redirect. Restart must **reset** taint (Q4) — the mod
  already clears gesture buffers on leaving gameplay; the guard taint should reset
  on gameplay (re)entry.
- **⚠️ Claim to VERIFY, not trust:** `force_game_over` sets `m_isDead` (`+0x1E8`)
  and the in-code comment asserts *"DPS's STEP_FINISH reads this to pick the FAILED
  shutter kind **and suppress score submission**."* If a natural DDR fail already
  suppressed the upload, quick-fail would need no new work — but a normal fail
  almost certainly still uploads a FAILED play record (clear lamp = failed, score
  as-played). The learnings doc ("Re-verify every load-bearing claim in an RE
  handoff") says to re-prove this against disassembly. The RE must check what
  STEP_FINISH actually does with `m_isDead` w.r.t. the network save. If "suppress"
  only means "don't write a CLEARED lamp", we still must suppress the upload
  ourselves.

## Where the guard logic should live (per Q5: hard-baked, shared service)

One detour owner on the save path (one-detour-per-target rule). Today
`custom_options_persistence` owns the `save_sender` detour. Options:
- **(Pref) Extend the existing `save_sender` trampoline** to consult a taint check
  before calling the original, returning early (suppress) for a tainted side. The
  guard's taint state lives in a small new module (e.g. `services::score_guard` or a
  pair of accessors), set by autoplay + quick_fail, read by the trampoline. This
  respects one-detour-per-target and reuses the per-side derivation already in the
  trampoline.
- This is contingent on RE confirming the score rides `save_sender`. If the score
  uses a *separate* sender/method, we add a new detour on THAT function (new
  signature in `signatures.rs`), and the guard owns it.

## Open items carried into score-submission-re.md

1. Confirm score lives in `playerdata_save` `/result` block vs. separate request.
2. Identify the gamemdx-side score-commit/marshal call(s) and the save-trigger
   variants (First/Stage/Logout) and whether they share one ess.dll sender.
3. Verify the `m_isDead`/STEP_FINISH "suppresses score submission" claim.
4. Confirm per-side suppression semantics at the chosen chokepoint (skip one side
   cleanly).
