# Idea Honing: Multiplayer Bot

Register accepted by the maintainer 2026-09-13 (D1–D12 Accepted; D13–D20 Assumed).
D1/D7/D10 accepted as DIRECTION pending Step 4 research (U1–U11); research confirmed all
three (see `research/versus-impersonation-re.md`, `research/bot-controller-re.md`).
D12 revised and re-accepted, D21 accepted, D22–D24 assumed after research.

**Readiness Confirmed 2026-09-13** — register complete (no `Proposed`/`Open` items),
research backing in `research/`, maintainer confirmed proceeding to design.

Decision register. Ordered by blast radius. `Status`: Proposed / Accepted / Overridden /
Assumed / Open. Rationale + rejected alternatives under each ID below the table.

| ID | Decision | Why it matters | Recommendation | Status |
|----|----------|----------------|----------------|--------|
| D1 | Impersonation architecture | How the second GamePlayActor, versus HUD and second results pane come to exist | **B — windowed impersonation**: at the song-select commit set `PlayerWork[bot]+0x4 = 1`, `GameWork+0 = 1`, mirror P1's chart identity into the bot side; restore on leaving the play window. Game builds everything natively. **Research-confirmed** (`research/versus-impersonation-re.md` §1–5) | Accepted 2026-09-13 |
| D2 | Bot plays the SAME chart as the human (song, style, difficulty) | Versus comparison is meaningless otherwise; the 1–10 level is *skill*, not chart difficulty | Same chart, always | Accepted 2026-09-13 |
| D3 | Eligibility gate (runtime, not row visibility) | Where the bot may appear; which side it takes | Engage iff exactly ONE side entered ∧ style SINGLE ∧ not course ∧ event mode 0. **Bot = the non-entered side** (human on P2 pad ⇒ bot is P1). Row always visible; "(1P Only)" in the label carries the constraint (center-arrows convention) | Accepted 2026-09-13 |
| D4 | Option rows + persistence | UI surface + wire schema | Parent `bot_opponent` bool "BOT OPPONENT (1P ONLY)" + child `bot_opponent_level` scalar 1..10 step 1, `ShowWhen::Equals{bot_opponent,1}`, default level 5. Both `PersistMode::Full` (wire `mod_bot_opponent` / `mod_bot_opponent_level`, JSON cache until the bemani-buddy migration). Both menus. Listed in `option_menu_settings` under `header_training_options` right after `autoplay` | Accepted 2026-09-13 |
| D5 | Bot identity on screen | Name shown on HUD name plates, BPL frame, results | Write `BOT LV<n>` (≤ 8 chars: `BOT LV10` fits) into `PlayerWork[bot]+0xC` at the flip; restore the original bytes at the restore point. Stock alternative would read `PLAYER2` | Accepted 2026-09-13 |
| D6 | Difficulty model 1–10 → play quality | The whole point of the level knob | Two per-level curves in ONE pure host-tested module: Gaussian timing error σ(L) + per-note miss probability p(L). Anchors: L=10 σ≈5 ms, p=0 (≈70 % MFC on a 500-note chart); L=1 σ≈75 ms, p≈12 %; geometric interpolation between. Per-play seeded RNG (fresh seed every play incl. quick restart); decisions stable per (note, panel) within a play. Freezes held if the head was hit; shocks always avoided. Constants are tunables for cabinet testing — the SHAPE is the decision | Accepted 2026-09-13 |
| D7 | Score / save policy | Integrity of P1's uploads; keeping the bot off the server | P1 saves NORMALLY (not tainted — their play was real). Bot side carries the autoplay taint as the DLL backstop (per-stage suppressed, logout sanitised) on top of the game's own no-card suppression. **Research-confirmed with one wire-visible effect:** the human's per-stage save carries `/data/mode` = `/data/battle_mode` = 1 (from `GameWork+0`); neither bemani-buddy nor bemaniutils reads them — see D24 | Accepted 2026-09-13 |
| D8 | Where the impersonation ENDS | TOTAL RESULTS (0-idx 32) semantics; logout/credit safety | Bot visible on the STAGE results (30) only. Restore on the first scene change out of the play window {26..30} — covers the natural 30→31, the quick-fail 29→24 redirect and any failure path. TOTAL RESULTS, WaitSequence, EAM exit all run as stock 1P | Accepted 2026-09-13 |
| D9 | Interaction with the 2P BPL Mode frame | A bot session satisfies BPL's eligibility once `side_entered` both + `GameWork+0 == 1` | ALLOW — the BPL score-margin frame + rank badges are the best "versus" readout there is. No special casing | Accepted 2026-09-13 |
| D10 | Bot lane options | Visual readability of the race; bot fairness | Copy the human's `ddr::player::Option` block into the bot side at the flip, then force JUDGMENT TIMING (`Option+0x24`) = 0 and gauge type = NORMAL (a copied LIFE4/RISKY would kill a mid-level bot instantly). **Layout confirmed:** gauge = `Option+0x18` (0 = NORMAL); JUDGMENT TIMING (`+0x24`) no longer needs forcing — the bot controller's grading is immune to it (D12) | Accepted 2026-09-13 |
| D11 | Bot controller placement in the code | One-detour rule on `judgeNotes`; autoplay must keep working for the human | Promote autoplay's foot-panel swap into a shared service (`services/auto_foot_panel.rs`) with a per-side controller `Off \| Perfect \| Imperfect{level, seed}`; `autoplay` sets `Perfect`, the bot sets `Imperfect`. Per-side `AutoFootPanel` buffers sized ≥ 0x58 (fixes the latent 0x40 under-allocation on 20260721+) | Accepted 2026-09-13 |
| D12 | Jitter mechanism | Where imperfection is injected | **REVISED after research:** a mod-owned `IFootPanel` object with a CLONED vtable whose `getPressAge`/`consumePress` (slots 5/6) are ours — `getPressAge(panel) = current_mc − event_mc[panel]`, so the judge's `event = mc − age` lands exactly on the planned `event_mc = note.mc + d` on EVERY build (no clock/stride dependence; slots 2/3 are byte-array reads identical on all builds). The DLL fills the flag arrays itself (replicating `update`'s freeze/shock logic), applying per-note `Hit{d}`/`Miss` decisions with a `blocked_until` rule for dense streams. Zero new detours. Supersedes "edit `pressTime` after `update`" | Accepted 2026-09-13 (revised) |
| D21 | Extra-stage grant under a bot | `FUN_1801ddcd0` (results window-out, stage 0) grants EXTRA STAGE only if EVERY `PW+0x4` side ranks ≥ AAA — a bot that misses AAA blocks the human's grant (never adds one) | v1: **exclude the bot from the check** with one small `GenericDetour` on the grant function that clears `PW_bot+0x4` around the original (scope-guarded, fail-open: without the AOB the stock rule applies and the log says so). Alternative rejected: accept the rule change (regresses extra-stage farming at low levels) | Accepted 2026-09-13 |
| D22 | Per-side SE panning | The sound manager's versus flag (`sound_mgr+0x20c4`) is set from `GameWork+0` at CAUTION (scene 21) — before our flip — so bot sessions keep centre-panned SEs unless we set it | Set the byte at the flip / clear at the restore IF `game_audio` already exposes the sound-manager pointer; otherwise skip (cosmetic). Never a new signature for this | Assumed |
| D23 | Bot side vs the `autoplay` option | Per-side option values outlive the player: the bot side's cached `autoplay` may be ON, which would swap the bot's actor to `Perfect` | Shared service arbitrates: `Bot` controller > `Perfect` for that side (one INFO); the autoplay watermark asks the service (`controller(side) == Perfect && side_entered`) instead of its own flag | Assumed |
| D24 | Wire `mode`/`battle_mode` = 1 in the human's per-stage save | The marshal copies `GameWork+0`; scene 30's save runs while the flip is active | Leave as the game writes it (honest; usable later as a server-side bot marker). The ess `save_sender` trampoline could zero staging `+0x44`/`+0x12C` if a backend ever objects | Assumed |
| D13 | Live-toggle semantics | When an edit takes effect | Rows read at the song-select commit (per song). Mid-song edits apply next song. Turning the parent OFF at song select ⇒ next song is plain 1P | Assumed |
| D14 | Training mode + bot | 2P training exists (shared timeline, P1 governs) | No special casing: a bot in a training session just autoplays the looped section. Verify on cabinet; refuse only if it misbehaves | Assumed |
| D15 | Quick restart / quick fail with a bot | Restart = fresh DPS; fail = skip-results redirect | Restart re-reads `PlayerWork` ⇒ bot respawns with a fresh seed. Fail follows D8's restore. Stale-record virginise (quick_restart's `PENDING_RECORD_VIRGINISE`) runs for both sides — the bot's record header is re-mirrored at the next commit, so no conflict | Assumed |
| D16 | Bot gauge / death | L=1 "decent chance to fail" | Stock gauge on the bot side (NORMAL type per D10). Bot death = the game's stock versus death handling (side shows FAILED, song continues while the human lives). Nothing to add | Assumed |
| D17 | Mod identity / defaults | Registry + menu | Mod id `multiplayer-bot`, name "Multiplayer Bot", default ON in `mods` (the option row itself defaults OFF, so nothing engages until a player turns it on — and the label atlas is flushed once at boot, so default-OFF mods have blank in-game labels) | Assumed |
| D18 | Results pane kind for the bot | Guest vs registered pane | Whatever the game shows for a cardless entered side (expected: the "Simple" guest pane). No forcing | Assumed |
| D19 | Attract demo / non-play scenes | Scope | Never engage outside a real credited session | Assumed |
| D20 | Credits / PASELI safety | A fake-entered side must never settle up | `PlayerWork[bot]+0x8` (payment kind) left untouched and verified `< 0` on a never-entered side (U4); the D8 restore runs long before EAM exit (scene 34) regardless | Assumed (research caveat) |

