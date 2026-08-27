# R2 — Target data flow, liveness, and the apply-lever decision

**Status: CONFIRMED, fresh.** Traces where the FPS value (R1) actually goes, answers the
**"re-read per frame vs. consumed once"** question (idea-honing Q4), and decides the apply
lever (Q6). Addresses file-relative to `gamemdx.dll` base `0x180000000` (build 20260324).

## The full data-flow chain (re-traced this session)

```
Application::onBoot()  FUN_1800020f0
   ├─ [RSP+0x6c] = 0x3C   (struct_base+0x1C; 0x4B if MachineType==1)   ← R1 imm32 site
   └─ LEA RCX,[RSP+0x50] ; CALL FUN_1801f0030(&struct)
                                  │
        FUN_1801f0030(struct)  ──┤  (prologue PUSH RDI; SUB RSP,0x40 — RCX passed through)
           └─ CALL FUN_1801eda10(struct)        ; RCX = struct, unmodified
                       │
              FUN_1801eda10(param_1)            ; the window/display-mode setup
                 ├─ DAT_1806ea490 = *(param_1+0x14)   ; (adjacent field)
                 ├─ DAT_1806ea48c = *(param_1+0x18)   ; (adjacent field)
                 └─ DAT_1806ea488 = *(param_1+0x1c)   ; ◄── THE FPS TARGET → global
                       │
           (one reader, also at boot:)
              FUN_1801edd20()  = "Renderer:initGs()"
                 └─ local_130 = DAT_1806ea488 ; packed into a GS/D3D device-config
                    struct, then FUN_18022e6a0() creates the D3D device with it.
```

### Corrections to the prior doc (`hex_edit_porting.md` Hack 5)

The prior (older-model, broad-scope) pass got the **mechanism direction right** but **two
structural details wrong** — exactly what the re-verification mandate was for:

1. **Offset:** prior doc said the target lands at **struct `+0x14`**. It is actually
   **`+0x1C`** (`[RSP+0x6c]` with struct base `[RSP+0x50]`). `+0x14` and `+0x18` are
   *adjacent* fields (`DAT_1806ea490` / `DAT_1806ea48c`) in the same display block. The
   byte-patch lever is unaffected (we patch the imm32 in `onBoot`), but a consumer-hook
   that assumed `+0x14` would have grabbed the wrong field.
2. **Consumer:** prior doc said "`FUN_1801f0030(&struct)` consumes the display target."
   `FUN_1801f0030` **does not read the struct** — it forwards `RCX` to `FUN_1801eda10`,
   which is the function that copies `+0x1C` into the global `DAT_1806ea488`. The actual
   *reader* of the value is `FUN_1801edd20` ("Renderer:initGs()").

## Liveness verdict — CONSUMED ONCE AT BOOT (not re-read per frame)

**`DAT_1806ea488` has exactly two xrefs:**

| Site | Function | Access |
|---|---|---|
| `1801eda93` | `FUN_1801eda10` (display setup, boot) | **WRITE** (from struct+0x1C) |
| `1801eddac` | `FUN_1801edd20` = `Renderer:initGs()` (boot) | **READ** (only reader) |

The single reader runs **once**, during `Renderer:initGs()` inside the `onBoot` chain, and
feeds the value into **D3D device creation** (`FUN_18022e6a0`). There is **no per-frame
read** of the refresh target. (The per-frame loop instead reads the *delta-time* global
`DAT_1806ea714` — a different global; see R3.)

### Design consequences (major)

- **Static value is the right model.** The value is latched into the D3D device at boot.
  Setting it once (before `initGs` reads it) is sufficient and complete.
- **"Changes take effect on restart" is CONFIRMED unavoidable** (idea-honing Q4 accepted
  this). A runtime overlay change can't retroactively change the present interval — the
  D3D device was already created. Applying a new value live would require tearing down and
  recreating the device (out of scope, fragile). So: overlay edits persist to config and
  apply **on next launch**.
- **Per-scene live switching (Milestone 2) is effectively INFEASIBLE via this lever.**
  Combined with R3 (World appears delta-time-correct → likely no menu speedup) and the
  maintainer's friend's live World test, **Milestone 2 should be dropped, not merely
  deferred.** The static value is the whole feature.

## Apply-lever decision (idea-honing Q6): byte-patch the imm32, via `early_apply`

Both levers must act inside the **boot window before `initGs` reads the value** — there is
no post-boot opportunity (value is latched into the device). Given that:

### Recommended: **AOB byte-patch the imm32** (R1 site) in an `early_apply` phase.

- **Precedent is exact:** `song_limit_expansion` faces the same "patch a value before the
  game's boot path reads it" race and solves it with the trait's **`early_apply`** hook
  (lib.rs runs all `early_apply`s right after `resolve_all`, *before* `resolve_derived` /
  service init / mod-init — see `src/lib.rs:88-135`). Its musicdb patch reliably lands
  before the loader reaches it (~750ms into boot). `timer_freeze` / `premium_free` are
  also AOB-resolved byte patches.
