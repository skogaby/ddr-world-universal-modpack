# Research: preview re-trigger mechanism + game preview machinery

Date: 2026-08-15. Static RE against `gamemdx_20260721.dll` (primary) with a
20260616 cross-check. All addresses file-relative to base `0x180000000`,
20260721 unless noted. Complements `docs/xact_streaming_research.md`,
`docs/xact_audio_research.md`, and `docs/training_mode_research.md` §8.

## 1. The game's preview pipeline, end to end **[static]**

### 1.1 Object chain

```
TransitionSequence (scene_manager)
  └─ *(TS+0x58)                       active scene child; at scene 25 =
     SelectMusicSequence               sequence::selectmusic::SelectMusicSequence
                                       (RTTI-confirmed, vftable 0x18036e5b0 region)
       ├─ +0xB0  → 0x400-byte object (FUN_1800fcfc0 ctor — wheel/BGM related)
       └─ +0xB8  → sequence::selectmusic::View  (0x4A0 bytes, ctor FUN_18010b090,
       │            vftable 0x18036ef68, RTTI ".?AVView@selectmusic@sequence@@")
       │    └─ +0xC8  sequence::AudioPlayer   (EMBEDDED member, vftable
       │              0x18035d6xx region — named in the View ctor)
       │         ├─ +0x08  unique_ptr<AudioLoader>  ← the ONE live request
       │         ├─ +0x18  std::string (path, dedupe store)
       │         ├─ +0x40  std::string (cue, dedupe store)
       │         └─ +0x68/+0x70/+0x78  deferred-request list
       └─ (ctor FUN_1800fc100 = SelectMusicSequence virtual, vtable slot at
           0x18036e628; creates View, wires selectmusic lambdas)
```

Both objects are allocated in `FUN_1800fc100` (`SelectMusicSequence`
virtual): `View = FUN_18010b090(malloc(0x4A0))` stored at `this+0xB8`.

### 1.2 Request flow (wheel settles on a song)

- The highlighted-song observer lambda (`selectmusic::<lambda23>`, event id 7
  on the manager's callback table `DAT_1806f2d50+0xC0`, registered by
  `View::setup` `FUN_18010b580`) fires `FUN_180111b80` → **preview request**
  `FUN_18010eab0(capture+8)`:
  - reads highlighted song shared_ptr at `DAT_1806f2d50+0x1B0/+0x1B8`,
  - builds path `data/sound/win/dance/<code>` and cue `<code>_s`
    (suffix literal at `0x18036eee8`),
  - calls `FUN_1801ccd10(View+0xC8, 5, path, cue, delay)` with
    `delay = DAT_1803a3698` = **0.4 s** (double) — the game's own
    wheel-settle debounce.
- `FUN_1801ccd10(AudioPlayer* q, int slot, string* path, string* cue,
  double delay)` — the request façade:
  - **Dedupe**: `FUN_18003b4b0(q+0x40, cue)` — same cue as stored ⇒ ENTIRE
    call is a no-op (why wheel jitter never restarts a preview).
  - Stores path/cue at `q+0x18`/`q+0x40`, clears the deferred list.
  - `delay > 0`: schedules a deferred thunk (`FUN_1801d2260(q+0x68, delay,
    thunk)`); `delay ≤ 0`: executes immediately:
    `loader = FUN_18002c9f0(slot, path, cue)` (slot ≠ 0) then
    `FUN_180031170(q+8, &loader)` — a **unique_ptr swap**: the OLD loader is
    released (`FUN_18002ce10`) + freed; the new one installed. There is
    exactly ONE loader at a time.

### 1.3 `sequence::AudioLoader` (0x70 bytes, ctor `FUN_18002cb90`, vftable `0x18035d5a8` — ONE virtual slot)

| Offset | Field |
|---|---|
| +0x00 | vftable (slot 0 = per-frame tick `FUN_18002cf30`) |
| +0x08 | i32 XWB file_id (resolved in ctor, **ref acquired**) |
| +0x0C | i32 XSB file_id (ditto) |
| +0x10 | i32 cue handle (−1 until played) |
| +0x14 | u8 failed flag |
| +0x15 | u8 mode (1 = se_play one-shot — the preview path; 0 = BGM/loop play via `FUN_1801aa5c0`) |
| +0x18 | i32 slot (5 for previews) |
| +0x20 | std::string path |
| +0x48 | std::string cue |

