# Idea Honing — Quick Logout

Decision register. `Status` ∈ `Proposed` | `Accepted` | `Overridden` | `Assumed` | `Open`.
Ordered by blast radius: user-visible behaviour and gating first, mechanics last.

Feature: trigger the end-of-credit / e-amusement logout sequence on demand from
music selection, **and** make the resulting logout save carry the player's profile
settings even when the session used Premium Free / Autoplay. Mechanism source:
`docs/quick_logout_research.md`.

**Round 1 outcome (2026-07-27):** the trigger was simplified to a bare gesture
(no confirmation, no prompt, no config, no mode gates) and the summary path is
now the only path. The `score_guard` interaction was inverted: instead of
accepting that a tainted session skips the logout save, the feature now
**neutralises the score content of the logout packet and lets the save through**.
That opens D21–D25 and three new research items.

## Register

| ID | Decision | Resolution | Status |
|---|---|---|---|
| D1 | Does the forced logout show TOTAL RESULTS? | **Always.** Mechanism A only — `finish(child, 30₁ᵢₙdₑₓ)` (0-idx 29 loader) + one-shot redirect `30 → 32`. No no-summary variant, no config switch, no automatic fallback | Overridden |
| D2 | Trigger gesture | **Triple-9** on either pinpad (3 presses inside the existing 1.5 s `GESTURE_WINDOW`) | Accepted |
| D3 | Confirmation model | **None.** Triple-9 during music selection fires immediately. No arm/confirm state machine, no cancel key | Overridden |
| D4 | Who may trigger in a 2P session | **Any single side** arms and commits; it ends the session for both. Documented, not gated | Accepted |
| D5 | Scene gate | **Music selection only** — 0-idx 25. Matching/battle song-select variants (47/49) simply never match the gate | Accepted |
| D6 | Course / event-mode gating | **No mode gates.** Course selection happens *inside* scene 25 (a different view of the same scene) and commits straight into gameplay, so the scene gate already covers it. Verify (R6/R7) that forcing 0-idx 32 cannot crash in either mode; re-raise if it can | Overridden |
| D7 | Session-active gate | Require **at least one side entered** (`PlayerWork+0x4 != 0`). Redundant with the scene gate in practice (scene 25 is unreachable without a paid-in side) but cheap, fails closed, and identifies which sides to log | Accepted |
| D8 | Behaviour on a `score_guard`-tainted session | **Superseded by D21–D25**: don't suppress the logout save — strip the score content from it and let the profile/customize write-back through | Overridden |
| D9 | Config surface | **None.** No `quick_logout` block. On/off is `mods["quick-logout"]` only | Overridden |
| D10 | Per-player option row on the MODS tab | **No row.** It is an action, not a setting | Accepted |
| D11 | Confirm prompt UI | **No UI.** Nothing is drawn; the scene transition is its own feedback | Overridden |
| D12 | Mechanism C ("make this my last stage") in scope? | **Out of scope**, recorded as future work | Accepted |
| D13 | Milestone ordering | **Straight to the summary path.** The no-summary de-risking milestone is dropped — Mechanism A traverses the same logout chain, so it tests the save anyway | Overridden |
| D14 | Diagnostics for the silent-no-op failure mode | Log trigger context, then log the observed tail scene chain with timestamps; WARN if 0-idx 34 is skipped or exits in < 500 ms. **More important now** that no de-risking milestone precedes it | Accepted |
| D15 | Signature set | `sequence_finish` (new, **required**) + the already-resolved `stage_record_accessor` and `player_work_table`. The `game_work_field_probe` from round 1 is **dropped** — its only consumer was the event-mode gate (D6) | Overridden |
| D16 | Fail-closed policy on decoded constants | Range-check every decoded offset and cross-check against `premium_free_stage_inc` / `stage_record_accessor`; a mismatch disables the dependent behaviour, never mis-writes | Assumed |
| D17 | Double-fire latch | `current_scene() != SONG_SELECT` (free — `advanceToScene` writes `TS+0x68` synchronously), active-child tree flags `& 0x24`, plus an internal fired latch cleared on the next song-select entry | Assumed |
| D18 | Thread / call site | Call `finish` **synchronously from the input callback** — `input_manager::poll` already runs on the frame thread inside the render hook, and `finish` frees nothing | Assumed |
| D19 | Side effects on other mod state (trigger side) | The trigger writes **no** game state: no taint, no Premium Free unfreeze, no stage-counter writes | Assumed |
| D20 | Deliverables | `src/mods/quick_logout.rs`; one signature def; `types::scenes` constants for 32/33/34/35 + a comment on 29/30's misleading names (**no rename**); AGENTS.md entry-point row; README operator section; cabinet findings folded back into `docs/quick_logout_research.md`; `progress.md` maintained | Assumed |
| **D21** | Does the sanitize-instead-of-suppress policy apply to *every* logout save, or only quick-logout-triggered ones? | **Every** `savekind == 3` save. The problem ("I used Autoplay early in the session, now my profile edits can never be saved") exists identically at a natural session end | Accepted |
| **D22** | Sanitize mechanism | **Virginise the tainted side's per-stage play records (`mcode = -1`) on entry to 0-idx 34, before `SavePlayerDataActor` marshals them.** R5 confirmed the marshal gates every stage block on `mcode != -1 && end_time != 0`, and that all score content is record-sourced — no packet surgery needed. One accepted residual: a dan/grade block that can only fire after a class-9 extra stage (unreachable from a song-select logout; see research) | Accepted |
| **D23** | Granularity | **All of a tainted side's stage records** — the 5 array slots **and the course record at the accessor's course offset (`+0x2D8`)**, which R7 showed the marshal uses instead of the array when `PlayerWork+0x4C == 10`. No per-stage taint tracking | Accepted |
| **D24** | Code placement | Hoist the play-record layout decode (GameWork ptr-global, course field, stage counter, record base/stride, **+ the course-record imm32 at accessor+36**) out of `premium_free` into one shared helper used by both `premium_free` and the sanitiser. The sanitiser lives with the save policy (`custom_options_persistence`); its taint source stays `score_guard`, with `is_logout_suppressed` renamed to reflect the new semantics | Accepted |
| **D25** | What if the sanitiser cannot arm? | **Fail closed on score integrity**: fall back to today's behaviour — suppress the whole logout save. `save_sender` consults "was this side actually sanitised this session?", not just the taint flag | Accepted |
| **D26** | League score leak on a tainted logout | `<league><current>` is a client-authoritative accumulator, NOT record-sourced — record-virginising doesn't cover it. For a tainted side's `savekind == 3`, `save_sender` **removes the `<league>` node** before forwarding; the backend's own guard (absent node → no-op) preserves the pre-session league score. Mechanism: libavs **Ordinal 164 = `property_node_remove(node)`** (verified by decompile — logs `node_remove` on the `property` channel), resolved alongside the service's existing 162/163/175/176. If 164 fails to resolve, tainted sides fall back to D25 suppression | Accepted |

