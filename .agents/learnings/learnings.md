# Project Learnings

Hard-won, project-specific engineering facts captured from prior sessions.
These are the subtle traps that compile fine and often survive the first
crash-prone code path, then detonate later — read the relevant entry before
working in the same area.

For the universal safety rules (panics across FFI, allocator matching, one
detour per target, render-thread discipline), see `CLAUDE.md`. For deeper
reference on allocators/hooking and RE address conventions, see
`.agents/steering/rust-hooking.md` and `.agents/steering/reverse-engineering.md`.

---

## LayeredFS textures

### Name-referenced MovieClip textures need donor-clone injection, not auto-inject

**Context:** Migrating a mod off the old `asset_loader::register_arc` path to
LayeredFS so custom textures load from `data_mods/` instead of a packed ARC. The
textures are filter labels (`sefi_version_{key}`) that the game's `filter_item`
BM2D MovieClip applies *by name*.

There are two distinct net-new-texture paths in LayeredFS, and they are NOT
interchangeable:

1. **Auto-inject** (`ifs_textures::inject_new_textures`, triggered by dropping a
   bare PNG into an IFS mod folder whose name isn't in the stock texturelist):
   builds each PNG as its own 1:1 atlas with full-coverage UV (0,0)–(1,1). Correct
   for a sprite you draw yourself via an image widget, where you control the UVs.
2. **Donor-clone** (`atlas_cloner::generate_cloned_atlases` + a
   `texturelist.merged.xml`): composites the PNG at a *donor texture's* pixel
   rect inside a cloned atlas, preserving the donor's UV slot.

If the game (a MovieClip/AFP template) looks the texture up by name and expects it
at a specific atlas position, you MUST use the donor-clone path. The auto-inject
path will "succeed" — the log even says `injected N new textures` — but the label
renders wrong/invisible because its UVs are full-coverage instead of the slot the
template expects. This cost a wasted cabinet deploy: the fix is to mirror what
`custom_options` and `folder_expansion` already do for the same IFS (clone a
plausible donor slot, e.g. `sefi_version_world` for version labels).

### Multiple mods injecting into one IFS must use unique atlas prefixes

**Context:** `custom_options`, `folder_expansion`, and `series_expansion` all
inject cloned atlases into `select_music_option_v3.ifs`.

`merge_xmls` combines *all* mods' `texturelist.merged.xml` files for an IFS (it
iterates `find_all_modfile`), and each mod's `<image>` entries are keyed by
distinct texture names, so the texturelist side coexists fine. The collision risk
is the atlas *blob*: `atlas_cloner` writes the cache file at
`{cache_root}/{ifs_mod_path}/md5(custom_atlas_prefix + "_NNN")`, and `cache_root`
+ `ifs_mod_path` are identical across mods targeting the same IFS. The
`custom_atlas_prefix` is therefore the entire collision key — reusing another
mod's prefix overwrites its atlas blob (last `enable()` wins) and makes its
textures vanish. Keep prefixes unique per mod: `copt_mods` (custom_options),
`cfolder_*` (folder_expansion), `cser_version` (series_expansion).

### LayeredFS is reactive — it only acts on files the game itself opens

A texture name becomes BM2D-resolvable via LayeredFS only by injecting into an IFS
the game opens on its own; LayeredFS never originates an open. There is no runtime
IFS/kbin *writer* in this codebase (only a decoder/reader), so a truly
free-floating "phantom" container can't be synthesized — net-new textures must
ride a host IFS the game loads, or be drawn by an image widget whose UVs you set
yourself. This is why the old `register_arc` (active `arc_load` of a real on-disk
container) was the only way to register textures with no host IFS, and why
removing it means demo-only textures with no host get dropped rather than ported.

---

## Rendering

### CommandList state leaks into downstream renderers

**Context:** Hooking a per-frame render function that emits CommandList commands
(SetShader, SetTexture, SetAlphaBlendMode) and running additional draw work
after the original returns.

