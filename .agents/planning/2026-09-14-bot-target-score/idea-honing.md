# Idea honing — Multiplayer Bot "Target Score" tier

Decision register (PDD lite, 2026-09-14). Maintainer approved the whole
register as recommended on 2026-09-14 (`Accepted`); `Assumed` = settled by the
agent, reversible, listed for audit.

**Readiness Confirmed 2026-09-14** — no `Open` decisions; research in
`research/orientation.md` (GhostActor wait/offset/identity gate verified in
Ghidra on 20260825 / 20260224 / 20250805; 20260721 left to the signature sweep).

| ID | Decision | Why it matters | Recommendation | Status |
|---|---|---|---|---|
| D1 | Row shape | The in-game menu can only render TEXT values through a Scalar row (Enum rows are chip textures) | Keep `bot_opponent_level` a **Scalar row 1..=11** (fine = coarse = 1) with a new `ScalarFormat` variant that renders `Level N` for 1..=10 and `Target Score` at 11 — enum-like to the player, scalar donor underneath; both menus render via the shared `format_scalar_value` | Accepted |
| D2 | Persistence | Maintainer: no backend support, ever | New **`PersistMode::Local`** = JSON cache write + JSON prime, NO network save, NO network load; both bot rows switch to it (`mod_bot_opponent*` wire fields stop being emitted). Mechanism: split the shared load gate by source (`LoadSource::{Network, JsonPrime}` on `resolve_from_load`; JSON prime gates on `json_cached()`); matrix tests updated. Existing cached values keep working (ids unchanged) | Accepted |
| D3 | Ghost source | Zero-detour, build-portable | Read the **HUMAN's** `GhostActor` vector (`*(GamePlayActor_h + gpa_ghost_actor_off) → +0x98..+0xA0`) at the bot's first `fill()` of the song (re-read on every Results rebuild). `gpa_ghost_actor_off` is DERIVED from the new `gpa_ghost_actor_probe` AOB (+0x1F8 on 2026-03+, **+0x1F0 on 20250805/20260224**) with the `isReady` callee prologue as identity gate; runtime gates = `memory::is_readable` + RTTI `ghost_actor_vtable` + `state == 2`. Derivation miss ⇒ tier unavailable that boot (WARN once, D5 fallback) | Accepted |
| D4 | Reproduction scope | Score parity with the target needs O.K./N.G. too | **Full**: taps (0..3 sampled in-window, 5 Miss, 4 Boo→Miss, stray 6→Marvelous / 7→Miss), **freeze N.G.** (planner drops the body hold for a head whose tail byte is 7), **shock N.G.** (one press on a shock panel at `mc`). Alternative rejected: taps only (freezes/shocks stay bot-perfect) — breaks the "bot final score == target points" invariant | Accepted |
| D5 | No usable ghost | The impersonation is applied at song select; the GhostActor exists only from GAMEPLAY setup, so the tier cannot refuse at the flip | Fall back to **Level 10** for that song + one WARN naming the reason (`empty` / `id 0` / `len mismatch a≠b` / `derivation missing`) + a 3 s toast `NO TARGET GHOST - BOT LV10` if the toast service is up (fail-open). Plate stays `TARGET`. Rejected: sit-out bot (reads as broken), undoing the flip mid-song | Accepted |
| D6 | S-Marvelous exclusion | Maintainer directive | When `s_marvelous::is_enabled()` AND the bot side is armed with window `W`, a ghost Marvelous samples `\|d\| ∈ [W+1, 17]` (exclusive Marvelous; `W=16` ⇒ exactly 17); otherwise `[0, 17]`. Pure sampler takes `smarv_floor` (0 = none); new `state::armed_window(side)` getter. Levels 1–10 untouched | Accepted |
| D7 | Offset distribution inside a band | Grade is fixed by the band; only FAST/SLOW readouts and the results timing graph see the shape | Magnitude **uniform** over the band's integer range; sign from the existing sticky Markov side chain (`skill::Form` at the L10 curve's bias/stickiness) so a song shows human-looking FAST/SLOW runs | Assumed |
| D8 | Name plate | 8-char plate | `TARGET` (`BOT LV<n>` unchanged for levels). Rejected: `GHOST`; the target's dancer name (needs new RE of the target-selection object) | Accepted |
| D9 | Length mismatch | A differently-modified or truncated ghost misaligns every index | Require `ghost.len() == results.len()` exactly; else D5 fallback + WARN with both lengths (field data decides whether a prefix-mapping relaxation is ever needed) | Assumed |
| D10 | Value encoding | Cache compatibility | Value 11 = Target, stored as-is in the JSON cache; load clamp becomes 1..=11; the pure layer exposes `BotMode::{Level(u8), Target}` from the raw value | Assumed |
| D11 | Diagnostics | Cabinet validation is log-driven | Restore INFO gains `mode=target ghost_len=N target=[m,p,g,gd,miss,ok,ng] repro_miss=N` where `repro_miss` = notes whose planner-resolved grade (after floors) ≠ the ghost byte; existing planner-vs-judge `mismatch` kept. Cabinet invariant to eyeball: bot money score == target's points | Assumed |
| D12 | Song reset / quick restart | Existing re-seed path | Ghost re-read from the same GhostActor on the Results rebuild; seed re-rolled as today (sign chain differs per attempt, grades identical) | Assumed |
| D13 | Host validation | `tools/bot_sim` mounts the pure files | New pure `ghost.rs` mounted + unit-tested (band edges, S-Marv floor, alphabet mapping, planner freeze/shock extensions); no simulator ghost mode in v1 | Assumed |
| D14 | Docs | | AGENTS.md multiplayer-bot row, `docs/multiplayer_bot_research.md` addendum (GhostActor wait + per-build offset), README option text; `option_strings.py` description "1 = beginner, 10 = expert" → mentions Target Score and the preview PNGs regenerated | Assumed |

## Notes per decision

**D1** — `PrefixedIndex`/`Unit` already prove letters render through the value
TextLayer; `Target Score` is 12 bytes (SSO 15). The variant is data-only
(`Labeled { prefix, terminal: (value, label) }`) so the format stays `Copy`
and host-testable in `scalar_format_tests.rs`.

**D2** — Today `Full` is the only JSON-cached mode and the JSON prime rides the
network-load gate. `Local` = `(save no, load no, json yes, session no)`; the
matrix invariant "JSON prime funnels through the load gate" is replaced by
"a JSON-cached mode is never session-scoped". No behaviour change for the four
existing modes.

**D3** — Cross-build evidence in `research/orientation.md` §2.2–2.3. The
GamePlayActor pointer for the human comes from `song_reset::gameplay_actors()`
(both actors, side at `+0x84`).

**D4** — Freeze N.G.: the freeze judge resolves N.G. whenever any body panel was
released, so simply not emitting the body hold for that head suffices. The
head↔tail link is "the next kind-2 entry after the head" (tails carry the head's
panel states); the planner records `drop_hold[head_idx]` when it resolves the
head. Shock N.G.: press at `note.mc` (inside `[mc−34, mc+84]`), same one-event-
per-panel discipline.

**D5** — L10 rather than the default L5: a missing ghost is almost always a
never-scored chart, and the maintainer's framing is "race the target"; L10 is
the closest stand-in. Reversible constant.

**D6** — The S-Marv classification is `|d| ≤ W` inclusive on the live judge
delta, so the exclusive band starts at `W+1`. Planner floors may still push a
resolved event (never into S-Marv — floors only move events LATER/outward).