## Detail and rationale

### D1 / D13 — summary path only
`finish(child, 30₁ᵢₙdₑₓ)` enters the 0-idx 29 loader, which is the only loader that
makes the `scene_result` package resident; a one-shot `30 → 32` redirect then turns
its natural successor (`ResultSequence`) into `TotalResultSequence`. Chain:

```
29 loader (loads scene_result)
  → [redirect 30→32] → 32 TOTAL RESULTS
  → 33 loader (loads scene_game_over + scene_eamusement_window)
  → 34 EAmExitRootSequence   ** credit expire + LOGOUT SAVE **
  → 35 THANK YOU → 36 → attract
```

Cost accepted: the 29 loader emits one spurious POSEVT `"playmusic"` event-log entry
(cosmetic telemetry on a private backend), and the cut out of song select has no
shutter wipe. The missing wipe is *load-bearing*: `TotalResultSequence`'s only exit
gate waits for the shutter to reach the closed state, and it gets there by its own
`close(0)` request — which only works because the shutter is open on entry. **Do not
close the shutter before triggering.**

Known cosmetic consequence: after a Premium Free session the summary will be empty
or near-empty (a frozen counter reuses one play record and `premium_free`'s
stale-record fix virginises it at every song-select entry; the row builder skips
virgin records). Accepted as-is.

### D3 / D11 — bare gesture
Triple-9 during scene 25 fires the transition on the spot. No arm state, no prompt
widget, no `widget_renderer` dependency. This also removes round 1's concern about
what should disarm an armed gesture (browsing songs generates constant menu input),
because there is no armed state to cancel.

### D6 — why no mode gates
Course selection is a *view* of scene 25, and committing a course goes straight to
gameplay and stays there until the course ends, so a course session is never sitting
at music selection mid-course. Two things still need verifying rather than assuming
(R6/R7): that `GameWork+0x70` is genuinely 0 while browsing courses at scene 25, and
that forcing 0-idx 32 in event/special mode (whose vanilla tail uses 0-idx 56)
degrades to the normal summary instead of crashing. If either check fails I will
re-raise this decision rather than silently reinstating a gate.

### D21–D25 — the save-policy inversion (new scope)