The CommandList is a sequential command buffer shared across all renderers in
the scene. Whatever state the original function left bound (shader, texture
slots, blend mode) persists into the next renderer's commands unless that
renderer explicitly rebinds. When appending draw work after the original, save
the bindings vanilla would have left (shader + texture slot 0 + blend mode),
make your state changes, draw, then restore those bindings before returning.
Otherwise downstream renderers (spot renderer, judge effects, etc.) inherit your
state and render with the wrong shader/texture. Restoring only blend mode is
insufficient — shader and texture bindings leak too. (See `mine_render.rs` for
the canonical restore-at-pass-end pattern.)

### BM2D child lookup: layer id (type-1) vs MC id (type-4) are different namespaces

**Context:** Driving an AFP MovieClip sub-child by name from a render override —
e.g. resolving `choice_usr/scroll_usr` under an option row's sub-MC to animate
the value-position marker.

libafp exposes two child-resolvers that look interchangeable but accept
**different id namespaces**: `afp_layer_mc_refer` (`bm2d_api::layer_find_child`)
takes a **type-1 layer id**, while `afp_mc_refer` (`bm2d_api::find_child`) takes a
**type-4 MC id**. The `mc_id` read from a sub-MC at `*(sub_mc+0x08)` is a type-1
layer id, so it must be resolved with `layer_find_child`. Calling `find_child`
with it returns **-1 (not found)** and the dependent op silently no-ops — no
crash, no log, just a missing visual. The first marker implementation used
`find_child` and produced exactly that "nothing renders, no error" symptom;
swapping to `layer_find_child` (which the working `option_usr`/`choice_usr`
texture binds in the same function already used) fixed it. Diagnosed live by
breakpointing both `afp_mc_refer` and `afp_layer_mc_refer` and decoding the
looked-up name from the captured register holding the SSO string fragment —
`afp_mc_refer` for our path returned RAX=-1, the layer variant resolved. Rule of
thumb: when an existing binding in the same code path uses one resolver, match
it; don't assume the two `*_refer` calls are equivalent. The native code is the
tiebreaker — it resolved these marker sub-clips via the layer-id form.

Related: pinning an AFP sub-clip's position every frame (the native per-frame
`afp_mc_op(mc, 0x0f04, …)` write) is also what *stops* it free-running its intro
timeline animation. A mod render override that skips that write lets the clip
play its sweep on every (re)instantiation; reinstating the per-frame position
write fixes the marker AND the unwanted animation in one change. (See
`docs/option_row_marker_render.md` and `custom_options/rows.rs::drive_value_marker`.)

---

## Hooking & engine reuse

### Reusing a judgment path for a synthesized note type

**Context:** Reusing the engine's judgment path for a new note type that
synthesizes per-panel notes (e.g. mines), where the engine's original note type
spans multiple panels at once (e.g. shock arrows).

Downstream listeners of the judgment's dispatched message may gate visual/audio
effects on fields of the note record that implicitly assume the original note
type's panel pattern. The engine's shock-effect listener is the canonical
example: its lane-flash animation fires only if `note->state[first_panel_of_side]`
equals `TRG`, because a real shock arrow sets all four per-side panels to `TRG` —
but a single-panel mine only sets one panel, so the lane flash only fires when
the mine happens to sit on the leftmost panel.

**Fix pattern:** at the dispatch call site, point the message's note pointer at a
stack-allocated *synthetic* note whose fields satisfy the listener's implicit
preconditions (here: all four per-side `state` slots set to `TRG`), copying
through only the fields other listeners actually read (here: `beat_count` for
talent measurement). The real note is preserved intact for
rendering/judgment/cache. Safe because the engine's message dispatch is
synchronous — the synthetic lives the entire dispatch window. When adopting this
pattern, audit every listener of the message to confirm which fields are read
from the note pointer, so the synthetic mirrors exactly what's needed and no more.

### Don't call native game functions from the DLL init thread

**Context:** Calling a native C++ constructor (or any function that transitively
touches game globals) from the hook DLL's init thread to "dry-run" a production
code path for validation.

