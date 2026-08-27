# XACT Streaming & File-IO Research — How DDR World's Song Banks Reach the Speakers

Durable reverse-engineering record for the song-rate streaming redesign (successor to the
deferred RE note from the retired file-cache delivery of the Song Playback Speed feature).
Complements `docs/xact_audio_research.md` (the engine surface, bank slots, cue playback —
from Assist Tick). This note covers the FILE side: how a per-song streaming XWB's bytes
travel from disk/RAM into the engine, byte for byte.

All gamemdx addresses are file-relative to image base `0x180000000`, build **2026-07-21**
unless noted. All xactengine2_10.dll addresses are file-relative to its `0x00400000` base
(the engine DLL ships with the game and is identical across game builds). Facts are marked
**[cabinet]** (live-proven during the retired feature's Step-4/5 investigation, 2026-08-06→08)
or **[static]** (Ghidra disassembly, this research pass, 2026-08-08).

## 1. The two-channel truth: handle vs RAM

For slot-5 (per-song dance) banks the audio pipeline is split in a non-obvious way:

- At **song confirm**, the game's FileManager loads the whole
  `data/sound/win/dance/<code>.xwb` into a RAM buffer via `avs_fs_open`/`avs_fs_read`
  (~3 s before the engine bank is created). The FileManager's xwb callback
  (`FUN_1801ac650`) returns NULL for slot 5, which selects the whole-file RAM load.
  **[cabinet]**
- At **bank create** (`wavebank_create`, `+0x1AB050`), gamemdx opens a real
  `CreateFileA` handle on the (path-converted) file — flags
  `0x60000000` = `FILE_FLAG_OVERLAPPED | FILE_FLAG_NO_BUFFERING` — and hands it to
  `IXACT2Engine::CreateStreamingWaveBank` (engine vtable `+0x58`). **[static]**
- That handle is **never read from disk** during normal play: gamemdx registers custom
  XACT file-IO callbacks, and its readFile callback services every engine read by
  memcpying from the FileManager RAM buffer. The handle functions only as a lookup key.
  **[cabinet]** (lsof + bank-timeline instrumented runs, 2026-08-07)

Consequence: whoever controls the read callback's answers for one handle controls every
audible byte of that bank, independent of what is on disk or in the FileManager buffer.

## 2. gamemdx's XACT file-IO callbacks

### Registration site (audio-manager constructor `FUN_1801aab60`)

The manager constructor builds `XACT_RUNTIME_PARAMETERS` on the stack and calls
`IXACT2Engine::Initialize` (engine vt `+0x30`):

```
1801aad34  C7 45 B7 FA 00 00 00      MOV dword [RBP-0x49], 0xFA      ; lookAheadTime = 250 ms
1801aad3b  48 8D 05 ...              LEA RAX, [0x1801aa0f0]          ; notification callback
1801aad42  48 89 45 E7               MOV [RBP-0x19], RAX
1801aad46  48 8D 05 ...              LEA RAX, [0x1801aa250]          ; readFile callback
1801aad4d  48 89 45 D7               MOV [RBP-0x29], RAX
1801aad51  48 8D 05 ...              LEA RAX, [0x1801aa350]          ; getOverlappedResult callback
1801aad58  48 89 45 DF               MOV [RBP-0x21], RAX
1801aad5c  ...                       CALL qword [RAX+0x30]           ; engine->Initialize(&params)
```

The `0xFA` immediate plus the three `LEA RAX/MOV [RBP+disp8]` pairs form a strong AOB
anchor; RIP-decoding the second and third `LEA` yields both detour targets (wildcard the
LEA disp32s and the frame disp8s across builds). The engine stores the pair in its own
object: readFile at engine`+0x190`, getOverlappedResult at engine`+0x198`. **[static]**

### `FUN_1801aa250` — readFile callback (ReadFile-shaped)

```
BOOL readfile_cb(HANDLE h, void* buf, DWORD len, DWORD* bytesRead, OVERLAPPED* ov)
```

1. `file_id = FUN_1801aba70(h)` — binary search of the sorted `{HANDLE → file_id}`
   vector at manager`+0x20C8..+0x20D0` (16-byte elements: handle, file_id). The walk is
   guarded by an AVS mutex (libavs imports `XCnbrep700000f`/`XCnbrep7000010`) gated on
   `DAT_1806f38fc > 0`.
2. Miss (`-1`) → plain `ReadFile(h, ...)` pass-through (real disk IO, possibly async).
3. Hit → **synchronous completion from RAM**: file size from the file table
   (`[DAT_1806f2f48+8] + file_id*0x40 + 0x14`, u32), buffer pointer at `+0x8`;
   the read offset is taken from the OVERLAPPED **as the full 64-bit union**
   (`ov->u.Pointer`, i.e. Offset|OffsetHigh); copy length = `min(len, size − offset)`;
   `memcpy` from `ram + offset`; `*bytesRead = copied`; **`ov->Internal += copied`**;
   returns TRUE iff `copied != 0` (a zero-size table entry returns FALSE). **[static]**

Note the accumulator: `OVERLAPPED.Internal` carries "bytes completed since last poll".

### `FUN_1801aa350` — getOverlappedResult callback

```
BOOL overlapped_cb(HANDLE h, OVERLAPPED* ov, DWORD* bytes, BOOL wait)
```

Miss → real `GetOverlappedResult`. Hit → `*bytes = ov->Internal; ov->Internal = 0;
return TRUE` — i.e. for RAM-backed files, completion polls always succeed instantly and
report whatever the readFile calls accumulated. **[static]**

### `FUN_1801aa0f0` — notification callback

Dispatches engine notifications by type: 1 = cue destroyed (`FUN_1801ab9d0` releases the
cue-handle table entry), 4/16 = sound/wave-bank destroyed (clears the matching manager
slot pair), 12 = wave-bank prepared (sets slot ready flag at `+0x10`), 17 = sound-bank
prepared. Not part of the read path but shares the manager object. **[static]**

## 3. The engine side: how streaming reads are issued and completed

Engine `IXACT2Engine` vtable at xactengine `0x402260` (DoWork `+0x40` = the named
`XACT2Engine_DoWork` `0x4122e0` confirms the base). `CreateStreamingWaveBank` = vt`+0x58`
→ `FUN_00410f00`:

- Validates `XACT_STREAMING_PARAMETERS`: **offset must be 2048-aligned**
  (`offset & 0x7FF == 0`) and **packetSize ≥ 2** (in 2048-byte sectors). gamemdx passes
  offset 0, packetSize `0x20` → **64 KiB packets**. **[static]**
- Allocates the bank object (0x298 bytes, ctor `FUN_00424830`), initializes the header
  buffer size to **0x1000**, links it into the engine's wave-bank list (append at tail —
  by-name binding picks the first match; relevant to the historical zombie-preview
  investigation), and calls `FUN_00424a70`:
  - copies the streaming params (handle at obj`+0x250`, packet size, offset),
  - allocates the 0x1000 header buffer and issues **one async read of 0x1000 bytes at
    offset 0** through `FUN_00426c80`. **[static]**