## Decisions

### D1 — Impersonation architecture
**Question.** How does a 1P session get a second `GamePlayActor`, the versus HUD layout and the second results pane?
**Recommendation.** B, "windowed impersonation". Evidence: actor count is decided by the stage loader's per-side `entered` byte handed to the DPS ctor (`docs/quick_restart_fail_speedup_research.md:857-861`), presumed read from `PlayerWork+0x4`; `GameWork+0` drives the HUD/results layout selectors (`src/mods/two_player_bpl_mode/logic.rs:13`). Flipping both for the play window lets the game do everything natively — no detours on the DPS/results code.
**Rejected.** A "fake entry at song select" — versus song select requires two cursors and both confirms (`docs/premium_free_stale_record_bug.md:253-262`), `EAmExitRootSequence` settles credits for every entered side (`docs/quick_logout_research.md:280-283`), `versus_mirror` would mirror the human's options into the bot. C "loader-struct injection" — avoids `PlayerWork+0x4` but every other `+0x4` reader in the window (READY panel, name, pane visibility) would treat the bot as absent; kept as the fallback if B trips a reader with side effects.
**Depends on research.** U1 (loader reads `+0x4`), U3 (`GameWork+0` sufficiency), U6 (what the commit leaves in the bot's record), U7 (pane visibility).

### D2 — Same chart
Versus is a comparison; the bot must play what the human plays. The level knob is skill. An independent bot chart picker was rejected (adds UI, breaks the comparison, and the two-cursor song select is exactly what D1 avoids).

### D3 — Eligibility gate
Exactly one side entered ∧ SINGLE ∧ !course ∧ event 0. Versus cannot be doubles (`GameWork+0 = 0` for doubles, `docs/in_shop_battle_local_versus_research.md:57`). The human may be on either pad — the bot takes the empty one (center-arrows already computes `active_side` this way, `src/mods/center_arrows_single.rs:223-266`). The row stays visible in all sessions like `center_arrows_1p`; per-side values outlive the player, so the gate MUST read `stage_records::side_entered`, never the option values (`.agents/learnings/learnings.md:731-750`).

### D4 — Option rows + persistence
`RegisterSpec::bool_toggle("bot_opponent")` + `RegisterSpec::scalar("bot_opponent_level", 1, 10, 1, ScalarFormat::Integer)` with `ShowWhen::Equals` — the `assist_tick`/`assist_tick_volume` shape (`src/mods/assist_tick.rs:1531-1558`, `:1091-1133`). `Full` persistence so a player's preference sticks across sessions; `Session` was rejected for that reason. Default level 5. Textures: `LABELS` + `PREVIEWS` entries in `scripts/option_strings.py` (en/ja/ko required), regenerate.

### D5 — Bot name
`PlayerWork+0xC` is an inline 8-char name; the game's `getName` prints `PLAYER1/2` for an entered nameless side (`docs/in_shop_battle_local_versus_research.md:483-487`). Writing `BOT LV<n>` makes the level visible everywhere the name is drawn (HUD plates, BPL frame, results) for free. Restore the original 9 bytes at the D8 restore point. Alternative `CPU` rejected as less informative.

### D6 — Difficulty model
Two curves, both functions of L ∈ 1..=10:
- σ(L): timing error std-dev in ms, Gaussian, applied per (note, panel). Stock Marvelous is ±17 ms; σ=5 gives per-note P(Marvelous) ≈ 0.9993 ⇒ ≈ 70 % MFC over 500 notes at L=10. σ=75 at L=1 spreads hits across Great/Good/Boo.
- p(L): per-note miss probability. 0 at L=10; ≈ 12 % at L=1 (each Miss costs a large gauge chunk; combined with Boos this fails most charts at L=1 without guaranteeing it).
- Interpolation geometric in L. All constants in one pure module (`bot_skill.rs`) with host tests on the distribution endpoints, tuned on cabinet.
- Seed: fresh per play (restart re-rolls); decisions cached per (note ptr, panel) so a frame never flips an earlier choice.
- Kept simple on purpose: freezes held when the head was hit, shocks always avoided (the game's own `AutoFootPanel` semantics), no streak/fatigue modelling. Future work if wanted.

### D7 — Score / save policy
The human's play is genuine — nothing the bot does alters it. Their `savekind==2/3` proceed as stock. The bot side gets the autoplay taint (the mechanism autoplay already uses, `src/mods/autoplay.rs:72-74`) so even if the game emitted a save for it, `score_guard` suppresses it; the game's own no-card gating (`quick_logout_research.md:339-344`) is the first line. Research U8 checks that the marshal writes nothing versus-specific for the human.

### D8 — Restore point
Restore on the first scene change to any scene outside {26, 27, 28, 29, 30}. Rationale: TOTAL RESULTS aggregates the session per side — a bot present for some songs and not others is nonsense there; and keeping the flag through 31/32 risks the WaitSequence/TotalResult readers (`GameWork+0x8` unchecked index, `PW+0x4` name, stage predicates). Restoring at scene-change time also covers the quick-fail 29→24 redirect and any limbo/failure path.

### D9 — BPL frame
`two_player_bpl_mode::logic::eligibility` = `GameWork+0 == 1 ∧ both entered ∧ event ∉ {1,2} ∧ !course ∧ GAMEPLAY ∧ network idle` — all true in a bot session under D1. Letting it engage gives per-player score boards, ratio gauges, live 1st/2nd badges and the score margin for free. The maintainer may not have considered this; it is the recommended default.

### D10 — Bot lane options
Copying the human's `Option` block makes the bot's lane scroll at the same speed / skin so the race reads visually. Two fields must NOT be copied: JUDGMENT TIMING (`Option+0x24`, would bias every bot hit) and the gauge type (LIFE4/RISKY would make a level-5 bot die on its 4th miss). Needs the Option layout for gauge type (U11); fail-open to stock P2 options.

### D21–D24 — Added after the Step 4 research
D21: the grant function iterates entered sides — the ONLY reader of `PW+0x4` in the window whose outcome changes the human's session. D22: `FUN_1801aa500(GameWork+0 == 1)` at CAUTION → `sound_mgr+0x20c4`, read by `FUN_1801aa220` to pan side 0/1 SEs. D23: arbitration lives in the shared foot-panel service. D24: bemani-buddy ignores both scalars (`handle_save_scores` never parses them); bemaniutils has no World handler.

### D11 / D12 — Controller placement and jitter mechanism
See `research/autoplay-internals.md` §Seams. The judge dispatcher allows multiple subscribers but same-priority order is registration order (`src/services/judge_hook.rs:11-12`), so the swap must have ONE owner: a shared service with a per-side controller. REVISED (research): the controller is a DLL-owned `IFootPanel` with a cloned vtable and its own `getPressAge`/`consumePress`; the judge computes grades from it exactly as for a human, so the gauge, combo, judgement effects, results and graphs all stay stock. Full algorithm: `research/bot-controller-re.md` §4.

### D13–D20 — Assumed
Recorded so the design can be audited; reversible.