Don't. The DLL's init thread runs very early in the game's boot — well before the
game has finished populating the runtime state a typical C++ member function
expects (reactive-stream registries, app-heap handle VALUE not just address,
resource-manager singletons, per-subsystem Lazy globals, etc.). Even if the
function's address is resolved correctly and the allocator is matched, the
parent-class ctors invoked inside it will dereference uninitialized globals and
crash with EXCEPTION_ACCESS_VIOLATION inside gamemdx. This happened with
`FUN_180173810` (ArrowColor ctor) at DLL-init time even though the signature
resolved, the donor vtable derived, `game_malloc` was wired, and every Rust-side
invariant held.

For init-time validation of a code path that will later run from a production
game-thread hook, do a READ-ONLY preview — read the relevant `.rdata` (vtables,
string literals) and log what the synthesized state WOULD be, but do not call any
game function that performs meaningful work. Save live execution for a context
where the game itself is about to do the same thing (e.g. inside a builder-hook
detour). Rule of thumb: anything deeper than pure memory reads needs a warm game
to run safely.

**Corollary — even a trivial setter crashes if its backing global isn't built
yet; gate live writes on a hook-fired "ready" latch.** The `timing-offsets` mod
crashed (EXCEPTION_ACCESS_VIOLATION) calling the game's config-map int setter
from its `enable()` (DLL-init thread) to push configured values. The setter
address resolved fine, but the config-map global (`DAT_1806ebcf0`) is created by
the game's *own* sound/input init, which runs AFTER the DLL init thread — so the
setter walked a null map. (Tell-tale: the mod's own hook-substitution log lines
were timestamped ~1s LATER than the init crash.) It's not just ctors — ANY game
function transitively touching a not-yet-initialized global is unsafe at init
time. Clean fix: the mod hooks the setter anyway (for substitution), so the first
time that hook FIRES is proof the map now exists; latch an atomic `MAP_READY` in
the hook and gate every live setter call on it. At boot the init-time push
no-ops and the game's own first publish (flowing through the hook) does the
seeding; runtime pushes (menu changes, re-enable) proceed normally. General
pattern: when you must call a game fn that depends on a lazily-created global,
drive it from (or gate it on) the same hook that proves the global is live —
never from raw init.

---

## Donor-vtable synthesis (custom MSVC C++ objects)

### The RTTI Complete Object Locator lives at vtable[-1]

**Context:** Synthesizing a custom MSVC C++ vtable by copying function-pointer
slots from a donor class's vtable (the "donor-vtable reuse" trick used for
injecting mod-authored OptionElement rows) so the game's scene graph can walk a
mod-owned object polymorphically.

MSVC C++ vtables carry a pointer to the class's `RTTI_Complete_Object_Locator` at
the slot IMMEDIATELY BEFORE slot 0 (i.e. at `vtable[-1]`). `__RTDynamicCast` —
used by game code whenever it checks whether an object implements a secondary
interface — reads through that negative offset; omitting it makes the cast walk
uninitialized memory and raise a C++ exception (magic `0xE06D7363` = `'msc'`). A
mod vtable assembled via `VirtualAlloc`-backed storage must include the donor's
`[-1]` slot: allocate `N+1` qwords (N = virtual slots needed), put the donor's
`*(donor_vtable - 1)` COL pointer at the head, and return a pointer offset by one
slot so callers' `vtable[i]` indexing maps to the compiler's slot numbering.
Symptom when forgotten: allocation succeeds, donor ctor runs, vtable swap lands —
then a crash where the game does perfectly reasonable polymorphic dispatch, logged
as `agcs: Access violation - no RTTI data!` followed by a caught `0xE06D7363`.
Easy to miss because Ghidra shows vtables starting at slot 0 and the COL pointer
at `[-1]` separately.

### `OptionElement` row+0x110 (value-model self-ptr) gates multiple native features

**Context:** Injected custom-option rows (donor-vtable clones) that skip the
native builder's reactive wiring. The builder populates `row+0x110` with a
self-pointer to an embedded `+0xF8` value-model/lambda subobject; our rows leave
it null (the ctor zeroes it, the builder fills it, we skip the builder).

