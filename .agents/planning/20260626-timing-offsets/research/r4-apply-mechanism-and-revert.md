# R4 — Apply mechanism + stock-value capture / master-OFF revert

Decides the primary lever for applying the four offset overrides, how to seed at boot, and
how the master-OFF "revert to stock" works.

## Decision: hook the int setter (primary lever); no `.rdata`/builder default-patching

Two candidate levers were in the doc:
1. **Patch the builder defaults** (`.rdata` rec0 ints + the inline imm32s of records 1..9).
2. **Re-set / intercept the published value** via the config-map setter.

**Chosen: lever 2 — hook the config-map int setter** (`FUN_1801acbf0` @ 20260324 /
`FUN_1801ae460` @ 20260526; AOB in R1). Rationale:

- **Single choke point.** The setter is the *only* writer of the four keys (R1: 8 call
  sites, all timing; both the boot publisher and the settings re-publisher go through it).
  Hooking it overrides every write regardless of which publisher runs — robust against the
  settings re-publish path clobbering us.
- **No `.text`/`.rdata` byte-patching.** Default-patching record 0 only fixes preset 0 and
  would need to also patch 9 inline imm32 records to be airtight; and it can't react to the
  settings re-publisher. The setter-hook is one detour, version-stable via the AOB, and
  avoids hardcoded offsets — squarely matches project conventions (CLAUDE.md rule 9).
- **Reuses the game's own data flow.** After the hook substitutes our value, the
  GamePlayActor ctor latches *our* value at the next gameplay entry (R2). No need to touch
  the actor fields or re-run the publish path.

### How the hook applies values

The setter signature is `i64 set(byte* key, i32 value)`. In the detour:
1. Identify which timing key is being set. Cheapest robust method: **FNV-1a hash the key
   arg** (seed `0x811c9dc5`, prime `0x1000193` — same as the game) once at init for the four
   key strings, and compare the incoming key's hash; or simply `strcmp` the key pointer
   against the four ASCII names (the arg is a readable C-string — verified). Either works;
   strcmp on ≤17-char keys is trivial and avoids re-deriving the hash.
2. If the key is one of our four **and** the mod is master-ON, replace `value` with our
   configured value (clamped to `[-1000, 1000]`) before calling the original setter.
3. Otherwise call the original unchanged.

This makes every publish (boot or settings) converge to our values while ON.

## Boot seed

No separate "seed" call is strictly required: the boot publisher *always* runs at subsystem
init and writes all four keys via the setter. With the hook installed before that runs, the
boot publish itself becomes the seed — the hook substitutes our configured values as the
publisher writes them. Two practical notes:

- **Install timing.** The hook must be installed before the boot publisher runs. The boot
  publisher runs at sound/input subsystem init, which is after our DLL's hook-install window
  (the mod's `enable()` runs during init, well before the game's subsystem init). So
  install-in-`enable()` is early enough. (To be confirmed by a one-shot log on first hook
  hit during the deploy test — if the publisher somehow already ran, fall back to an
  explicit post-init re-set via the setter, which is also fine since the keys exist by then.)
- **Explicit re-set fallback / live apply.** For a value changed in the overlay *after* boot,
  the mod calls the original setter directly (`set(key, value)`) to push the new value into
  the live map (update-only is fine — the key exists post-boot). Then it's latched at the
  next song (R2).

## Stock-value capture (for master-OFF revert)

Requirement Q7: master-OFF reverts to the game's **stock** values. We need to know stock to
restore it.

- **Stock defaults are constant and known:** SOUND 87, INPUT 28, RENDER 17, BOMB 0 (R1,
  re-read from `.rdata` on both builds; these are record 0 / the common preset). However,
  the *effective* stock value can differ if the cabinet selects a non-0 preset or the
  settings menu applied a user delta.
- **Robust capture:** when the mod is master-ON and the hook first sees the boot publisher
  write a key, **record the incoming (pre-substitution) value as the captured stock** for
  that key. That's the genuine value the game would have published. On master-OFF, re-set
  each key to its captured stock via the setter (takes effect next song, per R2). If we
  never observed a write for a key (shouldn't happen — boot always publishes all four), fall
  back to the known constant defaults (87/28/17/0).
- This "capture the value the game tried to write" approach is preset/settings-accurate and
  needs no extra getter call, though a getter call (`FUN_1801ace10(key,&out)`) is available
  as a cross-check if desired.

### Master-OFF semantics (matches Q7)

- **OFF = stop overriding + restore stock.** The hook, when master-OFF, passes writes
  through unchanged (so any future publish uses the game's value), and the mod additionally
  re-sets the four keys to captured stock once, so the live map returns to stock. Effect
  lands next song (R2) — documented, consistent with the best-effort-live posture.
- **ON = apply the four configured values** (hook substitutes; plus an explicit re-set so
  the change is live in the map immediately, latched next song).

## What we explicitly do NOT do

- No `.rdata`/`.text` default byte-patching (rejected above).
- No writing the live `GamePlayActor+0x16c..` fields for mid-song effect (out of scope, Q6).
- No touching `HIGH_PRECISION_INPUT` (out of scope, Q1) — the bool setter is left alone.

## Net design inputs

- One detour on the int setter (AOB from R1), key-filtered to the four offsets.
- Per-key state: `configured_value`, `captured_stock`, `master_on`.
- On master toggle / scalar change: push via the original setter for immediate live map
  update; effect latched next song.
- Graceful: if the setter AOB doesn't resolve, the whole mod self-disables (R1/Q8 — this is
  the load-bearing signature).
