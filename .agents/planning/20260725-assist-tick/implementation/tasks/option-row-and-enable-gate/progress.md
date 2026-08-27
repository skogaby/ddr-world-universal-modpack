# Progress — Step 5 Task 02: option row + enable gate + lifecycle

- [x] `ASSIST_TICK_ENABLED[2]` / `LATCHED_ENABLED[2]` atomics; `on_option_change` = bounds-checked
      atomic store only (fires on init/render/save-load/JSON-prime threads)
- [x] Row registered in `enable()`: `RegisterSpec::bool_toggle("assist_tick").default_value(0)`
      — builder-default `PersistMode::Full`; `Duplicate` = success + reseed from `get_value`;
      registration failure / custom_options unavailable → warn, both enables stay OFF (silent mod,
      not gateless)
- [x] FR-8 latch: enables snapshotted into `LATCHED_ENABLED` at GAMEPLAY entry, before arming the
      rebuild; judge path never calls `get_value`
- [x] FR-5 completed: `choose_actor` filters candidates by latched enables (0 enabled → `None`;
      1 → that actor, covering solo/doubles/2P-one-side; 2 → side 0). Doubles gated on the actor's
      own `+0x84` side
- [x] Inert path: no enabled side → one info line, **no Results walk at all**
- [x] Degraded-mode refinement: walk unavailable + dispatched side disabled → rebuild flag
      re-armed instead of latching, so the other side's actor can claim it
- [x] `disable()`: resets all four atomics (plus the existing cleanup); FR-10 honored — no
      score_guard reference anywhere
- [x] Gates: `cargo check` 0, `cargo fmt` clean, `./build.sh` 0; installed (sha256 match)

## Verification record (boot-sanity, 2026-07-27 — deliberately light per the maintainer's call)

One live session showed the whole gate:

```
01:10:28  registered ASSIST TICK option on the MODS tab
          enabled (… latency offset 125 ms, live via overlay)   ← maintainer's overlay tuning
                                                                   persisted + reseeded (permanence
                                                                   path exercised incidentally)
01:12:54  song 1 (default OFF): no participating side has ASSIST TICK on (sides=[0]) -- song inert
          [maintainer toggled the row ON in-game — "enabled successfully"]
01:14:31  song 2: song build -- … chosen_side=0 results=438 kept=340 … + clock line
```

FR-7 (row functions) and FR-8 (change applies from the next song) demonstrated live; OFF-inert
builds no list. The remaining Step 5 behaviours (card-out/in + relaunch persistence, runtime
disable, FR-10 upload, label after one relaunch — the texture is installed) are the maintainer's
end-of-step manual pass, per their scope decision.

No deviations. Commit deliberately not made (maintainer owns commits).

Status: Complete