`row+0x110 == 0` is a recurring discriminator the native code uses to decide "is
this row fully wired": at least two distinct option-screen features short-circuit
on it. (1) The slot-7 **value-position marker** render (`option_row_marker_render.md`)
aborts past `+0x110` — we replicate the marker block, which sits before the check.
(2) The **preview-image-box** name getter (`IOptionElement` slot 0,
`option_preview_image_box.md`) does `if (row+0x110==0) return ""` — so our rows
render a blank preview until we override the slot to write the name directly.
Pattern: when a native option feature "does nothing" on a mod row with no error,
suspect a `row+0x110`-gated path, and prefer replicating the post-gate behavior
(write outputs directly from the registry) over reconstructing the `+0xF8`
value-model closure (the per-instance-state trap below). Don't try to populate
`+0x110` itself — it points at a `BM2D::CLayer*`-capturing lambda the builder
constructs against live AFP state.

### Inherited donor slots can depend on unwired per-instance state

**Context:** Donor-vtable reuse where some donor virtual slots install
per-instance reactive state via kind-specific lambdas captured from a C++
anonymous namespace (e.g. an `onCreate` that wires `<lambda128>..<lambda133>`
keyed to an `OptionItem<KIND>` template specialization the mod's row deliberately
doesn't participate in).

Copying such slots verbatim compiles and runs through the first crash-prone path
(registration, RTTI walks), then detonates later when the render pipeline or
reactive streams fire against the unwired per-instance state. Symptoms:
empty-name AFP lookups (`afp_mc_load_bitmap no .`), null derefs inside
clip-animation code, or a caught access violation after a clean-looking
`builder done` log line. Rule of thumb: inspect every inherited slot for
dependencies on fields the donor ctor alone couldn't populate. If a slot reads
`this+<offset>` at a field the per-KIND subscriber wiring was supposed to fill
(e.g. `this+0x118` for the cached AFP sub-clip pointer), override that slot too —
with a catch-all no-op `unsafe extern "C" fn(_this, _a, _b) {}` if the mod row
genuinely doesn't need that method. "Invisible but pressable" is an acceptable
intermediate state; "crashes on first render tick" is not. Safe to inherit as-is:
destructor, initIndex (reads from shared OptionTab value-list), onReset, generic
onTick. Unsafe without an override: `onCreate` (reactive wiring), `render`
(animates per-clip state), and anything else that inspects kind-specific
lambda-populated fields.

---

## Reverse engineering

### Diagnostic build before rewriting on a "broken" test

**Context:** Debugging an apparent runtime read-through failure (code returns a
stale/default value even though the underlying field was updated elsewhere), and
the first in-game test round suggests the implementation is broken.

Before ripping the implementation apart, add one-shot WARN paths to every
fallback branch AND one INFO log dumping the full pointer chain + raw field
value. Ship that diagnostic build and have the user repeat the exact test. Two
outcomes: (a) the logs show the chain genuinely failing somewhere — you now know
exactly which link to investigate, not the whole chain; (b) the logs show the
chain resolving correctly, in which case the earlier "broken" test was likely a
state/timing artifact (player not yet committed to the chart, cache primed before
the relevant option was set, menu change not yet propagated) and the
implementation was correct all along. The second outcome is real here: the
arrow-shape follow-up redeployed with detailed walk logging produced a correct
`raw_shape=6` on the very first chart. Cost of the diagnostic build is ~5 minutes;
cost of blindly rewriting on a bad theory is hours.

### Trace the register back before trusting a "field at offset" claim

**Context:** Resolving a "struct field X lives at offset Y on object Z" claim
derived from decompilation of a function that writes the field (e.g.
"max_tab_count lives at OptionForm+0x08, written via MOV [RDX+8], 5").

Always trace the register feeding the write site BACK through the disassembly to
the function that produced it. In the row-builder case, RDX at the write site was
NOT `param_1` (OptionForm); it had just been reloaded from a stack slot populated
by an ordinary `shared_ptr` copy constructor pulling from `OptionElement +
0x1F8/0x200`. The field actually lives on a separate `TabMetadata` sub-object
reached via the shared_ptr, not on OptionForm. Mis-identifying the owner struct
means every downstream search for additional readers targets the wrong struct.
Mechanically: when the decompiler shows `field = 5` on a pointer you inferred to
be `this`, re-read the assembly for the most recent
`MOV <reg>, qword ptr [<something>]` preceding the write — that's what the pointer
actually is.

### Work out the MI-base `this` before publishing offset claims

**Context:** Reading decompiler output where the function takes `this = row +
some_offset` (an inner multiple-inheritance base `this`, as for virtual methods
dispatched through the secondary/third/fourth MI vtable of a multiply-inherited
class). The decompiler displays all field accesses as offsets from the inner
`this`, mixing small positive and negative offsets freely.

Before publishing any "field at `row + 0xNNN`" claim, explicitly work out which
MI-base `this` is in play. Take a known field (strongest anchor: the row's
position doubles at `row + 0x88/0x90`, or the primary vtable at `row + 0x00`) and
compute what the decompile calls it; the delta is the MI-base offset. Re-express
every other access as `row + (inner_offset + mi_base_offset)`. For
`OptionElement<T>` methods dispatched through the fourth MI vtable at `+0xC8`:
`this - 0x40` is `row + 0x88`, `this - 0x68` is `row + 0x60`, `this + 0x50` is
`row + 0x118`. Mis-attributing by one MI-base slot (0xC0 vs 0xC8, or 0x28 vs
0xC8) silently shifts every field claim by 0x08..0xA0 bytes and corrupts the
downstream design. Also: when a slot "disappears" from a vtable whose layout
you've inferred, check whether that vtable simply ends earlier and what looks
like "slot 8" is actually slot 0 of the next MI vtable.

### Re-verify every load-bearing claim in an RE handoff

**Context:** Receiving a handoff from a prior RE session that summarizes findings
("the visibility handler is at third MI slot 8", "textlayer_bind creates the
BmpString") and proposes a design premised on them.

Re-verify every structural claim against Ghidra disassembly (static) AND live
memory if available (Cheat Engine) before building on it. Bullet-point claims like
"at slot N" and "creates the BmpString if X succeeds" are structurally
load-bearing — if wrong, the proposed design is wrong too. Cost of checking each
claim is 2–3 Ghidra calls; cost of not checking is a research doc that perpetuates
the error. In the scalar-row investigation, a handoff had three wrong claims (a
vtable-slot mis-id, a function-role mis-attribution for BmpString creation, and
offset errors for row fields) — each caught by one disassembly read, and none of
the design based on them would have worked. Treat "what the agent observed in
memory" (offset X had value Y) as evidence, but "what the agent inferred about a
function's role" as a hypothesis needing re-proof.

### A panic in any `extern "C"` hook body aborts the whole process — non-deterministically

**Context:** Non-deterministic boot crash: the window vanishes, nothing in
`./log.txt`, and a stack trace captured on video ends with "thread caused
non-unwinding panic. aborting." on an AVS worker thread
(`dll_entry_init → arkMDXGetServerMessage → ess_eamuse_config_opt_update →
ess_soft_info_get → [libavs config/property funcs] → [our code] → abort`).

That exact message is what Rust emits when a panic tries to unwind **out of an
`extern "C"` function** — unwinding across the FFI boundary is UB, so the runtime
force-aborts. So the crash IS a Rust panic inside one of our hook callbacks; the
non-determinism is just *which* file/timing triggers it. Two things made it
invisible + hard to pin: (1) the default panic handler writes to stderr, which
spice2x doesn't capture — nothing reaches `./log.txt`; (2) the offending hook
(`avs_layeredfs::file_hooks`, the five `avs_fs_*` callbacks + `GetLongPathNameA`)
had no `catch_unwind` and contained `.unwrap()` sites (`CString::new(...).unwrap()`,
`STATE.lock().unwrap()`) — a latent CLAUDE.md-rule-1 violation. The AVS fs hooks
fire on arbitrary threads *including the boot config worker*, so any panic there
(bad path bytes, poisoned lock, interior-NUL CString) aborts at boot.

**Two-part fix, both reusable:**
1. **Install a global panic hook** (`logger::install_panic_hook`, called first in
   `init()`) that routes panic message + `file:line:col` + thread through
   `OutputDebugStringA` at ERROR level. It fires *before* any `catch_unwind`
   swallows the payload, so even a recovered panic is now visible in the log.
   This is why the crash never appeared in `log.txt` — fixed permanently.
2. **Wrap every `extern "C"`/`extern "system"` hook body in `catch_unwind`** and,
   on panic, fall through to the original unmodified call (serve the file vanilla,
   skip only our logic for that one call). `retour::GenericDetour` isn't
   `UnwindSafe`, so wrap the closure in `AssertUnwindSafe` (the `scene_manager`
   idiom) — sound here because the recovery path re-invokes the original and
   doesn't depend on any half-mutated state.

**General rule:** audit for hook callbacks lacking `catch_unwind` proactively —
`grep 'extern "C" fn'` vs the `catch_unwind` site list. A hook on a hot/early
path with an `.unwrap()` is a latent non-deterministic abort, not a "can't
happen." Adding init work that shifts boot thread-timing can change how often a
pre-existing race-y panic fires without being its cause (nothing of the new work
was on the crash stack).

**RESOLUTION FOLLOW-UP (2026-07-04):** the two-part fix above was necessary but
NOT sufficient — the crash persisted because the surviving `.unwrap()` was in the
`get_hook!` macro, evaluated BEFORE the `catch_unwind` wrapper in each callback.
The actual trigger was the detour-install race described in the next entry. Also:
the panic hook DID capture the payload on the next crash — the `PANIC at
file_hooks.rs:379` line was in log.txt all along, buried in `ea3-pos:` spam, and
a session concluded "no PANIC line" without grepping. **When checking a boot log
for the panic hook's output, `grep PANIC log.txt` — never eyeball it.**

### Store the detour handle BEFORE enabling it (install race = boot abort)

**Context:** every `retour::GenericDetour` install site. The callback can't
capture, so it reads its own handle back out of a `static mut`/`OnceLock` slot
to call the original through the trampoline.

The instant `hook.enable()` patches the target's prologue, ANY thread may enter
the callback — not just after the installer returns. The pervasive pattern
`enable() → log_info!() → SLOT = Some(hook)` leaves a window (widened by the
`log_info!`'s OutputDebugStringA round-trip) where the callback runs with the
slot still `None`. For a target that foreign threads hit during boot
(`GetLongPathNameA`, called by the AVS config worker inside libavs's file-open,
`FUN_1800082c0`), an `.unwrap()` on that slot is a non-deterministic
process-abort: panic → can't-unwind context → abort. This was the 2026-07 boot
crash; it consistently "stopped" the log at `custom_options: init` only because
the init thread ran ~20ms past the AVS worker's abort before teardown — a
timing red herring pointing at the wrong subsystem.

**Rule:** store the handle into its slot FIRST, then enable; on enable-failure,
clear the slot and report. Use `core/hooks.rs::install_enabled()` (added
2026-07-04, all sites converted) instead of hand-rolling
`GenericDetour::new/enable`. For `OnceLock` slots: `.set(detour)` first, then
`.get().map(|d| d.enable())`. And regardless, the callback must handle a `None`
slot by bailing gracefully (serve the original/a benign default), never
`.unwrap()` — belt and suspenders for the disable/teardown direction, which
`take()`s the slot while a callback may be mid-flight.

## BmpfontSimpleString has TWO alignment fields (2026-08-13)

The `kt::BmpfontSimpleString` line desc carries two alignment dwords, and
the widget API originally wrote the wrong one:

- **`desc+0xA8` = HORIZONTAL per-line alignment** — the render function
  offsets each line by its own PRE-MEASURED width (exact glyph metrics
  from the layout pass): 0 = left, 1 = center (`x += width × −0.5`,
  `DAT_18035a700`), 2 = right (`x −= width`). This is the reliable way to
  center a label about `set_position`'s x — text can change freely, no
  caller-side width math.
- **`desc+0xAC` = VERTICAL block alignment** (a separate mode switch in
  the same render fn; value 3 is a scrolling/marquee-like mode touching
  `state+0x98/+0xA0/+0xA4`).

Writing Center to +0xAC looks like "alignment does nothing" — the string
stays left-anchored (cabinet-observed on the training toast, then pinned
by decompiling the render fn at `render_function`'s match, FUN_18020cca0
on 20260721). `TextWidget::set_alignment` now writes +0xA8. The
14.8 px/char width-estimate trick (autoplay watermark) remains useful only
where an actual pixel WIDTH is needed (e.g. bounce extents) — never for
centering.

## GamePlayActor +0x178 is garbage until the anchor lands (2026-08-14)

The per-frame RAW music count at `GamePlayActor+0x178` derives from the
clock anchor at `+0x160` (`music_count = vt+0x248() + frameTick −
SOUND_OFFSET − anchor`). Before the run's first `0x1044` broadcast (DPS
state 6) the anchor is 0, so `+0x178` reads as the raw frame tick —
MINUTES-since-boot scale, comfortably inside
`current_raw_music_count()`'s 1-hour sanity range, so the accessor does
NOT filter it (cabinet-observed: `count 304644 ms` at 0.85 s into a
song, which disarmed the Step-4 training loop at entry).

Rules for anything comparing the music count against chart positions:

- Gate on `song_reset::first_anchored_frame()` before the FIRST
  meaningful read (DPS step 7 + actors step 4 + anchor ≠ 0 — the
  silent-start adjust's own gate).
- Even on the first anchored frame, `+0x178` is a per-frame CACHED
  value and can lag the anchor by one frame. Add a credibility check
  where a stale read is load-bearing: a live pre-cascade run can never
  read at/past the CMA `+0x98` song-over threshold, so
  `count < chart_end_raw` is a cheap "the cache caught up" predicate.
- The actor tree (and thus bound/threshold resolution) exists SECONDS
  before the anchor — "resolution completed" is not "clock valid".

## Scene-change callbacks used to fire UNDER the scene-manager mutex (2026-08-14)

`scene_manager::scene_hook` dispatched the `on_scene_change` callbacks
while HOLDING the `SCENE_MANAGER` mutex (the code's own "fire callbacks
outside the lock" comment was a lie). Any callback that re-entered the
scene manager — `current_scene()`, `add_redirect_once`, registering
another callback — re-locked the same non-reentrant `std::sync::Mutex`
from the same thread and DEADLOCKED THE FRAME THREAD: rendering freezes
on the last presented frame, input and the test menu die, only
background threads (spice api/keepalive) stay alive. Cabinet-hit by
training's threshold-restore condition evaluating `current_scene()`
inside the gameplay-exit callback (log_freeze 2026-08-14; the `&&`
short-circuit had hidden it until the first run with written
thresholds).

Fixed at the source: callbacks are now stored as `Arc` internally, the
hook snapshots the list under the lock and fires OUTSIDE it (same
pattern as `song_reset::SUBSCRIBERS`). Corollaries:

- A callback removed concurrently may still fire one final time —
  callbacks must tolerate firing after removal (gate on the mod's own
  ACTIVE latch, which every mod already does).
- Even so, treat scene callbacks like hook callbacks: prefer the
  `prev`/`next` ARGUMENTS over `current_scene()`, and keep them free of
  locks shared with other engine-facing paths ("no locks across engine
  calls" applies — the callback runs synchronously inside the game's
  scene transition).
- Debug signature of this class: the scene-change log line prints, SOME
  mods' exit lines print (registration order), then total frame-thread
  silence while spice api/keepalive lines continue.

## Diff-driven display actors: restore VALUES, leave display latches STALE (2026-08-16)

The gauge HUD (percent family `FUN_180073f90` AND LifeGauge
`FUN_1800709a0`) is diff-driven: the update body EARLY-OUTS while
`displayed_latch == live_value`, and everything interesting — the
gauge_usr color re-label AND the 0x1037/0x1038 danger on/off subtree
broadcasts — lives BEHIND that gate, keyed on the OLD display-state
latch vs a fresh classify. The in-place reset originally "helpfully"
snapped the displayed latch to the restored value and zeroed the state
latch, which (a) held the early-out closed forever (gauge COLOR frozen
until the first judge moved the value) and (b) destroyed the "was in
danger" evidence, so the engine never emitted the danger-off and the
lane kept flashing red.

The rule: when resetting state that a diff-driven display actor
watches, write ONLY the authoritative value fields and leave every
display latch (displayed value, animation velocity, last-state enum,
display-mode) untouched-stale. The next update tick then takes the
full path and the ENGINE performs the re-label and the on/off
transition broadcasts itself — with the correct old state. Emulating
the display side by hand means re-implementing (and racing) the
engine's own transition protocol.

Corollary for RE: a ctor seeding two adjacent fields from one param
does not make them the same thing — LifeGauge `+0x94` looked like
"starting lives" (ctor writes lives to both +0x90 and +0x94) but is
actually the update's last-displayed latch (update ends
`+0x94 = +0x90`). Check the UPDATE function's writes before naming a
field from the ctor alone.

## The ScreenRenderer layer-slot table is NOT walkable (2026-08-25)

`render_notes_hook::active_command_list()` (`state+0x68` index →
`state+0x40+idx*8`) is the ONLY verified-safe command-list append surface.
An overlay-draw experiment that walked ALL 9 slots and appended the menu's
background block to every valid-looking list (null checks + bump-invariant
gate passed!) crashed in-engine within seconds — non-active slots can hold
lists the engine is concurrently consuming or resetting, and field reads
on a torn list pass plausibility gates while still being garbage. If a
feature needs to reach a NON-active layer's list, that's new RE (find the
layer walk/composition), not a loop over the table.

Two adjacent facts from the same session (mod-menu animated backgrounds):
- BmpString wrappers call the hooked `wrapper_render` only while DIRTY
  (`render_state+0x68` byte); static text is served from a cached path.
  A widget can self-sustain a per-frame render chain by re-setting its
  dirty flag POST-original (the render pass clears it — a pre-original
  write is clobbered).
- WHICH list is active at a wrapper's rasterization is NOT the layer the
  wrapper's display quads composite in — an emission tied to the menu's
  own anchor wrapper landed in a list composed BELOW the attract movie
  even though the menu text draws above it. Rasterization time ≠ display
  time.

## Same command list ≠ same z — position within the layer walk decides (2026-08-25)

The mod-menu animated background rendered everywhere EXCEPT two loading
interstitials, with three successive "obvious" mechanisms all disproven by
cabinet evidence (arena wipe, list switch, scissor/RT mismatch). Root
cause: the emission appended at the widget layer's SEGMENT START, and on
loading screens the full-screen loading art itself renders through the
WIDGET layer's own wrapper walk (the layer register boot-installs BM2D
group wrappers into the same managers the DLL's widgets use) — burying a
segment-start quad while later-registered widgets stay visible. Getting
"into the right layer's list" is only half the job; z within the layer =
position within the walk. The fix is an IDENTITY-GATED anchor: emit only
at a menu-owned anchor wrapper's own `wrapper_render` (created first in
the menu's allocation), never via per-(list,frame) dedup — the dedup
variant let an earlier game wrapper claim the emission below the art.

Diagnostic craft from the same session:
- A per-frame arena reset only rewinds size/write — LAST frame's record
  chain stays readable at emission time. A bounded tag-size chain dump of
  the stale bytes is a complete picture of what the layer drew and in
  what order, for one INFO line per second.
- First-8-bytes "survival probes" can COLLIDE: the layer walk's own first
  record (`07:14`, canvas 1280.0) is byte-identical to our block's
  `set_context_2d` head. Compare full chains, not sentinels.
- When a draw is provably in the consumed list but invisible, split the
  hypothesis space with cheap live tests before RE: navigate the UI on
  the broken screen (live vs frozen presentation), re-bind to program 0
  with a plain quad (shader-specific vs draw-path), strip one state
  record at a time. Each test killed a whole theory class in one run.