- **Convention-compliant:** AOB-resolved (not a hardcoded file offset) — satisfies the
  Q6 clarification and CLAUDE.md rule 9.
- **Capturing genuine stock (Q4):** trivial — **read the imm32 before overwriting it.**
  The stock is the byte already there (`0x3C`, or `0x4B` if a 75Hz cabinet ever existed —
  none do). Store it for the OFF-revert. (Because apply is at boot, "OFF revert" mostly
  means: when OFF/disabled in config, **don't patch at all** → the game keeps its stock
  60. A runtime menu toggle to OFF persists and reverts on next launch.)

### Why NOT the hook (`FUN_1801eda10` / `onBoot`)

A detour on `FUN_1801eda10` (the global-writer) *would* capture the genuine runtime value
and substitute cleanly. But it offers **no timing advantage** (see deadline analysis
below) and is **heavier** than a one-byte write for **zero additional capability** here —
the value is static and the stock is knowable by reading the imm32. The `early_apply`
byte-patch matches the closest in-repo precedent (`song_limit_expansion`), keeps the mod
tiny, and has no FFI callback / allocator / render-thread surface.

## Boot-timing deadline analysis (refined w/ maintainer 2026-06-28)

**Injection model:** spice2x's `-k` loads our DLL *before* `gamemdx.dll`. Our `init()`
polls (`module_resolver::wait_for_game_module`, 10ms) for the game module, then
`resolve_all` → `early_apply`. Boot order:

```
process → spice2x injects our DLL → LoadLibrary(gamemdx.dll) → CRT/static init
  → me::fw + AVS framework bring-up → WinMain → Application::onBoot()
        ├─ 0x18000263d  MOV [RSP+0x6c],0x3C     ◄── byte-patch deadline (imm32)
        │   ...~microseconds of straight-line code...
        └─ 0x1800026b3  CALL FUN_1801f0030 → FUN_1801eda10  ◄── detour deadline (global write)
  → ... → master_loader → musicdb parse (~750ms)   ◄── song_limit_expansion's deadline
```

**Key facts:**
- **Deadline = before `onBoot`'s display-init region** (~`0x180002600`–`0x1800026b3`).
  After that the value is latched into the D3D device; window is gone.
- `onBoot` is **earlier** than song_limit's musicdb deadline, but still well after
  `gamemdx` loads — there's a real init gap (framework + AVS bring-up) between LoadLibrary
  and `onBoot`. `onBoot` is invoked as a framework lifecycle callback (its xrefs are
  data/dispatch-table entries), i.e. *after* framework init.
- **Byte-patch and detour share essentially the same deadline.** The span from the imm32
  line (`0x18000263d`) to the `FUN_1801eda10` call (`0x1800026b3`) is straight-line code
  executing in **microseconds** — so if `early_apply` loses the byte-patch race, a detour
  install would lose too. **The detour is NOT a wider-window escape hatch**; its only edge
  is genuine-stock capture (which we don't need — we read the imm32).
- **Optimism:** song_limit's `early_apply` already reliably beats the ~750ms musicdb
  milestone, proving poll→`resolve_all`→`early_apply` completes early in boot. `resolve_all`
  (the slow part) finishes well before then. The only open question is whether it beats
  `onBoot` *specifically* (earlier than musicdb).

**This is the single empirical timing risk.** Static analysis can't prove boot ordering;
the diagnostic deploy (a Step in the plan) confirms it — log whether the patched value
reaches `initGs` / observe the actual refresh rate on the cabinet.

## Fallback ladder if the in-memory race proves unreliable

1. **(primary) `early_apply` AOB byte-patch** — try first; almost certainly sufficient.
2. **(in-memory escalation, only if it helps) detour `FUN_1801eda10`** — captures stock,
   but **same deadline**, so only worth trying if profiling shows our patch lands *between*
   the imm32 line and the global write (a microsecond-wide gap — unlikely to be the actual
   failure mode). Realistically, if (1) loses, (2) loses too.
3. **(ABSOLUTE LAST RESORT) on-disk patch of `gamemdx.dll`** — AOB-locate the imm32 in the
   on-disk file, back up stock bytes, write the chosen value; applies guaranteed on *next*
   launch (can't patch an already-mapped image). **Strongly non-preferred:** it violates
   this project's core philosophy that **mods apply at runtime, never statically** — no
   existing mod modifies the game binary on disk. Adds lifecycle complexity (un-patch on
   OFF, re-derive after a game update, orphaned-state if the DLL is removed).
   - **NOT a concern:** online ban / data-integrity. Users run strictly on unofficial
     networks with **no binary-integrity checks of any kind** (maintainer-confirmed). So
     the only objection to on-disk is philosophical/maintenance, not safety.
   - Design this only if (1) is empirically proven unreliable on the cabinet.

## Open verification (deferred to deploy, not blocking design)

- **Does `early_apply` reliably beat `onBoot`'s FPS line?** → diagnostic deploy. Fallback
  ladder above if not.
- The `MachineType==1 -> 75` branch remains after patching (R1) — moot (no 75Hz cabinets).
