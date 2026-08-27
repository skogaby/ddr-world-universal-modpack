# Time-Critical Hooks and Hook Dependencies

> Research compiled by an exploratory agent on 2026-05-22. Verify
> file:line citations against the current source before acting.

## 1. Race-Sensitive Hooks

### The musicdb.xml Scaling Race (Headline Case)

**Evidence**: User report — when `musicdb.xml` has > ~2000 songs,
the game crashes on bootup ~75% of the time. Strongly suggests a
race between game code and our hook installer.

**What happens**:

1. On boot, `master_loader` (gamemdx.dll + 0x18019F230) calls
   `musicdb_parser` (0x1801A0EC0) — see
   `docs/omnimix_song_limit_research.md`.
2. The parser allocates a 1 MB buffer and reads
   `/data/gamedata/musicdb.xml` into it.
3. If buffer is still 1 MB when `musicdb.xml` is large: stack/heap
   corruption -> crash.
4. If `SongLimitExpansionMod` has patched the 6 sites first:
   8 MB buffer -> parse succeeds.

**Why ~75% failure rate?** The init sequence runs ~150-250 ms.
The boot-to-musicdb-parser window appears to fall in the 100-200 ms
range, so it's a coin flip whether our patches land in time.

**Code locations**:
- `SongLimitExpansionMod::init()`:
  `src/mods/song_limit_expansion.rs:68-134` (scan + verify).
- `SongLimitExpansionMod::enable()`:
  `src/mods/song_limit_expansion.rs:136-148` (write patches).
- 6 patch sites: 3 parsers (license/musicdb/coursedb) x 2 sites
  (alloc + read).

**Buffer math** (from `docs/omnimix_song_limit_research.md`):
- ~463 bytes/song in XML.
- 1 MB / 463 ~ 2,265 songs (upper bound).
- 8 MB / 463 ~ 18,100 songs.

## 2. Race-Sensitive Hook Inventory

| Hook | File:Line | Function | Must Beat | Evidence |
|------|-----------|----------|-----------|----------|
| **SongLimitExpansionMod patch** | `src/mods/song_limit_expansion.rs:68-148` | Expand XML buffer 1 MB -> 8 MB by writing 6 bytes in gamemdx.dll | `master_loader` -> `musicdb_parser` (~100-200 ms into boot, variable) | 75 % crash rate on > 2000 songs; `docs/omnimix_song_limit_research.md` |

This is currently the **only** hook with documented race behavior.
Other hooks fire on player input or gameplay frames, well after
init has completed.

## 3. Late-Binding-Tolerant Hooks

| Hook | File | Fires When | Window |
|------|------|------------|--------|
| Input callbacks | `src/services/input_manager.rs` | Arcade button press/release (60 Hz poll on render thread) | Any time after enable; player can't press during init |
| Scene change callbacks | `src/services/scene_manager.rs` | Scene transition | Any time; triggered by player action after init |
| Judge callbacks (post-register) | `src/services/judge_hook.rs` | During gameplay, per frame | Must exist before song-select -> gameplay; safe |
| Widget lifecycle | `src/services/widget_renderer.rs` | On-demand widget create/destroy | Any time; deferred to render thread |
| Mod menu | `src/mods/mod_menu.rs` | Player triple-5 gesture | Any time after enable |

All of these are reactive. The init phase ends well before any of
these triggers can fire, so they're not race-sensitive.

## 4. Hook-Dependency Graph

### judge_hook is the Shared Dispatcher

```
judge_hook (single retour detour on GamePlayActor::judgeNotes)
  Installed by services::judge_hook::init() — before mods register.
    |
    +- AutoplayMod::enable() -> register_pre(Late, autoplay_callback)
    |   (swap IFootPanel -> AutoFootPanel before judge runs)
    |
    +- NoteTypesExpansionMod::enable()
        +- register_pre(Early, mark_mines_skipped)
        +- register_post(Early, dispatch_mine_results)
```

**Why install before mods register?** CLAUDE.md rule 5: never
install two independent `retour::GenericDetour` on the same
function. Multiple mods need callbacks on `judgeNotes`, so the
shared dispatcher must already exist by the time mods enable.