- `FUN_00426c80` (shared by header and data reads): computes the absolute file offset
  into the request's OVERLAPPED (base offset + stream cursor), fetches the stored
  readFile callback from engine`+0x190` and calls it; a FALSE return is tolerated only
  with `GetLastError() == ERROR_IO_PENDING (0x3E5)` — anything else is a failure
  (`0x8007xxxx`). **So genuinely-async deferral is native to the contract.** **[static]**
- Completion is **polled, never waited**: `FUN_004274ca` calls the getOverlappedResult
  callback (engine`+0x198`) with `bWait = 0`; only on TRUE does it dispatch the
  completion handler (header parse = bank vt`+0x110` `FUN_00424af0`; data-packet
  completion re-arms the next read). This is the **only** call site of the
  getOverlappedResult callback in the engine (byte-scan across all `call [reg+0x198]`
  encodings). **[static]**
- Header parse `FUN_00424af0`: checks magic `WBND`, version getter == `0x2A` (42), the
  first segment offset `< 0x7A0`, and requires the full pre-data region
  (`header[+0x2C]` = wave-data segment offset) to fit the buffer — if it exceeds 0x1000
  the buffer is reallocated (rounded up by `FUN_00426d40`) and re-read from offset 0.
  Stock DDR banks put wave data at 2048, so the single 0x1000 read always covers the
  metadata (and incidentally the first 2048 bytes of entry-0 data, which the parser
  ignores). After parse it resolves region pointers (BANKDATA / entry metadata / seek /
  names) and validates entry consistency (`FUN_0040f120`). **[static]**
