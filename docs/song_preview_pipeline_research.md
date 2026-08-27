# Song-Select Preview Pipeline & Rate-Bound Previews

Reverse-engineering notes for the song-select preview player
(`sequence::AudioPlayer` / `sequence::AudioLoader`) and the mechanism the
Song Playback Speed mod uses to rate-bind and live-restart previews.
Static RE against `gamemdx_20260721.dll` (primary) with a 20260616
cross-check and four-build AOB validation; addresses file-relative to
base `0x180000000`, 20260721 unless noted. Complements
`xact_streaming_research.md` (the streaming binding engine the previews
reuse) and `xact_audio_research.md` (se_play / bank pairing).

Feature planning record (decision register, design, deploy log):
`.agents/planning/2026-08-15-song-preview-rate/`.

## 1. The game's preview pipeline, end to end

### 1.1 Object chain

```
TransitionSequence (scene_manager)
  └─ *(TS+0x58)                       active scene child; at scene 25 =
     SelectMusicSequence               sequence::selectmusic::SelectMusicSequence
       ├─ +0xB0  → 0x400-byte object (FUN_1800fcfc0 ctor — wheel/BGM related)
       └─ +0xB8  → sequence::selectmusic::View  (0x4A0 bytes, ctor FUN_18010b090,
       │            vftable 0x18036ef68, RTTI ".?AVView@selectmusic@sequence@@")
       │    └─ +0xC8  sequence::AudioPlayer   (EMBEDDED member)
       │         ├─ +0x08  unique_ptr<AudioLoader>  ← the ONE live request
       │         ├─ +0x18  std::string (path, dedupe store)
       │         ├─ +0x40  std::string (cue, dedupe store)
       │         └─ +0x68/+0x70/+0x78  deferred-request list
       └─ (ctor FUN_1800fc100 = SelectMusicSequence virtual; creates the View,
           wires the selectmusic lambdas)
```

### 1.2 Request flow (wheel settles on a song)

- The highlighted-song observer lambda (event id 7 on the selectmusic
  manager's callback table `DAT_1806f2d50+0xC0`, registered by
  `View::setup` `FUN_18010b580`) fires the preview request
  `FUN_18010eab0`: builds path `data/sound/win/dance/<code>` and cue
  `<code>_s` (suffix literal at `0x18036eee8`), then calls the
  AudioPlayer façade `FUN_1801ccd10(View+0xC8, 5, path, cue, delay)`
  with `delay = DAT_1803a3698` = **0.4 s** — the game's own wheel-settle
  debounce.
- The façade **dedupes on the stored cue** (same cue ⇒ the whole call is
  a no-op — why wheel jitter never restarts a preview). After the
  deferral a new `sequence::AudioLoader` is constructed — its ctor
  **acquires FileManager references** on `<path>.xwb` / `<path>.xsb`
  (creating rows and queuing loads if absent) — and swapped into the
  unique_ptr; the swap releases the OLD loader, which stops its cue by
  stored handle and releases its file refs (the FileManager sweep then
  unloads the rows and unregisters the old banks).

### 1.3 `sequence::AudioLoader` (0x70 bytes, ctor `0x18002cb90`, vftable `0x18035d5a8` — ONE virtual slot)

| Offset | Field |
|---|---|
| +0x00 | vftable (slot 0 = per-frame tick `0x18002cf30`) |
| +0x08 | i32 XWB file_id (resolved in ctor, ref acquired) |
| +0x0C | i32 XSB file_id (ditto) |
| +0x10 | i32 cue handle (−1 until played) |
| +0x14 | u8 failed flag |
| +0x15 | u8 mode (1 = se_play one-shot — the preview path; 0 = BGM/loop) |
| +0x18 | i32 slot (5 for previews — skips the SE mute filter) |
| +0x20 | std::string path |
| +0x48 | std::string cue |

- The per-frame tick gates on both rows' load state (`row+0x20` ∈
  {0, 5, 6, 8}) AND `handle == −1` AND `!failed`, then plays:
  `se_play(slot, cue, pan=0)`, storing the returned handle (−1 ⇒
  `failed = 1`). **Fires exactly once; setting `+0x10` back to −1
  re-arms it** — the replay lever the restart uses.
- Release: stop cue by stored handle (`cue_handle_stop 0x1801aa7c0` —
  handle-table entry at `DAT_1806f2d60 + (handle+5)*0x20`; live cue ⇒
  cue vt+0x08 `Stop(0)`, dead entry ⇒ soundbank vt+0x10 fallback), then
  release both file refs.

### 1.4 Bank creation is load-completion-driven

`sound_bank_create_router 0x1801aa520(file_id)` — called from the
FileManager "sound"-category task callbacks at load completion: path
extension `.xsb` ⇒ sound-bank create `0x1801aafa0`, anything else ⇒
`wavebank_create 0x1801ab050` — **the function the song-rate engine
detours**, so calls into the router compose with the create detour (and
its preview bind branch) for free.

## 2. Why the game's own request path can't re-trigger a song

- The façade dedupes on the stored cue — a same-song request is a no-op.
- Even bypassing dedupe: the new loader's ctor acquires refs BEFORE the
  swap releases the old ones (1→2→1), so the rows never release, the
  banks never unregister, and a live bank cannot change its declared
  entry lengths (XACT parses entry metadata once at create; there is no
  seek/redeclare path). A rate change ⇒ a different stretched length ⇒ a
  fresh header ⇒ a fresh bank create.

## 3. The rate-bound preview mechanism (Song Playback Speed mod)

Two halves, zero new hooks (`src/services/song_rate/preview.rs`):

