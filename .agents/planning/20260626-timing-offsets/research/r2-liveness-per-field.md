# R2 — Per-field liveness (when does a config-map write take effect?)

**Question (requirements Q6):** for each of the four offsets, does the consuming subsystem
read the published config-map value **fresh** (live-honoring) or **latch** it at
init/gameplay-start? This determines which fields update in real-time vs. next-song/boot.

**Answer: all four offsets are LATCHED at gameplay entry, into the `GamePlayActor`.** A
live config-map write therefore takes effect at the **next `GamePlayActor` construction**
(i.e. the next song / gameplay entry), NOT mid-song. This matches the maintainer's intuition
(Q6/A): real-time mid-song change is not a realistic scenario; "next song" is the practical
granularity.

## Evidence

### The only getter consumer is the GamePlayActor constructor

The config-map int getter (`FUN_1801ace10` @ 20260324 / `FUN_1801ae680` @ 20260526) has
**exactly 4 call sites, all inside the `GamePlayActor` constructor**
(`FUN_18005b4c0` @ 20260324 / `FUN_18005a6b0` @ 20260526). Near the end of the ctor:

```c
if (DAT_1806ebcf0 != 0) {                                   // config map exists
    FUN_1801ace10("SOUND_OFFSET",      actor + 0x16c);      // → GamePlayActor+0x16c
    FUN_1801ace10("INPUT_OFFSET",      actor + 0x170);      // → +0x170
    FUN_1801ace10("RENDER_OFFSET",     actor + 0x184);      // → +0x184
    FUN_1801ace10("BOMB_FRAME_OFFSET", actor + 0x188);      // → +0x188
}
```

(The ctor's vtable assignment `*param_1 = sequence::dance::GamePlayActor::vftable` confirms
the class identity. Cabinet build `FUN_18005a6b0` latches identically into the same offsets
via `FUN_1801ae680`.)

So once gameplay starts, the engine reads the offsets from the **actor's own fields**
(`+0x16c/0x170/0x184/0x188`), a snapshot of the config map at construction time. No code
path re-reads the live config map during a song for these keys.

### Writers of the config map

Two writers, both via the int setter (R1's apply lever):
1. **Boot publisher** (`FUN_18002bbd0`/`FUN_18002bbb0`) — runs once at sound/input subsystem
   init; seeds the four keys from the selected preset record.
2. **Settings re-publisher** (`FUN_18002e2b0`/`FUN_18002e180`) — re-publishes
   `base + userDelta[i]` for each offset. Reached only through a settings/option-apply
   handler table (its two wrappers `FUN_18002d800`/`FUN_18002dc80` are referenced only from
   a function-pointer dispatch table at `.rdata 0x18035a580`, not from a per-frame/per-song
   path). I.e. it fires when timing settings are applied via that menu, not every song.

Because **both** writers funnel through the single int setter, a setter-hook (R1/R4)
overrides every published write — including any settings re-publish — so our value can't be
silently clobbered by the game re-publishing.

## Practical liveness, per the apply mechanism (see R4)

- **Apply lever = hook the int setter** and force the four keys to our configured values.
  Each time the game (or our own seed call) writes one of the keys, the hook substitutes
  our value, so the config map always holds our value going forward.
- The GamePlayActor ctor then latches our value at the next gameplay entry. Net effect:
  **a change made in the overlay applies on the next song.** This is the same for all four
  fields — none is read live mid-song.
- A change made while sitting in a menu (before entering a song) applies to that upcoming
  song, which is the realistic tuning workflow.

## Documentation conclusion (feeds the design + README)

All four fields behave identically: **changes apply on the next gameplay entry (next song),
not mid-song.** There is no per-field difference in liveness to document — it's a single,
uniform rule. We do NOT attempt to force a mid-song re-latch (would require reaching into
the live GamePlayActor's `+0x16c..` fields during play; out of scope per Q6, and pointless
for the stated use case). The mod's docs/log will state: "Timing offset changes take effect
on the next song."

> If a future need for true mid-song change arises, the lever would be writing the live
> `GamePlayActor+0x16c/0x170/0x184/0x188` fields directly (the actor pointer is reachable,
> but this is explicitly out of scope now).