- Ctor: appends `/%s.xwb` / `/%s.xsb` to the path, resolves each through
  `FUN_1801fef30(DAT_1806f2f48, full_path)` = **FileManager acquire**:
  existing live row ⇒ `refcount++ (row+0x24)` and returns its file_id; no
  row (or release-state row — invisible to the lookup, `FUN_1801febd0`)
  ⇒ allocates a NEW row, tags category "sound", queues the load.
- Per-frame tick `FUN_18002cf30` (the only vtable slot): gates on both rows'
  load state (`row+0x20` ∈ {0, 5, 6, 8}) AND `handle == −1` AND
  `!failed`, then plays: mode 1 ⇒ `se_play(slot, cue, pan=0)`
  (`FUN_1801aa6e0`; slot 5 skips the SE mute filter), stores the returned
  handle at +0x10 (−1 ⇒ failed=1). **Fires exactly once; setting +0x10
  back to −1 re-arms it** — the replay lever.
- Release `FUN_18002ce10`: stop cue by stored handle
  (`FUN_1801aa7c0(handle)` — handle-table stop: entry at
  `DAT_1806f2d60 + (handle+5)*0x20`; live cue ⇒ cue vt+0x08 `Stop(0)`
  (non-immediate); dead entry ⇒ soundbank fallback), then release BOTH file
  refs (`FUN_1801ff1b0` — pushes file_id onto the FileManager release ring;
  the sweep unloads at refcount 0, and the "sound"-category unload
  unregisters the banks: `FUN_1801ac6c0` → `wavebank_unregister +0x1AB3D0`).

### 1.4 Bank creation is load-completion-driven

`FUN_1801aa520(file_id)` — the **sound-bank create router** (called from the
FileManager Task callbacks `FUN_1801ac5a0`/`FUN_1801ac650` at load
completion): path extension `.xsb` ⇒ sound-bank create `FUN_1801aafa0`,
anything else ⇒ `wavebank_create +0x1AB050` (**our detoured function** —
calls into the router's target land on the patched entry, so the binding
detour composes for free).

## 2. Same-song re-trigger: why the game's own path can't do it

- `FUN_1801ccd10` dedupes on the stored cue — same cue ⇒ no-op.
- Even bypassing dedupe: the new loader's ctor **acquires refs before the
  swap releases the old ones** (1→2→1), so the rows never release, the
  banks never unregister, and the tick would replay the cue on the OLD bank
  whose header (entry durations) was parsed at its create — **a live bank
  cannot change its declared entry length**. Rate change requires a fresh
  bank create.
- Null-swap-then-re-request (forcing the stock release → fresh row path)
  works in principle but needs the FileManager sweep to transition the row
  to release state between the two steps (≥1 frame, unbounded), and reloads
  the whole XWB from disk. Rejected in favor of §3.

## 3. The chosen re-trigger: in-place AudioLoader restart **[static, needs cabinet validation]**

All steps on the game thread; every function is stock and already exercised
in this exact role by the game itself:

1. Resolve the loader: `TS → *(TS+0x58) → 
   check scene==25 + child alive (flags+0x20 & 0x24 == 0) →
   View = *(child+0xB8) → identity-check *View == View::vftable →
   loader = *(View+0xC8+0x8)`; verify `slot==5`, both file_ids ≠ −1, both
   rows loaded, cue string matches `*_s`.
2. **Stop** the playing cue: `FUN_1801aa7c0(*(loader+0x10))` — the exact
   stop the game's own teardown uses.
3. **Unregister** both banks: call the (detoured) `wavebank_unregister`
   entry for XSB id then XWB id (stock order per the 2026-08-05 timeline:
   133 then 132) — the existing unregister prelude retires any live preview
   binding first.
4. **Arm** the preview binding intent, then **recreate** both banks:
   `FUN_1801aa520(xwb_id)` + `FUN_1801aa520(xsb_id)` — the XWB create runs
   through our create detour, which qualifies and publishes the
   `_s`-stretched virtual bank (or nothing at 100%). The FileManager rows
   are untouched throughout (refcounts, RAM buffer, load state) — no disk
   I/O, no sweep dependency.