### Other Dependencies (Mostly Temporal)

```
widget_renderer::init()
+- install render_function_hook  -> captures font pointer on first frame
   v (font_ptr must be populated before widgets can render)
splash_screen_thread (deferred)
+- poll for widget_renderer::is_available() (10 ms x 300 = 30 s timeout)
   +- run_on_render_thread -> create widgets
```

```
custom_options::init()
+- depends on afp_patcher::init() (for AFP template patches)
   v
options_scroll::init()
+- depends on custom_options (reads option-tree state)
```

### No Code-Level Dependencies Between Mods

Mods do not call each other's code at init. They only:
1. Register callbacks against shared dispatchers (judge_hook) at
   enable time.
2. Declare `required_signatures` so they're skipped if their
   needs aren't met.

## 5. MusicDB Deep-Dive

### Detailed Flow

```
main()
+- ... game systems init ...
    +- master_loader [gamemdx.dll + 0x19F230]
        +- musicdb_parser [+0x1A0EC0]
        |   +- MOV EDX, 0x100000        <- patch site #1 (alloc size)
        |   +- malloc(0x100000)
        |   +- MOV [RSP+0x20], 0x100000 <- patch site #2 (read size)
        |   +- open /data/gamedata/musicdb.xml
        |   +- read into buffer
        |   +- parse XML
        |   +- if parse overflows buffer -> crash
        +- coursedb_parser  (sites #3, #4)
        +- license_parser   (sites #5, #6)
```

### Patch Sites

| Parser | Site | Pattern | Patched Byte |
|--------|------|---------|--------------|
| license | alloc | `ALLOC_PATTERN` (`45 33 C0 BA 00 00 10 00 E8`) | offset +6: 0x10 -> 0x80 |
| license | read | `READ_PATTERN` (`C7 44 24 20 00 00 10 00`) | offset +6: 0x10 -> 0x80 |
| musicdb | alloc | `ALLOC_PATTERN` | offset +6: 0x10 -> 0x80 |
| musicdb | read | `READ_PATTERN` | offset +6: 0x10 -> 0x80 |
| coursedb | alloc | `ALLOC_PATTERN` | offset +6: 0x10 -> 0x80 |
| coursedb | read | `READ_PATTERN` | offset +6: 0x10 -> 0x80 |

All 6 must be patched; any unpatched parser can overflow.

### The Race Window (Hypothesis)

```
t=0       DLL injection
t~10-50   Polling for gamemdx.dll
t~60-120  Signature scan completes
t~120-150 SongLimitExpansionMod::enable() patches 6 bytes <- critical
                                vs.
t~120-180 Game boot reaches musicdb_parser() <- crash if not patched
```

The 75 % crash rate suggests boot-to-parser median is ~150-180 ms,
right where our patches land. Pulling our patches earlier is the
core requirement.

### Strategies to Eliminate the Race

1. **Faster scan + earlier patch.** Reduce init from ~150-250 ms
   to ~50-80 ms via multi-pattern single-pass scanning. Lower
   probability of losing the race, but not zero.

2. **Suspend game threads during install.** Detect gamemdx.dll
   load -> suspend all non-init threads -> install patches -> resume.
   Eliminates the race entirely. (See main-thread response for
   tradeoffs.)

3. **Hook musicdb_parser entry itself.** Detour the parser; on
   first call, ensure patches are applied before falling through.
   Requires hooking another function but bounds the race window.

4. **Patch on LoadLibrary detection.** If we can intercept
   gamemdx.dll's `LoadLibrary` (rather than poll), we get a
   deterministic earliest-possible callback. This could be
   combined with #2.

## Key Observations

- The musicdb race is not "scanning is slow," it's "we don't have
  a deterministic install point earlier than the game's first read
  of musicdb.xml." Faster scanning **shrinks** the race window;
  thread suspension or hooking the parser **eliminates** it.
- All other hooks tolerate late binding because they're reactive.
  The optimization budget should focus on the musicdb hook
  specifically and treat the rest as "as fast as is convenient,
  not critical."
