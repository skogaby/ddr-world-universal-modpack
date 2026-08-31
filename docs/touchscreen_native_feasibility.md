# Touchscreen-Native UI — Feasibility & Strategy Research

Research record for the question: **can DDR World's own UI be made touch-native** —
touch the game's on-screen elements directly (tap a song jacket, drag the wheel,
tap menu rows), rather than overlaying emulated cabinet buttons (the existing
`smx-hardware` stopgap)?

**Verdict up front: feasible, no fundamental blocker — but the full vision is a
multi-month, per-screen bespoke effort.** The surprise finding is that the game is
built on frameworks that were *designed touch-first at Konami* and shipped in DDR
with the touch input source absent. Three separate layers contain dormant or
reusable machinery: the ark IO layer has a complete (device-less) touch-panel
subsystem, the gamemdx UI layer has a retained-mode Component/VirtualButton
framework with per-button callbacks, and the AFP (Flash) runtime has a fully
intact mouse/hit-test/AS2-event pipeline that nothing feeds. None of these gives
touch "for free" — the authored content and per-screen glue still assume buttons —
but they change the strategy from "reimplement everything" to "feed and steer
existing machinery."

**Binaries examined** (all addresses file-relative to base `0x180000000`):

| Module | Build | Role in this research |
|---|---|---|
| `gamemdx_20260721.dll` | 20260721 | UI framework, InputEventDispatcher, VirtualButton, panels |
| `libafp-win64.dll` | AFP 2.13.7 | Mouse pipeline, hit-test walker, AS2 event system |
| `arkmdxbio2_20260721.dll` | 20260721 | Touch-panel sub-IO, test-menu touch framework |

Cross-version verification has NOT been done for the new addresses in this doc —
every address below is a single-build (20260721) finding unless marked otherwise.
See "Cross-Version Notes" before building anything on them.

---

## Table of Contents