- Data streaming (per prepared wave, `FUN_004265d0` = streaming-wave Prepare):
  the initial read is clamped (PCM: 64 KiB) and **rounded down to a whole multiple of
  the codec block-align**; thereafter the bank's packet reader (bank vt`+0xE8`
  `FUN_00424d70`) issues sequential packet reads — file offset = wave-data segment
  offset + entry data offset + stream cursor, one outstanding read per stream context.
  Loop-aware fields in the stream context bound reads to the loop end and restart the
  cursor at the loop start. **[static]**

### Read-pattern summary (what a virtual bank must serve)

| Read | Offset | Length | When | Thread |
|---|---|---|---|---|
| Header | 0 | 0x1000 (rare: grown to pre-data size, re-read from 0) | inside `CreateStreamingWaveBank` (i.e. inside `wavebank_create`, game thread) | game thread (issue); poll on engine pump |
| Data packets | seg4_offset + entry_offset + cursor (2048-aligned advance) | ≤ 64 KiB, block-align-rounded | cue Prepare and continuously thereafter, ~250 ms look-ahead | engine pump/notify threads |
| Loop restart | stretched loop-start offset | packet-sized | looped entries only | engine pump |

There is **no retail seek path** — BGM start is cue Prepare→IsPrepared→Play from offset 0;
the only backward jump is the loop start. **[cabinet + static]**

## 4. `wavebank_create` (+0x1AB050) and unregister (+0x1AB3D0), precise semantics

Create, in order **[static]**:
1. Duplicate guard: linear scan of the manager's bank-record list (`manager+0x68..`,
   0x20-stride records `{file_id, _, handle, bank*}`) — a live bank with the same
   file_id returns 0 (no create).
2. Slot classification `FUN_1801aa3c0(file_id)`: slots {0,3,5} stream; {1,2} in-memory.
3. Streaming path: `avs_fs_convert_path` (import `XCnbrep7000046`) on the file-table
   row's path (`[DAT_1806f2f48+0x28] + file_id*0xA0 + 0x11`) → `CreateFileA` →
   **slot-5 only**: insert `{handle → file_id}` into the sorted lookup vector (under the
   AVS mutex) → engine vt`+0x58` create (issues the header read **synchronously into our
   callback** before returning) → append the bank record → **`DoWork` (vt`+0x40`) is
   called immediately**, which can dispatch the header parse on the game thread, still
   inside `wavebank_create`.
4. In-memory path (slots 1,2): engine vt`+0x50` with the RAM buffer directly.

