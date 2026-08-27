# Plan — Real-Time Calorie Display (Power User Statistics)

Short plan for a **live "calories burned this song" line** on the in-gameplay
Power User Statistics text widget. Single repo (`ddr-world-universal-modpack`).

Read alongside `docs/calorie_weight_profile_research.md` (§3, §3.1 — the calorie
actor, its fields, and the running accumulator).

> If this grows beyond the scope below, promote it to a full `/pdd` feature. For
> now it's a small, self-contained addition to an existing mod.

---

## Motivation

We now let players set WEIGHT + DISPLAY BURNED CALORIES in-game
(`webui_options::profile_fields`). The natural companion is showing the calories
they're burning **live during the song**, as an extra line on the existing
Power User Statistics gameplay overlay (next to EX / ms-error).

## Feasibility — the one fact that makes this cheap

**Don't recompute the formula — read the game's own live counter.** The game's
`CalcCalorieActor` maintains a **running per-stage kcal accumulator at
`actor+0x94`** (RE doc §3.1):

- slot 6 (per-frame tick, `FUN_180053470`) does `actor+0x94 += inc` while a
  measurement window is open;
- slot 5 (finalize) commits `actor+0x94` → `PlayerWork[+0x5dc + dayIndex*0x2b8]`,
  which the result-screen CalorieTab sums with `today_cal` for the shown total.

So `actor+0x94` is exactly "kcal burned in this song so far," in the **same
integer unit** the result screen shows and the profile saves. Reading it:

- needs **no re-derivation** of the step-pattern class (`+0xC8`) / intensity
  (`+0xD0`) classification the game does internally, and
- **sidesteps the weight-unit anomaly** (RE §3.1) — we display precisely what the
  game counts, independent of whether stored weight turns out to be scaled.

`actor+0x58` = playSide; Single subtype size `0xd8`, Double `0xe0`; base ctor
`FUN_180053340` sets the `"CalcCalorieActor"` name label @ `0x18035f438`; vtables
Base `0x18035f458` / Single `0x18035f4b8` / Double `0x18035f518` (0x60 apart).

## Approach (recommended): cache the live value from the actor's own tick

Detour the calorie actor's **per-frame tick** (slot 6, `FUN_180053470`). On each
call: read `[RCX+0x58]` (side) and `[RCX+0x94]` (kcal), store into a per-side
`static [AtomicI32; 2]`, then call the original. The widget reads the atomic.

Why the tick and not the ctor: we only ever touch the actor **from inside its own
live tick**, so there is **no dangling-pointer / lifetime problem** — we never
hold a raw actor pointer across frames, only the cached integer. One detour
covers both subtypes (slot 6 is shared Single/Double).

- Reset both atomics to 0 on gameplay entry (scene 28/GAMEPLAY) so a stale value
  from the previous song can't flash before the first tick.
- The widget appends `Cal: {kcal}` whenever the timing-stats block shows — see
  the gating decision below.

**Gating (revised per user):** there is **no separate calorie toggle**. The
calorie line is part of the renamed **REALTIME GAMEPLAY STATISTICS** block
(`timing_stats` option), always shown when that block shows. This is sound
because the calorie calc is **always available** during normal gameplay (see the
resolved open question) — it does not depend on the profile's `is_disp_weight`.

**Alternative considered (not chosen):** hook the base ctor `FUN_180053340`,
store the actor pointer(s), read `+0x94` at widget time. Rejected: requires
managing actor lifetime (clear before free) to avoid a dangling read — the
tick-hook avoids that class of bug entirely.

## Components & files to touch

| File | Change | Status |
|------|--------|--------|
| `src/core/signatures.rs` | Add `calc_calorie_tick` — AOB on the slot-6 tick prologue (byte-identical 20260324 @`0x180053a50` / 20260616 @`0x180053470`; two short-jump displacements wildcarded). | ✅ done (Step 1) |
| `src/mods/power_user_statistics/calorie_feed.rs` (new) | slot-6 `GenericDetour` (mirrors `data_feed::install` — store-before-enable, `OnceLock`); hook reads `+0x58`/`+0x94` → `REALTIME_KCAL[side]` atomic; `reset()` zeroes both; `latest(side)` reads. Bring-up log-on-change spike. | ✅ done (Step 1) |
| `src/mods/power_user_statistics/mod.rs` | `init`: `calorie_feed::install` (flag `calorie_feed_installed`). scene cb: `calorie_feed::reset()` on GAMEPLAY entry. | ✅ done (Step 1) |
| `scripts/gen_option_labels.py` | Relabel `timing_stats` → `"REALTIME GAMEPLAY STATISTICS"` (regenerated). **No** new `realtime_calories` label (no separate option). | ✅ done (Step 1) |
| `src/mods/power_user_statistics/timing_stats_widget.rs` | Append a `Cal: {kcal}` line via `calorie_feed::latest(side)` to the existing widget text, shown under the same `timing_stats` gate. | ✅ done (Step 3) |
| `README.md` / `AGENTS.md` | Note the calorie line under Power User Statistics (part of the timing block, not a new toggle). | ✅ done (Step 3) |