5. **Re-arm the tick**: `*(loader+0x10) = −1`, `*(loader+0x14) = 0` — the
   game's own per-frame tick replays the cue next frame (`se_play(5,
   "<code>_s", 0)`) and stores the fresh handle, keeping the loader's
   teardown bookkeeping fully consistent (no stale-handle hazard).

Wheel-move / song-confirm / scene-exit teardown then work stock (the loader
release stops OUR replayed cue by ITS stored handle and releases the refs;
unregister retires the preview binding through the existing prelude).

## 4. R-A (gameplay-header safety) — RESOLVED, cabinet-proven

From the 2026-08-05 song-playback-speed bank-event timeline (instrumented
cabinet run, `.agents/planning/2026-08-05-song-playback-speed/progress.md`):
at song confirm the preview XSB+XWB are UNREGISTERED (`t+91000ms
UNREGISTER file_id=133` + `132`) ~2.5 s BEFORE the gameplay create, which
lands on a **fresh file_id** (1638) — the preview row is release-state and
invisible to the lookup, so the gameplay loader registers a NEW row and the
duplicate guard never sees the preview bank. A preview-stretched header can
never leak into gameplay via the natural flow. (Defense-in-depth: the
runtime scene callback force-retires any live preview binding on leaving
scene 25 anyway.)

## 5. Cross-version notes

- 20260616 preview player `FUN_18010db40`: structurally identical —
  `*param_1+0xC8` (AudioPlayer offset **0xC8 stable**), manager
  `DAT_1806f1d58+0x1B0`, same 0.4 s delay-global pattern, same string flow.
- The four-build signature matrix (create/unregister/file-table) already
  exists (`docs/xact_streaming_research.md` §6); the new derivations
  (View ctor → vftable, AudioLoader ctor → vftable, create router, handle
  stop) need the same four-build AOB validation at design time.
- Engine-side (xactengine2_10.dll, byte-identical across releases): sound
  banks resolve wave banks lazily BY NAME; wave-bank destroy + re-create
  under the same internal name re-resolves. Recreating BOTH banks (stock
  confirm-time pattern) sidesteps any stale-pairing question.

## 6. Function/global inventory (20260721)

| Item | Address | Role |
|---|---|---|
| preview request | `0x18010eab0` | builds path/cue, calls the AudioPlayer façade (research only — never called by the mod) |
| AudioPlayer façade | `0x1801ccd10` | dedupe + deferred/immediate loader swap |
| AudioLoader ctor | `0x18002cb90` | vftable `0x18035d5a8`, field layout §1.3 |
| AudioLoader tick | `0x18002cf30` | row-state gate + one-shot se_play |
| AudioLoader release | `0x18002ce10` | stop handle + release refs |
| loader swap | `0x180031170` | unique_ptr swap (release-old) |
| cue-handle stop | `0x1801aa7c0` | handle-table stop (table at `DAT_1806f2d60+(h+5)*0x20`) |
| se_play | `0x1801aa6e0` | already resolved by game_audio |
| create router | `0x1801aa520` | .xsb ⇒ soundbank create `0x1801aafa0`; else ⇒ wavebank_create `0x1801ab050` (detoured) |
| FileManager acquire | `0x1801fef30` | path→file_id, refcount++ / new row |
| FileManager release | `0x1801ff1b0` | release-ring push |
| unload→unregister | `0x1801ac6c0` | wraps `wavebank_unregister 0x1801ab3d0` |
| View ctor | `0x18010b090` | vftable `0x18036ef68` (identity gate) |
| View setup | `0x18010b580` | lambda registration incl. the preview observer (id 7) |
| SelectMusicSequence ctor-ish | `0x1800fc100` | View alloc/store at `this+0xB8` |
| selectmusic manager | `DAT_1806f2d50` | +0x1B0 highlighted song, +0xC0 event table |
| preview delay | `DAT_1803a3698` | double 0.4 (s) |
| `_s` suffix literal | `0x18036eee8` | cue suffix |

## 7. What remains for cabinet validation

1. The in-place restart sequence end-to-end (stop → unregister×2 →
   create×2 → tick replay) — latency and audio cleanliness (the stop is
   non-immediate; unregister lands ~0 ms later vs the stock sweep's ~1
   frame — stock-shaped but compressed).
2. Preview binding streaming under CrossOver at 25 %/175 % (start latency
   target < ~1 s slow-path).
3. Preview end-of-cue behavior at slow rates (stop vs loop — inherited
   stock semantics either way, D6).

## 8. Deploy-#1 incident RE addendum (2026-08-16): the WSOLA cue-start race

Cabinet deploy #1 (build md5 2f757e76…) showed inconsistent silent previews
in pitch-preserved mode. Log forensics + engine RE:

- Every preview bound and reclaimed correctly; refusals zero. The
  discriminator: WSOLA previews split into ~19 ms vs **~583 ms** max read
  deferrals (constant ±4 ms); resample previews ≤ 8 ms and all audible.
- ~583 ms = time to synthesize the engine's FIRST data packet (64 KiB ≈
  1.27 s of output audio) at WSOLA's ~2.2× realtime under CrossOver —
  **output-frame-bound, hence rate-independent**.
- The AudioLoader tick fires `se_play` as soon as the file ROWS are
  resident (always, at create) — it never waits for XACT stream prepare.
  Stock banks prepare ~instantly; our WSOLA preview leaves a ~0.6 s
  unprepared window. A Play landing in it can fail ⇒ the loader latches
  `failed` (+0x14) and NEVER retries ⇒ silent preview until the next
  settle. Engine-pump timing decides which settles hit the window — the
  observed "no pattern".
- **Short completions are UNSAFE (verified in xactengine2_10.dll):** the
  single completion-poll site `FUN_004274ca` (the binary's only
  `call [reg+0x198]`, confirming §3) passes `&bytes` to the
  getOverlappedResult callback and NEVER READS IT BACK — the completion
  handler (`vt+0x110`) receives no count. The engine assumes its full
  request arrived; serving short would make the decoder consume stale
  buffer tail. The EOF "short completion" tolerance exists only because
  the engine never requests past the declared stream length.
- Initial-read size is engine-fixed: `FUN_004265d0` (wave Prepare) sets
  the first read to `min(stream buffer capacity, remaining)` rounded to
  block align — the full 64 KiB packet for ADPCM. No smaller first read
  can be induced per-bank from outside.

Conclusion: the ~0.6 s pitch-preserved first-audio latency is irreducible
within safe bounds. The reliability fix is the **preview play watchdog**
(Step 5 executor duty): when a live preview binding's initial window is
produced and the loader sits failed-latched (`handle == −1` / `failed ==
1`), clear `failed` and re-arm `handle = −1` so the game's own tick
retries `se_play`. Maintainer decision 2026-08-16: "slightly late but
reliable" accepted for pitch-preserved slow-rate previews (resample mode
is unaffected).

## 9. Step-4 signature matrix (validated 2026-08-16, exactly one match per build)

Byte-level authority for the four `SignatureDefinition`s in
`src/core/signatures.rs`. Wildcards per house style: every RIP disp32,
CALL rel32, stack-frame displacement, and branch displacement is `??`;
semantic immediates and struct-field offsets stay literal so a layout
change breaks the match instead of silently mis-resolving. Validated via
`ghidra_search_byte_patterns` on all four supported builds.

### 9.1 `audio_loader_ctor` (match = ctor entry + 0x3F on 20260721)

```
48 8D 05 ?? ?? ?? ??    LEA RAX,[rip+AudioLoader::vftable]  ← disp32 at match+3
48 89 01                MOV [RCX],RAX                        vftable install
48 C7 41 08 FF FF FF FF MOV qword [RCX+0x8],-1               XWB/XSB ids = −1,−1
C7 41 10 FF FF FF FF    MOV dword [RCX+0x10],-1              cue handle = −1
C6 41 14 00             MOV byte  [RCX+0x14],0               failed = 0
0F B6 45 ??             MOVZX EAX,byte [RBP+disp8]           (stack arg — wildcarded)
88 41 15                MOV [RCX+0x15],AL                    mode
89 51 18                MOV [RCX+0x18],EDX                   slot
```

Yield: `AudioLoader::vftable` = `decode_rip_relative(match+3)` (one
virtual slot: the per-frame tick). The −1 initializers and the
0x08/0x10/0x14/0x15/0x18 field offsets are the loader-layout facts the
restart executor's constants rest on — kept literal as the layout gate.

| Build | Match | vftable |
|---|---|---|
| 20260324 | `0x18002cbff` | — |
| 20260421 | `0x18002cdbf` | — |
| 20260616 | `0x18002d0ff` | `0x18035c5b8` (decoded, slot-0 in-module) |
| 20260721 | `0x18002cbcf` | `0x18035d5a8` (== §1.3) |

### 9.2 `selectmusic_view_ctor` (match = ctor entry)

94-byte pattern over the ctor head: prologue (stack disp wildcarded), the
base-ctor CALL (wildcarded), then the vftable-install cluster. **The View
vftable is the SECOND LEA (`4C 8D 1D`, R11, disp32 at match+30), stored
bare to `[RBX]` by `4C 89 1B`** — the first LEA (`48 8D 05`, disp32 at
match+23) is an inner interface vftable stored at `+0x28`. Literal
layout pins: the `+0x28` store, `[RBX+0x1E8]` LEA, `+0xC0`/`+0xD0`
pointer clears, the third LEA's store to **`+0xC8` (the embedded
`sequence::AudioPlayer` — THE load-bearing offset)**, `+0xD8 = 1`, and
the `+0xF8 = 0xF` std::string-capacity init.

Yield: `View::vftable` = `decode_rip_relative(match+30)` (the restart
executor's identity gate for the `child+0xB8 → View` walk).

| Build | Match | View vftable |
|---|---|---|
| 20260324 | `0x18010a700` | — |
| 20260421 | `0x18010afb0` | — |
| 20260616 | `0x18010a120` | `0x18036df68` (decoded; xrefed only from ctor/dtor) |
| 20260721 | `0x18010b090` | `0x18036ef68` (== §6, RTTI-confirmed) |

### 9.3 `cue_handle_stop` (match = function entry)

Pattern spans entry through the two vtable-dispatch branches: the lock
prologue (`LEA RAX,[rip+lock]` / count test / `FF 15` EnterCriticalSection
— disps wildcarded), then the distinctive body: `CMP EBX,-1; JZ;
LEA RAX,[RBX+5]; SHL RAX,5; ADD RAX,[rip+handle_table]` (the `(h+5)*0x20`
indexing, literal) and both dispatch arms — live cue ⇒ `FF 50 08` (cue
vt+0x08 `Stop(0)`), dead entry ⇒ `BA 01 00 00 00` + `FF 50 10`
(soundbank vt+0x10, flags=1). The vtable offsets and the stop-flags
immediate are kept literal.

Yield: the function address itself (executor step 2).

| Build | Match |
|---|---|
| 20260324 | `0x1801a7c30` |
| 20260421 | `0x1801a8900` |
| 20260616 | `0x1801a9730` |
| 20260721 | `0x1801aa7c0` |

### 9.4 `sound_bank_create_router` (match = function entry)

Pattern spans entry through both dispatch calls: lock prologue (disps
wildcarded, the post-call `90` NOP retained — present on all four
builds), then the distinctive FileManager row walk: `LEA RCX,[RBX+RBX*4];
SHL RCX,5` (0xA0 row stride), `ADD RCX,[RAX+0x28]` (rows base),
`MOVZX EAX,[RCX+0x8F]` (path length byte), `LEA RCX,[RAX+RCX+0x11]`
(extension backset), `MOV R8D,3` + `LEA RDX,[rip+"xsb"]` + strncmp CALL,
and the two-way dispatch (`75 ??` ⇒ wavebank-create arm, fallthrough ⇒
soundbank-create arm; both CALL rel32s wildcarded, both `0F B6 D8`
result captures literal). Row-layout offsets 0x28/0x8F/0x11 literal.

Yield: the function address itself (executor step 4 — calls into it land
on the detoured `wavebank_create` for the XWB, so the re-create composes
with the preview branch for free).

| Build | Match |
|---|---|
| 20260324 | `0x1801a7990` |
| 20260421 | `0x1801a8660` |
| 20260616 | `0x1801a9490` |
| 20260721 | `0x1801aa520` |