Unregister, in order **[static]**: engine bank Destroy (via the record's bank vtable) →
`CloseHandle` **synchronously** → remove the bank record → remove the `{handle→file_id}`
vector entry (under the AVS mutex). A destroyed bank cannot stream; the handle and lookup
entry outlive the Destroy call by microseconds only.

## 5. Facts that shape a virtual (synthesized) bank

- **No size cross-checks anywhere**: xactengine's only `GetFileSize` lives in the
  WAV-file `PrepareWave` path (`FUN_0042d0e0`, file-stream vtable `0x4052B0+0x38`), which
  the XWB streaming path never touches; gamemdx imports `GetFileSize` **zero** times.
  The engine believes whatever the header's segment table declares; the read callback's
  own EOF clamp is the only size authority. **[static]**
- Reads can therefore be served for a bank whose virtual size differs from the on-disk
  file backing its handle (bigger at slow rates, smaller at fast ones).
- The 0x1000 header read overlaps the first 2048 bytes of entry-0 data; those bytes are
  parser-ignored, but serving real (or zero) bytes there is harmless either way.
- The engine tolerates `FALSE + ERROR_IO_PENDING` at issue time and polls completion
  with `bWait=0` — a virtual bank can apply back-pressure without blocking any engine
  thread, **provided the getOverlappedResult callback is also intercepted for that
  handle** (the stock one always reports instant completion for vector-listed handles,
  which would convert a deferral into a spurious 0-byte completion).
- Short completions are normal at EOF (stock behavior: `min(len, size − offset)`).
- The preview player (`FUN_18010eab0`) creates slot-5 banks through the identical path
  at song select (cue `<code>_s`); any interception must be binding-gated, not
  path-gated. **[cabinet]**

## 6. Cross-version notes

- The engine DLL (`xactengine2_10.dll`) ships with the game and has been byte-identical
  across observed releases; its addresses in this note are stable until Konami ships a
  different engine build (guard: module presence/size check, as `game_audio` already
  does).
- gamemdx callback/manager addresses move every build. Resolution strategy (shipped as
  the `song_rate_io_callback_regsite` signature + `derive_song_rate_io_callbacks` in
  `src/core/signatures.rs`): one AOB on the constructor's callback-setup region (§2)
  yields both callback addresses by RIP-decode (LEA disp32s at match+21 / match+32);
  the handle→file_id lookup helper is decoded from the readFile body's first CALL
  (`E8 rel32` at entry+0x21, behind a 34-byte literal-prologue validation — the
  prologue is byte-identical across all four builds except the rel32 itself); the
  file-table global is RIP-decoded from the existing `song_rate_wavebank_unregister`
  match (`MOV RAX,[rip+disp32]` at match+15, disp32 at match+18 — the pattern's
  literal bytes already pin the `+0x28`/0xA0-stride/`+0x11` path-row access shape).
- **Handle→file_id mechanism decision (2026-08-10): call the stock lookup helper
  directly** (fastcall: HANDLE in RCX, returns file_id in EAX, −1 on miss; it takes
  the AVS mutex itself). This replicates the stock locked sorted-vector walk exactly,
  allocation-free, and avoids re-deriving the manager global, the handle-vector offset
  (manager`+0x20C8`), and the mutex gate. Documented cost: an *unbound* read pays the
  lookup twice (once in the detour to classify, once inside the trampolined original)
  — a cost class the stock path already pays on every read (§7).
- **Cross-version verification (Ghidra, 2026-08-10, all four supported builds).** The
  regsite AOB (`C7 45 ?? FA 00 00 00` + 3× `LEA/MOV` with disp32/disp8 wildcards)
  matches exactly once per build; all decoded addresses below verified as real
  function entries / data globals. The 20260721 callbacks equal §2's
  `FUN_1801aa250`/`FUN_1801aa350`; the file-table decode reproduces `DAT_1806f2f48`
  (20260721) exactly.

  | Build | regsite match | readFile cb | getOverlapped cb | handle lookup | unregister match | file-table global |
  |---|---|---|---|---|---|---|
  | 2026-03-24 | `0x1801a81a4` | `0x1801a76c0` | `0x1801a77c0` | `0x1801a8f10` | `0x1801a8870` | `0x1806ebec8` |
  | 2026-04-21 | `0x1801a8e74` | `0x1801a8390` | `0x1801a8490` | `0x1801a9bb0` | `0x1801a9510` | `0x1806ee068` |
  | 2026-06-16 | `0x1801a9ca4` | `0x1801a91c0` | `0x1801a92c0` | `0x1801aaa10` | `0x1801aa370` | `0x1806f1f50` |
  | 2026-07-21 | `0x1801aad34` | `0x1801aa250` | `0x1801aa350` | `0x1801aba70` | `0x1801ab3d0` | `0x1806f2f48` |

  Note the 40 regsite match bytes are byte-identical across builds (same disp32s —
  the callbacks sit at fixed offsets relative to the registration site), as are the
  readFile prologues; the derivation still wildcards every displacement so a future
  layout shift breaks the match (fail-closed) instead of mis-resolving.
- File-table struct layout (readFile disasm, 20260721): the global holds a pointer;
  at use `obj = *(global)`; data rows base `*(obj+0x8)`, row stride 0x40, buffer ptr
  at row+0x8, size u32 at row+0x14; path rows base `*(obj+0x28)`, row stride 0xA0,
  path string at row+0x11 (MSVC std::string SSO flag byte at row+0x8F).
- The manager global (`DAT_1806f2d60` on 20260721, decodable from the unregister
  match at +25) is NOT published — unnecessary under the helper-call mechanism.

## 7. Gotchas

- `OVERLAPPED.Internal` is repurposed as a completion accumulator by gamemdx's
  callbacks; any interception must preserve that protocol exactly for pass-through
  files (best: call the original callback for everything unbound).
- The readFile callback runs on **both** the game thread (header read, nested inside
  `wavebank_create`) and engine pump threads (packet reads) — interception must be
  thread-agnostic, allocation-free, and log-free.
- The stock callback performs the locked vector lookup **on every read**; an
  interception that replicates the lookup adds no cost class the stock path doesn't
  already pay.
- gamemdx's clamp arithmetic (`size − offset` unsigned) assumes offsets stay within the
  declared regions; the engine guarantees this for well-formed headers. A virtual header
  must keep segment declarations self-consistent or inherit the same wraparound hazard.
- The engine pairs sound↔wave banks **globally by internal name** (see
  `docs/xact_audio_research.md` §1 amendment); a virtual bank must keep the stock
  internal name to bind with the stock XSB — which is exactly what serving a
  stock-shaped header achieves.

## 8. Implementation-time findings (2026-08-10/11, live bring-up)

Recorded during plan Step 5's four cabinet deploys; the working records live under
`.agents/planning/2026-08-08-song-rate-streaming/implementation/` (notably
`step05-fix-preview-side-buffer/progress.md`).

- **WSOLA throughput at the game's sample rate is only ~2–6× realtime under
  CrossOver** (~114 k frames/s ≈ 2.4× observed at 47 kHz on the cabinet). The
  earlier "21× realtime" reading divided total frames by a stall that only ever
  covered the preview entry. Consequence: stretching content the player may never
  hear is unaffordable during loading.
- **Preview passthrough (maintainer-approved deviation from the design's "both
  entries stretched").** The engine's bank prepare primes a stream context for
  EVERY wave in the bank — including the never-played `<code>_s` preview at the
  virtual file's tail — so a linear producer had to synthesize the whole main
  entry plus the preview before gameplay's first packet was serveable (23–25 s of
  loading at 25 %). Shipped model: the non-main entry keeps its STOCK header
  values and is served **verbatim** from the resident source copy (an append-only
  `side_buffer` produced first), while the ring covers only the main entry's
  range and regeneration targets are main-only. Song-select previews therefore
  play at normal speed. Loading at 25 %/175 % is ~5 s (normal); a full 8.5-minute
  25 % song played with **0 deferrals**.
- **Emission follows the PARSER's layout rule, not a stricter one.** The stream
  serializer originally required whole-block durations for everything it wrote;
  the verbatim stock preview has its duration inside its final block (as real
  banks do), so every bind refused `HeaderSynth` — invisible on host because
  every fixture was block-exact ("too clean"). `xwb`'s stream-layout validation
  now delegates to the codec's own layout rule (`ceil(duration/128)` == block
  count), and the fixtures were made honest first (durations NOT block-exact) so
  the refusal reproduced on host before the fix. Lesson recorded: never build
  fixtures cleaner than the real data.