## Data model

- `static REALTIME_KCAL: [AtomicI32; 2]` — latest `actor+0x94` per side, updated
  each frame by the tick hook, read by the widget. Reset to 0 on GAMEPLAY entry.
- Actor fields read (never written): `+0x58` playSide (i32), `+0x94` kcal (i32).
- **Unit (empirically confirmed on cabinet):** `+0x94` counts small-calories
  (cal); the game's own display shows **kcal = cal / 1000**. Observed 408 cal →
  "0.4 kcal" in-game. The widget therefore renders `Cal: {:.2}` of
  `latest(side) as f32 / 1000.0` (e.g. `Cal: 0.41`). (The result-screen tab
  `FUN_1800e9d90` divides its own animated accumulator by 100 for a differently-
  labeled "consumed" number via digit sprites — not the same quantity/format as
  the live kcal display; we match the live display.)

## Error handling / degradation

- Signature miss → `calorie_feed::install` returns false, mod logs WARN
  (`calorie_feed_installed = false`), the calorie line never appears (rest of the
  mod unaffected) — mirrors `data_feed_installed` gating.
- One detour per target (no other mod hooks `FUN_180053470`).
- Hook body: read two fields + store an atomic + call original; no locks, no
  allocation, panic-free (matches `judge_submit_hook` discipline).
- Update cadence: the widget refreshes on `judge_submit` (existing tick), so the
  shown kcal is at most one judgment stale even though the atomic updates
  per-frame — acceptable for a stat line.

## Testing (live deploy + DebugView; no unit tests per repo)

1. `cargo check --target x86_64-pc-windows-msvc` + `./build.sh` clean ✅. Copy the
   regenerated `seop_item_timing_stats.png` into the cabinet `data_mods/`.
2. **Step 1 spike:** deploy, enable Power User Statistics, play a song → the log
   shows `calorie_feed: side=N kcal=… (was …)` incrementing live; the end value
   matches the result-screen calorie delta for that stage (validates field/unit).
   Confirm the signature resolves at boot (init signature log).
3. Per-side: versus (two Single actors) logs each player's own value; double logs
   the one player's value.
4. (Step 3) `Cal: N` renders on the REALTIME GAMEPLAY STATISTICS widget under the
   `timing_stats` gate.
5. Degradation: with the signature unresolved, no crash, no calorie log/line.

## Open questions

- ✅ **RESOLVED (static, 20260616): the `CalcCalorieActor` is created regardless
  of `is_disp_weight`.** In `FUN_18005be50` the Single/Double actor is constructed
  unconditionally in the normal-gameplay block (gated only by `param_1+0x280 == 0`,
  a HUD-suppression flag — there is no `+0x28`/`is_disp_weight` check near the
  construction). So the calc always runs; `is_disp_weight` only gates Konami's own
  on-screen display. ⇒ the calorie line can be always-on with the mod. (Consistent
  with the byte-identical tick on 20260324; worth a one-line cabinet confirmation
  via the Step 1 spike log.)
- Confirm `+0x94` reads as plain int kcal (expected) and increments as observed —
  covered by the Step 1 spike.

## Step checklist

- [x] **Step 1 — Signature + capture spike.** `calc_calorie_tick` added
  (AOB verified unique + byte-identical across both builds); `calorie_feed.rs`
  (detour + `REALTIME_KCAL` + `reset`/`latest`) wired into `mod.rs` (install +
  gameplay-entry reset) with a log-on-change spike; `timing_stats` relabeled.
  `cargo check` + clippy + `./build.sh` clean. **Pending: cabinet spike log
  confirmation.**
- [ ] **Step 2 — (folded into Step 1)** capture module done; only live
  confirmation of the value/lifetime remains (deploy + read the spike log).
- [x] **Step 3 — UI + docs.** `Cal: {kcal}` line added to `timing_stats_widget`
  (via `calorie_feed::latest`, sixth line, under the existing `timing_stats`
  gate); widget seed text + capacity updated. Bring-up log-on-change spike
  trimmed to a silent cache (the widget is the display now; `install` still logs
  once). README (Power User Statistics entry) + AGENTS (Key Entry Points row)
  updated. `cargo check` + clippy + `./build.sh` clean.
- [x] **Step 4 — Validate (cabinet).** Confirmed on cabinet: the `Cal:` line on the
  REALTIME GAMEPLAY STATISTICS widget tracks live and reads in kcal (raw `+0x94`
  cal ÷ 1000, 2 decimals) matching the game's own display (408 cal → 0.4 kcal).
  Calorie accumulation is present in normal (non-autoplay) play, consistent with
  the unconditional actor construction. **Feature complete.**