1. [What "touch-native" requires](#1-problem-decomposition)
2. [Layer 1: the ark IO touch subsystem (dormant, device-less)](#2-ark-touch)
3. [Layer 2: gamemdx input & component framework](#3-gamemdx-framework)
4. [Layer 3: the AFP runtime's mouse/hit-test pipeline (dormant, intact)](#4-afp-mouse)
5. [The spatial mapping problem (what is on screen, where)](#5-spatial-mapping)
6. [Candidate architectures](#6-architectures)
7. [Per-screen scope inventory](#7-scope)
8. [Recommended strategy & phasing](#8-strategy)
9. [Key addresses](#9-addresses)
10. [Cross-version notes](#10-cross-version)
11. [Gotchas](#11-gotchas)
12. [Open questions / future RE tasks](#12-open-questions)

---

## 1. What "touch-native" requires <a name="1-problem-decomposition"></a>

Touch-native UI decomposes into four independent problems:

1. **Touch capture** — getting touch/pointer events into the process.
   **SOLVED.** The `smx-hardware` mod's WndProc subclass already delivers
   per-contact press/move/release with window discovery, CrossOver mouse
   fallback, and IR-frame debounce (`docs/smx_hardware_research.md` §4–5).
2. **Spatial mapping** — translating a screen point into "the user touched
   *this* UI element." This is the genuinely new problem. Options range from
   static per-screen layout tables to live queries against the AFP scene graph
   (§5).
3. **Actuation** — making the game *respond* as if the user had navigated to
   and activated that element. Partially solved: menu-button/pinpad synthesis
   is cabinet-proven (digest override words, pinpad pulses). The gap is
   *targeted* actuation ("select THIS song", not "press right 3 times") — §3
   and §6 cover what the binary offers.
4. **Feedback** — the UI visibly reacting to touch. Mostly free when actuation
   goes through the game's own cursor/focus model (the game animates focus
   itself); custom gestures (wheel drag) need bespoke handling.

The existing SpiceManiaX-style overlay solves 1+3+4 by *bypassing* 2 (buttons
drawn by the mod, at mod-chosen positions). Touch-native means solving 2 against
the game's own layouts, and upgrading 3 from "button events" to "semantic
actions" where the interaction model demands it (wheel drag, direct row taps).

---

## 2. Layer 1: the ark IO touch subsystem (dormant, device-less) <a name="2-ark-touch"></a>

The shared Konami arcade-IO framework inside `arkmdxbio2.dll` ("arkCore")
supports touch panels as a first-class input device. DDR's build contains the
complete framework with **no touch device installed**.

### 2.1 Evidence chain

- `DefaultArkCoreIO::touch` = `FUN_180009d20` (arkmdxbio2 20260721). Returns the
  touch sub-IO object at `this+0x88`. When null, it logs
  `msg=pSubIOTouch_, file=programs\io\defaultArkCoreIO.cpp,
  func=DefaultArkCoreIO::touch, line=0x3ca` to `/dev/raw/exception_record.log`
  (rate-limited to 6) and returns null. Disassembly-verified.
- The getter is exposed at **core-IO vtable offset `+0x260`**: `MdxHWIO`'s
  vftable is `0x1800f7c88` (see `docs/smx_hardware_research.md` §1.1) and
  `0x1800f7c88 + 0x260 = 0x1800f7ee8`, one of the 4 vtable slots referencing
  `FUN_180009d20`. Every consumer calls `(*(core_io_vtbl + 0x260))(io)`.
- **No writer of `[reg+0x88]` (64-bit store) exists anywhere in the DLL**
  (masked byte-pattern search over the common REX encodings). `pSubIOTouch_`
  is never populated on MDX — the touch device is absent, not disabled.
- **No export** in the entire ~800-entry export table exposes touch (full
  export list reviewed). The subsystem is ark-internal.

### 2.2 The touch device interface (from the TOUCHPANEL CHECK screen)

`FUN_1800685b0` (arkmdxbio2) is the operator-menu touch calibration screen. It
documents the device interface by use:

```
core_io = FUN_180002630()                    // core IO singleton getter
touch  = (*(core_io_vtbl + 0x260))(core_io)  // DefaultArkCoreIO::touch
(*(touch_vtbl + 0x18))(touch, 0, &out_xy)    // get position: (index, int32[2] out)
(*(touch_vtbl + 0x28))(touch, 0, &calib)     // set calibration (rect block, see below)
```

Calibration is persisted through the settings property tree under
`touchpanelCheck/calibration{LeftUpX,LeftUpY,RightDownX,RightDownY}/current` and
`touchpanelCheck/calibrated/current`, and the calibration block passed to
`vtbl+0x28` is built from screen-percentage products
(`screen_w * calib_x / 100`, …) — the framework expects raw panel coordinates
that it maps to screen space itself.

### 2.3 The test-menu framework is touch-first; DDR rewrites the hints

The single most instructive finding: `gamemdx!FUN_180006b10` (20260721) is the
test-menu hint renderer, and it contains a **17-entry rewrite table converting
touch-first instruction strings to button instructions**:

```
"TOUCH ENTER = EXECUTE"      → "START BUTTON = EXECUTE"
"TOUCH ENTER = DECIDE"       → "START BUTTON = DECIDE"
"TOUCH U/D = SELECT ITEM"    → "PLAYER1 LEFT/RIGHT = SELECT ITEM"
"TOUCH L/R = CHANGE VALUE"   → "PLAYER2 LEFT/RIGHT = CHANGE VALUE"
"TOUCH SCREEN = EXIT"        → " START BUTTON = EXIT"
... (17 total, disassembly-verified)
```

The shared operator-menu framework (also present in arkmdxbio2, same strings at
`0x180104960`/`0x18010cfb8`/etc.) natively renders touch instructions; products
without touch panels string-swap them. This proves the frameworks underneath DDR
were authored for touch cabinets (DANCE aROUND-class hardware) and DDR is the
buttons-only configuration of the same code.

### 2.4 What this layer offers a touch mod

Installing a mod-owned touch device object at `core_io+0x88` (implementing at
least the `+0x18` position getter) is the "most native" injection imaginable —
but its only *known* consumer on MDX is the TOUCHPANEL CHECK screen and
(hypothetically) the ark test-menu navigation. The main game (gamemdx) never
calls the touch getter: no gamemdx import or indirect-call path reaches slot
`+0x260` (the hint-rewrite table exists precisely because the GAME side is
button-driven). **Verdict: interesting for a touch-navigable operator menu
experiment; not the road to touch-native gameplay UI.** (Hypothesis — the ark menu
navigation consuming touch when a device exists is unproven; the hint strings
may be gated on a product config we can't flip.)


---

## 3. Layer 2: gamemdx input & component framework <a name="3-gamemdx-framework"></a>

This is where touch-native actuation actually happens. The sequence layer is not
"actors polling raw buttons" everywhere — menu screens are built on a
retained-mode component framework with an event dispatcher, and (at minimum on
the song-select screen) **logical buttons with attached callbacks already
exist**.

### 3.1 `sequence::InputEventDispatcher`

- RTTI `.?AVInputEventDispatcher@sequence@@` (TypeDescriptor `0x180482120`,
  vftables `0x18035fdc8` + `0x18035fe10`, 20260721).
- Ctor `FUN_18004c440`: two per-player repeat-config entries — initial delay
  **500 ms**, repeat interval **200 ms** (the menu auto-repeat everyone knows).
- The poll/update method (vtable impl `FUN_18004c810`) reads per-player,
  per-button state records (`+7`/`+8` = current/previous level bytes;
  `+4..+6` = repeat bookkeeping) and synthesizes `sequence::InputEvent`s. The
  event's button code is offset by a **modifier constant encoding the edge
  class**: observed `code` (press), `code+0x20`, `code+0x40`, `code+0x80`
  (repeat/long-press/release classes — exact assignment unproven; the raw code
  space is small integers per button). Events are dispatched via
  `(*(dispatcher_vtbl + 0x20))(dispatcher, &event)`.
- Subscribers are `std::tr1::function<void(sequence::InputEvent&)>` lambdas;
  RTTI shows consumers in at least the `selectmusic`, `entry`, and `dance`
  sequence namespaces.
- `sequence::InputEvent` is a plain struct (no RTTI). Verified fields: a
  response map at `+0x08` (rb-tree keyed by small int "slots"), player index
  at `+0x10`, key/code int at `+0x18`. Fields at `+0x20`/`+0x24`/`+0x28` are
  captured by VirtualButton's event handler (see below) — **semantics
  unproven** (tempting to read as x/y/flag, but do not build on that without
  live verification).
- The event carries a **response-arbitration map**: components receiving the
  event don't act immediately — they register response closures at priority
  slots (0, 1, 2 observed) via `FUN_180044ff0(event, slot, closure)`, and the
  dispatcher resolves which response runs. This is how overlapping panels
  (e.g. an open FilterPanel over the wheel) arbitrate input focus, alongside
  `sequence::selectmusic::InputLock`.

**Implication for touch:** injecting synthetic `InputEvent`s at the dispatcher
is possible but adds nothing over the cabinet-proven digest-word injection —
which also reaches ark-internal consumers and the operator menu. The dispatcher
matters for a different reason: it is the *model* for how a future
`TouchEventDispatcher` should deliver events into the component tree, and its
response-map arbitration is the mechanism a touch router must respect (or
reuse) so touches obey the same focus rules as buttons.

### 3.2 `sequence::Component` / `BaseButton` / `Button` / panels

RTTI inventory (all `@sequence@@`): `Component`, `BaseButton`, `Button`,
`GridPanel`, `ReactiveAction<T>`, plus per-screen namespaces. The song-select
screen alone (`sequence::selectmusic`) decomposes into:

```
View, SelectMusicSequence, InputLock,
MusicPanel, MusicInfoPanel, DifficultyPanel, OptionPanel, FilterPanel,
SortPanel, FolderPanel, SidePanel, ClosedPanel, StageSkipPanel, RecordPanel,
RivalPanel, TenkeyPanel, GradePanel, GradePreparePanel, GuideFrame,
VirtualButton, VirtualSelectBox, ScrollBar, FilterButton, ChartMetadata, ...
```

Components form a tree (children registered via `FUN_180045c10(parent+0x68,
&child)`), receive `InputEvent`s polymorphically, and expose enable/visible
state. Lambda signatures in RTTI (`bool(sequence::Component*)`) show
visitor-style tree walks.

### 3.3 `sequence::selectmusic::VirtualButton` — the smoking gun

Object size 0x148 (CRT heap), ctor `FUN_18004f860` (20260721), two vftables
`0x18035fee0` / `0x18035fe98` (primary + `+0x28` MI base shared with
`Component`).

| Offset | Type | Field | Evidence |
|---|---|---|---|
| +0x00 | ptr | primary vftable | ctor |
| +0x28 | ptr | MI-base vftable (Component interface) | ctor |
| +0x30 | u16 | state/flags word (`0x100` written at FilterButton wiring) | `FUN_180127310` |
| +0x31 | u8 | sub-flag, zeroed at wiring | `FUN_180127310` |
| +0x68 | list | child/registration list (Component base) | `FUN_180045c10` usage |
| +0xA0 | f64 | width — **default 10.0**, overwritten from host FilterButton `+0xA0` (cell_width 108.0) | ctor + `FUN_180127310` |
| +0xA8 | f64 | height — **default 10.0**, overwritten from host `+0xA8` (cell_height 26.0) | ctor + `FUN_180127310` |
| +0xC0 | string (SSO) | sound-effect name (e.g. `"se_common_cancel_b"`) | `FUN_180127310` |
| +0xD0 | string (SSO) | button name (ctor inits empty, cap 15) | ctor |
| +0xD8 | tr1::function | **enable predicate** — vtable method `FUN_18004f9f0` returns `pred()`, or 1 when unset | disasm |
| +0xF8 | tr1::function | **action callback** — vtable method `FUN_18004fc10` invokes it | disasm |
| +0x108 | tr1::function | `void(bool)` press-state callback (lambda89 wired here) | `FUN_180127310` |
| +0x140 | tr1::function | **InputEvent handler** — `onInputEvent` (`FUN_18004fa10`) captures `ev+0x20/+0x24/+0x28` into a closure and registers it at response slot 0 | disasm |
| +0x08.. | hash set | FNV-1a (basis `0x811c9dc5`, prime `0x1000193`) hashes of mode names (`"Reset"`, `"System"`) the button participates in | `FUN_180127310` |

The FilterButton wiring function `FUN_180127310` (called per filter entry)
builds one VirtualButton per FilterButton: cancel-SE, an activation lambda
carrying the FilterButton pointer + index, an `InputEvent` lambda, and the
button's **cell width/height**. `VirtualSelectBox` is the grid/list container
analog.

**Why "smoking gun":** a *logical button* object that carries spatial extent,
an activation callback, an enable predicate, and a per-button sound effect is
exactly the abstraction a touch router needs. Whether the game itself ever
consumes the width/height spatially on MDX is **unproven** (it may exist purely
for the touch builds of this framework — which would make it even better: the
dimensions are there *for us*). Firing a VirtualButton's `+0xF8` action
callback from mod code = activating that UI element through the game's own
handler, with its own SE and state transitions.

**Caveats:** VirtualButton is only *proven* in the `selectmusic` namespace (and
the RTTI shows `entry` namespace lambdas over `Component*` too). Coverage of
other screens is an open question (§12). Positions: the wiring copies only
width/height; the button's screen position presumably lives in the host
Component/FilterButton (`+0x88/+0x90` base_x/base_y per
`docs/filter_scroll_research.md`) — a touch router would hit-test against the
host component's rect, then fire the attached VirtualButton.


---

## 4. Layer 3: the AFP runtime's mouse/hit-test pipeline (dormant, intact) <a name="4-afp-mouse"></a>

libafp is a Flash player (AFP 2.13.7, `afp/libcore/afp-play.c` per debug
strings), and it retains the **entire Flash mouse interaction model** — exports,
per-layer mouse state, recursive display-list hit-testing, AS2 event dispatch,
and drag-and-drop. DDR never feeds it.

### 4.1 Mouse input API (exported, unfed)

| Export | Ordinal | libafp addr | Behavior (disassembly-verified) |
|---|---|---|---|
| `afp_layer_set_mouse_status` | 12 | `0x18001bfe0` | `(layer_id, x, y, buttons)`. Writes shorts to `layer+0x4A/+0x4C/+0x4E`, sets dirty bit `0x40` in `layer+0x08`. Magic id `0x78000000` = "system mouse": writes globals `0x180244ff8..` + press/release counters (`FUN_1801207f0`) |
| `afp_layer_set_mouse_wheel` | 13 | `0x18001c0b0` | `(layer_id, delta)`. Accumulates into `layer+0x50`, same dirty bit |

**gamemdx imports neither ordinal** (import table reviewed) — the pipeline is
never driven. The "system mouse" globals are write-only in this build (no
readers), but the **per-layer** path has a live consumer:

### 4.2 The hit-test machinery

- `FUN_18001bf10` (libafp) — "topmost display object under the layer's mouse":
  reads `layer+0x4A/+0x4C`, walks the display list from `*(layer+0x1A0)`
  back-to-front (last sibling first via `+0x68`/`+0x60` links = top-down
  z-order) calling the recursive hit-tester per child. 7 callers, all AS2
  built-in implementations (verified: `_droptarget` / drag-and-drop at
  `FUN_1800f67e0`, which builds the slash-path name of the hit object).
- `FUN_1800f06c0` (libafp) — the **recursive point hit-test**:
  `(node, x, y, exclude, flags, out_list) -> hit_node`. Concatenates the local
  matrix chain (`FUN_1800f2330`/`FUN_180013410`), dispatches per character
  type (tag-group switch: nested sprites recurse; shapes/text/bitmaps get
  geometric point tests via `FUN_1800a0b20`, `FUN_180155940`, `FUN_1801a3b50`,
  `FUN_18011a300`), respects visibility (`node+0x30 & 4`) and hit-disable
  flags (`0x20000`), handles mask layers (`+0x8C` clip-depth logic), and its
  error path logs `"unknown tag(...) to %s.", "mouse check 3"`. This is
  `MovieClip.hitTest` infrastructure — the strings `hitTest`, `hitTestPoint`,
  `hitTestObject`, `getBounds`, `localToGlobal` all exist in the builtin
  tables.
- The full AS2 event vocabulary is present as a contiguous name table at
  `0x1801f3ee0..`: `onKeyDown/Up`, `onMouseDown/Up/Move/Wheel`, `onRollOver`,
  `onRollOut`, `onPress`, `onRelease`, `onReleaseOutside`, `onDragOver`,
  `onDragOut`, plus `Button.on` and `onClipEvent` dispatch machinery.

### 4.3 What this layer is (and isn't) good for

**It will not make the game react by itself.** Even if we feed
`afp_layer_set_mouse_status` per frame, mouse events dispatch into *authored
content handlers* (`onPress` on clips / Button characters) — and DDR's UI
movies are driven procedurally by gamemdx C++ (SetFrame/position writes); it
is near-certain the authored AFPs contain no mouse handlers (unverified but
consistent with every AFP inspected in prior research; §12).

**It is extremely valuable for spatial mapping.** The hit-tester answers
"which named MovieClip is under screen point (x,y)" with correct matrix math,
z-order, visibility, and masking — the exact query a touch router needs, with
zero geometry re-implementation. Two consumption routes:

1. Call `FUN_1800f06c0` directly (needs a new libafp AOB — it is not
   exported; entry helper `FUN_18001bf10` additionally needs the layer's
   mouse fields pre-written, which `afp_layer_set_mouse_status` does).
2. Reuse its building blocks that ARE exported: `afp_mc_traversal` (tree
   walk), `afp_layer_get_matrix`/`afp_layer_get_info`, `afp_mc_get_param`
   (position/size params) — i.e., walk and transform manually. More work,
   more supported.

Coordinate space: the x/y passed are in the layer's stage space (shorts). The
concrete mapping from 1280×720 screen space through the layer's own placement
transform must be calibrated live (§12). Thread rule: all libafp calls from
the game thread only (established project rule).

---

## 5. The spatial mapping problem (what is on screen, where) <a name="5-spatial-mapping"></a>

Four viable sources of touch-target geometry, in increasing order of fidelity
and cost:

| # | Source | Fidelity | Cost | Where it fits |
|---|---|---|---|---|
| S1 | **Static layout tables** per screen (hand-measured 1280×720 rects, like the SMX overlay's model) | Low — breaks when panels move/animate; blind to dynamic content | Very low | Simple static screens (mode select, caution, results advance) |
| S2 | **Live component-tree reads** — walk game objects (FilterButton `+0x88/+0x90/+0xA0/+0xA8`, option rows, wheel card slots) for authoritative rects | High for the screens we've already REd | Medium; per-screen struct knowledge (much already exists: filter grid, option rows, wheel cards) | Song select panels, options screen |
| S3 | **AFP hit-test** (§4.2) — ask the scene graph directly, get the named MC under the point | Highest (handles animation, z, masks) | New libafp AOBs + coordinate calibration + name→action mapping table per screen | Any screen; the long-term backbone |
| S4 | **VirtualButton enumeration** — walk the Component tree, hit-test against host rects, fire the attached VirtualButton | Highest *semantic* fidelity (the game tells us what is pressable) | Requires the Component-tree root anchor + per-screen coverage survey | Wherever VirtualButtons exist (selectmusic proven) |

A practical system combines S1 for dumb screens, S2/S4 for the song wheel and
panels, and grows S3 as the general mechanism.


---

## 6. Candidate architectures <a name="6-architectures"></a>

### A. Overlay-of-native-buttons (current stopgap — NOT the goal)

Mod draws its own buttons, hit-tests its own layout, injects digest/pinpad on
tap. Zero spatial coupling to the game. This is `smx-hardware` today. Listed for
contrast: it is the thing you explicitly do not want.

### B. Touch → synthesized directional navigation ("steer the cursor")

Touch a target → compute how many LEFT/RIGHT/UP/DOWN presses move the game's
existing cursor from current selection to the touched one → inject that many
edge-timed presses through the proven digest path. The game animates its own
focus, so feedback is free.

- **Pros:** builds entirely on cabinet-proven injection; no new game-state
  writes; safe (identical to a human mashing buttons); works on any
  cursor-based screen.
- **Cons:** requires knowing *current* selection index and *target* index per
  screen (S2 reads); visibly "scrolls" rather than jumping; poor for the wheel
  (hundreds of songs) and for absolute gestures (drag). Fine for menus with a
  handful of items.

### C. Touch → semantic actuation ("fire the element's handler")

Hit-test to a UI element (S2/S3/S4), then invoke the game's own action for it:
fire a VirtualButton `+0xF8` callback, call the panel's decide/confirm entry, or
write the selection model directly and let the game re-render.

- **Pros:** instant, native feedback, correct SE; the true "touch-native" feel;
  supports absolute jumps (tap a distant song).
- **Cons:** per-screen bespoke — each screen needs its element inventory,
  its action entry points, and its state model REd; highest risk of touching
  half-initialized state (learnings: gate on live "ready" latches, never
  write from init).

### D. Native touch device (feed ark `core_io+0x88`)

Install a mod-owned touch device so the ark framework consumes touch natively.

- **Pros:** most "native" for the operator/test menu; potentially unlocks the
  ark test-menu touch nav for free.
- **Cons:** no evidence gamemdx game screens consume ark touch (§2.4); likely
  limited to operator menu; product-config gating unknown. **Not recommended
  as the primary path**; worth a single spike for the operator menu only.

### Recommended: **B for simple screens, C for interactive screens, S3 (AFP
hit-test) as the shared spatial backbone.** A `touch_router` service owns
capture (reuse `smx-hardware`'s WndProc), scene-aware dispatch (via
`scene_manager`), and a per-screen "touch controller" registry. Each controller
declares its targets (S1/S2/S3/S4) and its actuation (B or C). New screens are
added incrementally without touching existing ones.

---

## 7. Per-screen scope inventory <a name="7-scope"></a>

Effort sizing: **S** = static rects + nav injection (days); **M** = live struct
reads + nav/semantic actuation (1–2 weeks); **L** = bespoke model manipulation
+ gesture handling (multi-week). Scene IDs per `docs/ddr_world_scene_ids.md`
(0-indexed).

| Scene | Screen | Interactions wanted | Source | Actuation | Size | Notes |
|---|---|---|---|---|---|---|
| 14 | Title ("press start") | tap anywhere = start | S1 | B (Start) | S | trivial; one target |
| 41 | Language select | tap a language | S1/S2 | B | S | few fixed items |
| 20 | Mode select (1P/2P) | tap a mode | S1 | B | S | 2–3 targets |
| 21 | Caution | tap = advance | S1 | B (Start) | S | one target |
| 25 | **Song select (wheel)** | drag wheel, tap jacket, tap difficulty, open panels | S2/S4 | C + gesture | **L** | the centerpiece; §7.1 |
| 25 | Filter/Sort/Folder panels | tap filter chip, tap sort, tap folder | S2/S4 | C (VirtualButton) | M | FilterButton grid already REd (`filter_scroll_research.md`); VirtualButtons proven here |
| 25 | Options panel (in-song-select) | tap row, tap value ± | S2 | C or B | M | option rows REd (`option_*` docs); scalar/enum rows |
| 27 | Stage indicator | tap = advance | S1 | B | S | |
| 28 | Gameplay | (out of scope — stepping is physical) | — | — | — | do not touch |
| 29 | Stage pass/fail | tap = advance | S1 | B | S | |
| 30 | Results detail | tap = advance / tap tabs | S1/S2 | B | S–M | |
| 32 | Final results | tap = advance | S1 | B | S | |
| 35 | Thank you | tap = advance | S1 | B | S | |
| 1 | Operator/test menu | touch nav | D or S1 | D or B | M | candidate for the ark-touch spike |

### 7.1 Song select — the hard screen (Scene 25)

The wheel is the defining touch interaction and the one that most needs C + a
bespoke gesture model. Known anchors from prior research:

- `SelectMusicModel` global; highlighted song is a `weak_ptr` at
  `*(selectmusic_model)+0x1B0` (per AGENTS.md / `music_wheel_song_length`); the
  song code lives inline at `music::Info+0xD`.
- `MusicPanel` (`.?AVMusicPanel@selectmusic@sequence@@` @ RTTI `0x1804a10e8`)
  drives the card layout; `music_scroll_setup` = `gamemdx+0x18F1E0`;
  `ScrollBar` = `sequence::selectmusic::ScrollBar` (vtable `+0x37AE50`);
  scroll child `music_card_scroll_root` / `scroll_usr`.
- Wheel movement responds to MENU LEFT/RIGHT `InputEvent`s → a scroll
  target/easing model (exponential easing pattern seen in `choice_scroll`).

Three touch behaviors, escalating difficulty:

1. **Tap a visible jacket → select it.** Hit-test the card slots (S2: card
   rects from the panel), then either (B) inject N left/right to walk the
   cursor to that card, or (C) write the wheel's scroll-target + highlighted
   index and let the ease/render run. B is safer and probably feels fine for
   ±a-few cards.
2. **Tap difficulty / open panel → fire it.** DifficultyPanel / FilterPanel /
   SortPanel VirtualButtons (C). Highest-value, most tractable "native" wins.
3. **Drag to scroll the wheel.** Bespoke: map touch delta → scroll-target
   delta, feed the model's scroll accumulator per frame, respect the settle
   snap. This is the one genuinely new continuous-gesture controller and the
   single largest sub-task. Momentum/flick is a further increment.

Direct model manipulation (1c/3) risks the diff-driven-display and
half-initialized-state traps in `learnings.md`; prefer driving the game's own
scroll-target input over writing final positions.


---

## 8. Recommended strategy & phasing <a name="8-strategy"></a>

A `touch-native` mod (distinct from `smx-hardware`, though sharing its capture
layer) built in dependency order:

**Phase 0 — shared infrastructure.**
- Factor the `smx-hardware` WndProc/contact-tracking into a reusable
  `services/touch_input` (per-contact down/move/up in 1280×720 model space,
  CrossOver mouse fallback, IR debounce — all already written).
- Build `services/touch_router`: scene-aware (subscribes to `scene_manager`),
  owns a registry of per-scene "touch controllers", dispatches contacts to the
  active controller, panic-contained on the frame callback.
- Decide the spatial backbone: implement the S3 AFP hit-test service
  (`bm2d_api::hit_test(layer_id, x, y) -> hit MC name/id`) — either new AOBs
  for `FUN_18001bf10`/`FUN_1800f06c0` or a manual walk over
  `afp_mc_traversal` + matrix reads. Calibrate the screen→layer coordinate
  transform live on one known screen.

**Phase 1 — prove the model on the easy screens (B).**
- Title, caution, stage indicator, all results screens, mode select, language
  select. Static rect tables + directional/Start injection. Ships value fast,
  exercises the router, low risk. This alone makes ~8 screens tappable.

**Phase 2 — song-select panels (C, VirtualButton).**
- FilterPanel / SortPanel / FolderPanel / DifficultyPanel: hit-test host
  component rects, fire the attached VirtualButton action. Requires the
  Component-tree root anchor + a name→button survey. Highest "wow" per unit
  effort — direct taps on filter chips and difficulties.

**Phase 3 — the wheel (L).**
- Tap-to-select visible jackets (start with B-style cursor-walk), then
  drag-to-scroll against the scroll model, then flick/momentum. This is the
  make-or-break UX and deserves its own planning cycle.

**Phase 4 — options + long tail.**
- In-select options rows, results tabs, operator-menu spike (D).

**Cross-cutting requirements:**
- **Player-side attribution.** A cabinet-wide touch must resolve to P1 or P2
  (or "the entered side"). Reuse the `stage_records::side_entered` logic the
  SMX/announcer mods already use.
- **Input arbitration.** Respect `InputLock` and the InputEvent response-map so
  touches obey the same focus/modality rules as buttons (don't select a song
  behind an open panel).
- **Score safety.** Navigation-only; no gameplay input synthesis → low network
  risk. Still route through the existing `score_guard` awareness if any
  synthesized input could reach a scored context.
- **Versus / doubles.** Every controller must handle 1P-only, 2P, and doubles
  seat layouts, or explicitly no-op outside its supported mode.

**Realistic total scope:** the whole-game vision is **multi-month** (15+ screens,
each its own controller, plus the wheel gesture work and the AFP hit-test
backbone). But it is **incrementally shippable** — Phase 1 delivers most of the
game's linear flow tappable in days-to-weeks, and each later phase stands alone.
There is no architectural cliff that forces "all or nothing."

---

## 9. Key addresses <a name="9-addresses"></a>

All addresses file-relative to `0x180000000`. **Single-build unless noted;
re-derive via AOB before use (§10).**

### gamemdx 20260721

| Symbol | Addr | Description |
|---|---|---|
| `InputEventDispatcher::vftable` | `0x18035fdc8` / `0x18035fe10` | primary + MI-base |
| `InputEventDispatcher` TypeDescriptor | `0x180482120` | RTTI |
| InputEventDispatcher ctor | `0x18004c440` | repeat cfg 500/200 ms |
| InputEventDispatcher poll/update | `0x18004c810` | synthesizes InputEvents |
| dispatch entry (vtbl +0x20 impl) | (via vtable) | `(disp+0x20)(disp, &event)` |
| `VirtualButton::vftable` | `0x18035fee0` / `0x18035fe98` | primary + `+0x28` MI base |
| VirtualButton ctor | `0x18004f860` | size 0x148 |
| VirtualButton enable-pred (vtbl) | `0x18004f9f0` | reads `+0xD8` |
| VirtualButton action (vtbl) | `0x18004fc10` | invokes `+0xF8` |
| VirtualButton onInputEvent | `0x18004fa10` | captures `ev+0x20/24/28`, response slot 0 |
| FilterButton→VirtualButton wiring | `0x180127310` | per-entry builder |
| test-menu hint rewriter | `0x180006b10` | TOUCH→BUTTON string table |
| `MusicPanel` RTTI | `0x1804a10e8` | wheel card layout |
| `music_scroll_setup` | `0x18018F1E0` | wheel scroll init |
| `ScrollBar` vtable | `0x18037AE50` | selectmusic scrollbar |

### libafp 2.13.7

| Symbol | Addr | Description |
|---|---|---|
| `afp_layer_set_mouse_status` | `0x18001bfe0` | ord 12; feed x/y/buttons |
| `afp_layer_set_mouse_wheel` | `0x18001c0b0` | ord 13 |
| topmost-under-mouse walker | `0x18001bf10` | reads layer `+0x4A/4C`, walks display list |
| recursive point hit-test | `0x1800f06c0` | `(node,x,y,excl,flags,out)`; the core |
| `_droptarget` builder (example caller) | `0x1800f67e0` | builds hit-object path |
| AS2 event name table | `0x1801f3ee0` | onPress/onRelease/onMouse*/… |
| `afp_mc_traversal` | `0x180038b10` | tree walk (child/sib/parent modes) |

### arkmdxbio2 20260721

| Symbol | Addr | Description |
|---|---|---|
| `DefaultArkCoreIO::touch` | `0x180009d20` | core-IO vtbl `+0x260`; returns `io+0x88` (null on MDX) |
| TOUCHPANEL CHECK screen | `0x1800685b0` | touch-vtbl `+0x18` get-pos, `+0x28` set-calib |
| touch device slot | `core_io+0x88` | `pSubIOTouch_`, never written on MDX |

### Per-layer AFP display-object fields (from hit-test disassembly)

| Offset | Field |
|---|---|
| layer `+0x08` | flags; bit `0x40` = mouse-dirty |
| layer `+0x4A/+0x4C/+0x4E` | mouse x / y / buttons (shorts) |
| layer `+0x50` | mouse wheel accumulator |
| layer `+0x1A0` | display-list root (for hit walk) |
| node `+0x30` | flags: `0x4` hidden, `0x20000` hit-disabled |
| node `+0x58` | children; `+0x60`/`+0x68` sibling links (z-order) |
| node `+0x70`/`+0x78` | name ptr / name hash |
| node `+0x8C` | mask clip-depth |


---

## 10. Cross-version notes <a name="10-cross-version"></a>

**No cross-version verification was performed for any address in this doc.**
Every gamemdx address is 20260721-only; libafp is 2.13.7; arkmdxbio2 is
20260721. Before any implementation:

- Derive `VirtualButton::vftable`, the InputEventDispatcher, and the hint
  rewriter via **structural AOBs / RTTI-string scans**, not the raw addresses
  here — vtable and function addresses shift every build.
- The **RTTI class names are the stable anchors** (`.?AVVirtualButton@...`,
  `.?AVInputEventDispatcher@sequence@@`, `.?AVMusicPanel@...`). Resolve
  vtables via COL back-reference from the TypeDescriptor string, as the
  existing donor-vtable code already does.
- libafp exports (`afp_layer_set_mouse_status`, `afp_mc_traversal`, …) resolve
  **by name** and are stable across builds — prefer them over the internal
  `FUN_1800f06c0`/`FUN_18001bf10` (which need fresh AOBs per libafp version).
- The ark touch subsystem is framework code shared across many Konami titles;
  its shape is likely stable, but the `+0x88` offset and `+0x260` vtable slot
  are single-build findings.
- Struct offsets (VirtualButton `+0xD8`/`+0xF8`/`+0xA0`, InputEvent `+0x10`/
  `+0x18`) must be re-confirmed from ctor/wiring disassembly on each target
  build — they are the load-bearing claims.

Per the project rule: verify on **both** currently-supported builds before
declaring anything version-agnostic, and document both resolved addresses.

---

## 11. Gotchas <a name="11-gotchas"></a>

- **The AFP mouse pipeline will not actuate anything by itself.** Feeding
  `afp_layer_set_mouse_status` dispatches to *authored* `onPress`/`onRelease`
  handlers, which DDR's procedurally-driven UI movies almost certainly lack.
  Use the pipeline for *hit-testing only* (query which MC is under a point);
  do actuation through gamemdx (VirtualButton / cursor injection).
- **Rasterization time ≠ display z / display time** (`learnings.md`). Any
  touch-feedback overlay drawn by the mod inherits the overlay-draw z rules
  already learned the hard way — reuse the identity-gated anchor pattern.
- **Diff-driven display actors** (`learnings.md`): writing a final scroll
  position / selection index directly can freeze diff-gated re-renders. Drive
  the game's *input/target* (scroll-target, cursor step) and let the engine
  transition itself.
- **Never actuate from init or a background thread.** VirtualButton callbacks,
  libafp calls, and model writes are all game-thread-only and depend on
  warmed-up state; gate on a hook-fired ready latch (`learnings.md`).
- **InputEvent field semantics `+0x20/+0x24/+0x28` are unproven.** The
  temptation to read them as touch x/y/flag is strong (VirtualButton's handler
  captures them) but unverified — could be code/player/handled. Do not build
  coordinate logic on them without live confirmation.
- **VirtualButton spatial consumption on MDX is unproven.** The width/height
  may be layout-only (or purely for the touch build's own hit-test). Confirm
  whether MDX ever reads them before assuming they define a hit rect; if not,
  hit-test against the *host component's* rect (`FilterButton+0x88/+0x90` +
  `+0xA0/+0xA8`) instead.
- **Panic discipline across FFI.** The touch frame callback and any hit-test
  path are `extern "C"`-reachable; wrap in `catch_unwind`, no `.unwrap()`
  (the file-hooks abort saga in `learnings.md`).
- **Response-map / InputLock arbitration.** Firing a VirtualButton or injecting
  an event out of turn can bypass modal panels. Respect `InputLock` and the
  focused-component response slots, or touches will "reach through" open
  overlays.
- **Player-side ambiguity.** One physical touchscreen, two logical players.
  Every actuation must pick a side deterministically (entered-side rule).
- **The ark test menu strings are conditionally rewritten**, not removed —
  don't assume "TOUCH …" strings in the binary mean touch is active; on MDX
  they are always converted to BUTTON hints at render.

---

## 12. Open questions / future RE tasks <a name="12-open-questions"></a>

Ordered by how load-bearing they are for a real implementation:

1. **VirtualButton coverage survey.** Is the Component/VirtualButton framework
   used on screens beyond `selectmusic` (mode select? entry? results?), or are
   those screens plain actor-polls-buttons? Determines how far strategy C
   reaches vs. falling back to B. (RTTI shows `entry::` `Component*` lambdas —
   promising but unconfirmed.)
2. **AFP hit-test coordinate calibration.** Concrete transform from 1280×720
   screen space into a given layer's stage coordinates for
   `afp_layer_set_mouse_status` / `FUN_1800f06c0`. Needs a live capture on one
   known screen (write a probe point, read back which MC hit-tests).
3. **Does MDX read VirtualButton `+0xA0/+0xA8` spatially?** Breakpoint the
   fields' readers in a live session. If yes → S4 is a clean hit-rect source;
   if no → hit-test host component rects.
4. **InputEvent struct layout** (`+0x20/+0x24/+0x28` semantics; full field
   map). Live-inspect an event during menu navigation.
5. **Component-tree root anchor** for the active scene — where to start a
   walk/hit-test enumeration. Likely reachable from the SelectMusicSequence /
   View object; find the child-list head and traversal entry.
6. **Wheel scroll model.** Exact scroll-target field(s) and settle/snap logic
   in `MusicPanel` / `ScrollBar` for drag-to-scroll (strategy 3 in §7.1).
   The `weak_ptr` at `selectmusic_model+0x1B0` gives the highlighted song;
   need the scroll accumulator + target for gesture input.
7. **ark-touch operator-menu spike (strategy D).** Install a stub device at
   `core_io+0x88` implementing the `+0x18` position getter; observe whether the
   ark test menu becomes touch-navigable or whether product config gates it.
8. **Do any authored UI AFPs contain `onPress`/button characters?** A quick
   scan of a few select-music/menu AFP streams for AS2 tag groups would confirm
   (near-certainly no) whether the libafp event path could ever self-actuate —
   closing out strategy variants that assume it can.

---

## Summary

Touch-native DDR is **feasible and not architecturally blocked**, because the
game sits on frameworks Konami built touch-first: an ark touch subsystem (device
absent), a gamemdx Component/VirtualButton framework with real per-element
callbacks, and a complete-but-unfed AFP mouse/hit-test pipeline. Capture and
button/pinpad actuation are already cabinet-proven from `smx-hardware`. The new
work is **spatial mapping** (best served by the AFP hit-tester as a shared
backbone, plus live struct reads) and **semantic actuation** (firing
VirtualButton callbacks / driving the wheel's scroll model). The full vision is
genuinely **per-screen bespoke and multi-month** for the whole game — your
expectation is correct — but it is cleanly incremental: a scene-aware
`touch_router` + per-screen controllers lets Phase 1 (the linear
title→results flow) ship in days-to-weeks on static rects + navigation
injection, with the song-wheel gesture work isolated as its own later effort.
The load-bearing claims here are single-build, disassembly-only — §12 lists the
live-verification tasks that must precede code.