1. **Wheel-settle binding**: every slot-5 dance-bank create at scene 25
   already flows through the detoured `wavebank_create`. After the
   gameplay path resolves to Stock, the preview branch qualifies
   (exactly one entered side desiring ≠ 100 % — versus/none ⇒ stock) and
   publishes a `StretchTarget::Side` virtual bank (stretched `_s` entry,
   verbatim main) into the registry's dedicated preview slot. Preview
   bindings never touch Q31/score/movie/lifecycle/XactSlots.
2. **Live-edit restart**: option-row edits stamp a debounce cell; a
   per-frame executor (input-manager frame callback, game thread) fires
   150 ms after the last tick (superseded if a wheel settle re-published
   the selected song meanwhile), re-validates the loader chain
   (`TS child → View (+0xB8, vftable identity) → AudioPlayer (+0xC8) →
   loader (+0x08, vftable identity)`; slot 5, mode 1, rows loaded, cue
   `*_s`), then runs the stock sequence: `cue_handle_stop(handle)` →
   `wavebank_unregister(xsb)` → `(xwb)` **through the patched entries**
   (the detour prelude retires the preview binding) →
   `sound_bank_create_router(xwb)` → `(xsb)` (the XWB create re-qualifies
   through the preview branch — or stays stock at 100 %) →
   `loader.handle = −1; failed = 0` (the game's own tick replays the
   cue). Every gate failure fails open; actionable failure classes
   (identity-gate mismatch, non-preview loader shape, unloaded rows,
   wrong cue, create failure) WARN once per class, while the
   loader-absent case declines silently — the scene-entry profile load
   seeds the persisted rate through the same change callback, so the
   executor's first fire routinely lands before any preview loader
   exists (deploy-#2 finding).

### 3.1 The preview play watchdog (WSOLA cue-start race)

The loader's tick fires `se_play` as soon as the file ROWS are resident
— it never waits for XACT stream prepare. A pitch-preserved (WSOLA)
preview's first engine packet (64 KiB, engine-fixed by the wave-Prepare
read sizing) takes ~583 ms to synthesize under CrossOver
(output-frame-bound ⇒ rate-independent); a Play landing in that window
fails and the loader latches `failed` forever ⇒ silent preview.

Engine RE (xactengine2_10.dll) ruled out the alternatives: the single
completion-poll site discards the completion byte count (the engine
assumes full requests — serving short would corrupt the decoder), and
the initial read size cannot be induced smaller per-bank. The accepted
behavior is **"slightly late but reliable"**: the same frame executor
watches the live preview binding, and once its produced watermark covers
`min(target_data_start + 64 KiB, target_data_end)` while the loader sits
failed-latched, clears `failed` and re-arms `handle = −1` — one retry
per preview generation.

## 4. Gameplay-header safety (cabinet-proven)

At song confirm the preview XSB+XWB unregister ~2.5 s before the
gameplay create, which lands on a **fresh file id** (release-state rows
are invisible to the FileManager path lookup), so a preview-stretched
header can never serve gameplay through the natural flow. Defense in
depth: a scene callback force-retires the preview binding on any
transition leaving scene 25.

## 5. Signatures (validated exactly-once on 20260324/20260421/20260616/20260721)

| Signature | Yields | 20260721 match |
|---|---|---|
| `audio_loader_ctor` | `audio_loader_vftable` (RIP-decode at match+3) | `0x18002cbcf` (entry+0x3F) |
| `selectmusic_view_ctor` | `selectmusic_view_vftable` (RIP-decode at match+30 — the SECOND LEA; the first is an inner interface vftable stored at +0x28) | `0x18010b090` |
| `cue_handle_stop` | the function (match = entry) | `0x1801aa7c0` |
| `sound_bank_create_router` | the function (match = entry) | `0x1801aa520` |

Byte-level authority (annotated disassembly, per-build match tables,
wildcard rationale):
`.agents/planning/2026-08-15-song-preview-rate/research/preview-retrigger-re.md`
§9. Loader/View struct offsets are compile-time constants gated at
runtime by the two vftable identities — layout drift on a future build
fails the walk closed (stock previews) rather than mis-poking.

## 6. Function/global inventory (20260721)

| Item | Address | Role |
|---|---|---|
| preview request | `0x18010eab0` | builds path/cue, calls the façade (research only) |
| AudioPlayer façade | `0x1801ccd10` | dedupe + deferred/immediate loader swap |
| AudioLoader ctor | `0x18002cb90` | vftable `0x18035d5a8`, layout §1.3 |
| AudioLoader tick | `0x18002cf30` | row-state gate + one-shot se_play |
| AudioLoader release | `0x18002ce10` | stop handle + release refs |
| loader swap | `0x180031170` | unique_ptr swap (release-old) |
| cue-handle stop | `0x1801aa7c0` | handle-table stop |
| create router | `0x1801aa520` | .xsb ⇒ soundbank create; else wavebank_create (detoured) |
| FileManager acquire | `0x1801fef30` | path→file_id, refcount++ / new row |
| unload→unregister | `0x1801ac6c0` | wraps `wavebank_unregister 0x1801ab3d0` |
| View ctor | `0x18010b090` | vftable `0x18036ef68` (identity gate) |
| View setup | `0x18010b580` | lambda registration incl. the preview observer |
| selectmusic manager | `DAT_1806f2d50` | +0x1B0 highlighted song, +0xC0 event table |
| preview delay | `DAT_1803a3698` | double 0.4 (s) |
| `_s` suffix literal | `0x18036eee8` | cue suffix |

Cross-version: 20260616 preview player `FUN_18010db40` structurally
identical (AudioPlayer offset **+0xC8 stable**, View vftable
`0x18036df68`, loader vftable `0x18035c5b8`).