**Problem.** `custom_options_persistence::save_sender` currently suppresses
`savekind == 3` outright for any side whose `score_guard::SESSION_TAINTED` flag is
latched. That flag latches the first time a per-stage save is actually suppressed —
i.e. after any Autoplay song, or a Quick Failure. The logout save is the *only*
carrier of the profile / customize write-back (the WebUI cosmetics are
`PersistMode::SaveOnly`, and the workout-profile fields ride the same packet), so
today: **use Autoplay once and no profile edit can be saved for the rest of the
session.** Suppressing the whole packet is heavier than the risk it manages.

**Why the packet was suppressed wholesale in the first place.** The logout save
re-bundles *every* stage's result, including stages whose own per-stage save was
suppressed — so simply letting it through would upload exactly the scores that were
just blocked. The fix has to remove the score content, not the packet.

**Recommended mechanism (D22).** The `savekind == 3` marshal walks stages
`0 .. min(stage_counter, 4)` and **skips any record whose `mcode == -1` or
`end_time == 0`**. So zeroing the tainted side's records before the marshal runs
produces a packet with a full profile/customize block and an empty stage list —
using the exact write `premium_free` already performs for its stale-record fix, on
the exact records it already addresses. Timing: scene 32 (the summary) renders
*before* scene 34 (the save), so sanitising at scene-34 entry leaves the summary
intact; sanitising at trigger time would blank it.

**What must be verified before this is designed (R5).** Whether every score-bearing
field in the `savekind == 3` payload is sourced from those per-stage records, and
whether a zero-stage payload can damage profile aggregates (playcount, playtime,
lamps, unlocks) on the way in. If some score field has another source, the fallback
is kbin node removal inside the existing `save_sender` trampoline — the service
already navigates and mutates that tree to inject its `mod_*` children.

**Blast radius (D21).** This changes behaviour for natural session ends too, which
is intended. It does not weaken the per-stage suppression (`savekind == 2` stays
all-or-nothing), and `autoplay` keeps failing closed on `score_guard::is_available()`.

## Research outcomes (Step 4, 2026-07-27)

Detail in `research/mechanism-verification.md` and `research/savekind3-marshal.md`.

| # | Question | Outcome |
|---|---|---|
| R1 | `sequence_finish` AOB unique? | ✅ 1 match on 20260721 (`0x18021DF70`) and 20260616 (`0x18021DB90`) — matches the doc's table. 2-build check per user |
| R2 | 29-loader always loads `scene_result`? | ✅ `0x10000` is the unconditional default mask; the only variant (`0x30000`, extra-stage) is a superset |
| R5 | savekind==3 payload | ✅ Every stage block gated on `mcode != -1 && end_time != 0`; all score content record-sourced; `mcode = -1` is sufficient. Course sessions marshal the course record at `PlayerWork+0x2D8` instead of the array. One inert residual (class-9 extra-stage grade block) documented |
| R6 | Forcing 0-idx 32 in course/event mode | ✅ Course fork never touches `scene_result` and exits via its own shutter close; event mode is read nowhere in the update, and `getNextID` merges `0x21`/`0x39 → 0x22`. **No mode gates needed** — D6 stands |
| R7 | Course records | ⚠️ Sanitiser must also virginise the course record (`+0x2D8`, decodable from `stage_record_accessor`+36). Folded into D23/D24 |
| R4 | ark network-step confirmation global | Skipped — needs a new AOB for a log line; D14's scene-timing diagnostics cover the failure mode |
| R3 | Numpad inside options modal | Deferred to the cabinet test (low stakes) |
| new | Re-entrant scene transition from the input callback | ✅ Safe: `input_manager` dispatches callbacks outside its lock; `scene_manager` hooks take their mutex in disjoint scopes. The synchronous re-entry is what makes `current_scene()` the free double-fire latch |
| **R8** | bemani-buddy `savekind==3` semantics (`crates/game-server/src/handlers/ddr_world/playdata.rs`) | ✅ Regular song results in savekind=3 are **ignored** (already saved per-stage); the `<result>` list is consumed only for Dan-course results (which update Dan grades — so a tainted course record MUST be virginised); `<league><current>` is stored directly → the D26 league strip; `increment_play_count` still fires on a sanitised save (desired); `<grade>` cursor state passes through (not score — left alone) |
| **R9** | AVS node-removal API for D26 | ✅ libavs **Ordinal 164** = `property_node_remove(node)`, confirmed by decompile on libavs-win64_20260324 (self-identifying log string). Same numeric-ordinal resolver the service already uses |

---

Readiness Confirmed 2026-07-27.
