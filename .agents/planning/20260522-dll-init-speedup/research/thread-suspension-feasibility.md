# Thread-Suspension Feasibility for DLL Init Speedup

> Compiled 2026-05-22 to inform the design phase of
> 20260522-dll-init-speedup. The motivating use case is the
> musicdb.xml race documented in `time-critical-hooks.md`: with
> > ~2000 songs the game crashes on bootup ~75% of the time because
> our 6-byte buffer-size patch lands variably 100-250 ms into init,
> overlapping the window in which the game's `master_loader`
> reaches `musicdb_parser`.
>
> The question this file answers: **can we eliminate that race by
> suspending the game's threads while we install the patches?**

---

## 1. Win32 Thread Suspension Mechanics

### 1.1 The API surface

Three relevant entry points:

- **`SuspendThread(HANDLE hThread)`** — increments a per-thread
  suspend count; thread stops executing user-mode code when the
  count is non-zero. Returns previous count, or `(DWORD)-1` on
  failure. Source: MSDN
  ([learn.microsoft.com/.../suspendthread](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-suspendthread)).
- **`Wow64SuspendThread(HANDLE hThread)`** — same, for 64-bit
  callers suspending 32-bit (WoW64) threads. Not relevant for us:
  DDR World is 64-bit.
- **`NtSuspendProcess` / `NtResumeProcess`** (NTDLL, undocumented
  but stable since XP) — suspends/resumes every thread in a
  process atomically. Convenient because we don't have to
  enumerate threads, but the caller's own thread is also
  suspended unless we filter ourselves out — which means in
  practice we still have to enumerate threads if the call is
  against our own process.

Thread enumeration uses either:

- Toolhelp32: `CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)`
  + `Thread32First/Next` — well-known, but races (a thread can
  appear or disappear during iteration).
- `NtGetNextThread` (NTDLL) — newer, walks the kernel's thread
  list with a stable cursor.

For our purposes (a one-shot freeze right after gamemdx.dll
detection), Toolhelp32 is fine. The race window is bounded
because the game has only just begun loading; thread count is
relatively small and changing slowly.

### 1.2 Suspension is asynchronous

The MSDN page ([SuspendThread, Remarks
section](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-suspendthread))
states verbatim:

> "This function is primarily designed for use by debuggers. It
> is not intended to be used for thread synchronization. Calling
> **SuspendThread** on a thread that owns a synchronization
> object, such as a mutex or critical section, can lead to a
> deadlock if the calling thread tries to obtain a synchronization
> object owned by a suspended thread. To avoid this situation, a
> thread within an application that is not a debugger should
> signal the other thread to suspend itself. The target thread
> must be designed to watch for this signal and respond
> appropriately."

The salient facts:

- **Asynchronous w.r.t. instruction stream.** A thread may be
  suspended at any user-mode instruction boundary, including in
  the middle of a critical-section acquire/release sequence,
  during `RtlAllocateHeap`, while inside `LoadLibraryEx`, or in
  the user-mode portion of a `Read/WriteFile` syscall.
- **Can hold any number of locks.** SuspendThread does not
  consult what locks the target thread holds. If the suspended
  thread owns the loader lock, the heap lock, or any application
  CRITICAL_SECTION / SRW lock, no other thread can acquire those
  locks until we resume.
- **Microsoft explicitly says: don't use this for
  synchronization.** Hook-installation is closer to synchronization
  than debugging, but several mature production frameworks
  (Detours, MinHook, SafetyHook, EasyHook) do use it anyway,
  having concluded that the deadlock surface is manageable for
  short, carefully-bounded operations. We're in that camp if we
  proceed.

### 1.3 Locks the suspended thread might hold

| Lock | Scope | Who takes it | Risk to us |
|------|-------|--------------|------------|
| **Loader lock** | Per-process | `LoadLibrary`, `FreeLibrary`, `GetModuleHandleEx`, DLL initializers, some `GetProcAddress` paths | High during early boot — sibling DLLs may still be loading |
| **Process heap CS** | Per-heap (default heap is process-wide) | `HeapAlloc`/`malloc`/`new`, including all CRT alloc paths | High — almost any non-trivial work allocates |
| **AGCS app heap** | Custom Konami allocator | `agcs_heap_malloc`/`free` | Low |
| **`OutputDebugStringA` global SRW** | Per-process | Any thread logging | Medium — we log a lot |
| **CRT FILE locks** | Per-FILE | `fopen`/`printf`/`stdio` | Low — we don't use stdio |
| **Application CRITICAL_SECTIONs** | Per-CS | Konami code | Unknowable from outside; assume any could be held |

The high-risk ones are the loader lock, the process heap CS, and
the `OutputDebugStringA` lock. Section 3 walks through how to
avoid each.

### 1.4 Mid-syscall suspension

If a suspended thread is in the kernel-mode portion of a syscall,
suspension waits until it returns to user mode, but kernel-held
locks are orthogonal — they belong to the kernel, not our process,
and release when the syscall completes. Not a practical deadlock
risk; the user-mode lock discussion in §1.3 dominates.

---

## 2. Concrete deadlock scenarios

### 2.1 Loader-lock deadlock

```
Game thread T1: LoadLibrary("libafp-win64.dll")
                  -> takes loader lock
                  -> begins DLL init
Init thread (us): SuspendThread(T1)              [T1 suspended holding loader lock]
                  GetModuleHandleA("libavs-win64.dll")
                  -> takes loader lock           [BLOCKED FOREVER]
```

Any kernel32/ntdll API that touches the module list takes the
loader lock. `GetModuleHandleA` does. So does `GetProcAddress`.
So does `OutputDebugStringA` in some Windows versions.

**Implication:** While threads are suspended, we MUST NOT call
`LoadLibrary`, `FreeLibrary`, `GetModuleHandle*`, or
`GetProcAddress`, AND we must avoid logging via
`OutputDebugStringA`.

### 2.2 Heap-lock deadlock

```
Game thread T2: malloc(1024)
                  -> RtlAllocateHeap
                  -> takes process heap CS
Init thread (us): SuspendThread(T2)              [T2 suspended holding heap CS]
                  log_info!("freeze starting")
                  -> formatting allocates
                  -> RtlAllocateHeap
                  -> takes process heap CS       [BLOCKED FOREVER]
```

Rust's `String` and `format_args!` machinery hits the global
allocator. Even our logging macros (`log_info!`) format a string
before passing it to `OutputDebugStringA`.

**Implication:** No allocation under freeze. Pre-format any log
strings before suspending threads, or don't log at all from inside
the freeze region.

### 2.3 OutputDebugStringA serialization

`OutputDebugStringA` takes a process-wide lock to serialize debug
output (historically uses the `DBWinMutex`). If a suspended
thread is mid-`OutputDebugStringA`, our log call will block on
the same primitive.

**Implication:** Treat `log_info!` etc. as unsafe inside the
freeze region. Buffer log lines, emit after resume.

### 2.4 SRW locks held in critical OS components

Modern Windows uses SRW locks heavily in CRT, NTDLL, and various
shared services. Same pattern: if a suspended thread owns one,
we can't acquire it.

**Implication:** Same as above — minimize Windows API surface
inside the freeze region.

---

## 3. What our init thread does that could hit a suspended-thread lock

| Call | Lock(s) taken | Safe under freeze? |
|------|---------------|---------------------|
| `VirtualProtect(addr, len, PAGE_EXECUTE_READWRITE, &old)` | None process-wide; per-VAD spinlock at most, kernel-only | Yes — see §5 |
| Single-byte writes via `*ptr = 0x80` | None (volatile store on x86) | Yes — see §5 |
| `VirtualProtect(addr, len, old, &dummy)` (restore) | Same as above | Yes |
| `FlushInstructionCache(GetCurrentProcess(), addr, len)` | None process-wide | Yes |
| `OutputDebugStringA` (logging) | Process-wide DBWin serialization | **No — pre-format and emit after resume** |
| `GetModuleHandleA` / `GetProcAddress` | Loader lock | **No** |
| `HeapAlloc` / `malloc` / `Box::new` / `String::from(...)` / `format!` | Process heap CS | **No** |
| `SuspendThread` / `ResumeThread` (the syscalls themselves) | Per-process thread-list lock; brief, system-managed | Yes |
| Toolhelp32 snapshot / `Thread32First/Next` | Brief snapshot lock | Yes (called BEFORE the freeze) |

Recommended **freeze-region allowlist**:

1. Already-resolved raw pointers + plain pointer arithmetic.
2. `VirtualProtect`, `FlushInstructionCache`, `GetCurrentProcess`,
   `GetCurrentThreadId`.
3. Single-byte / multi-byte stores to known addresses.
4. `SuspendThread` / `ResumeThread` themselves.
5. Reads from already-mapped module memory.

Recommended **freeze-region denylist**:

1. Any allocation (`Box`, `Vec`, `String`, `format!`, etc.).
2. Any logging (`log_info!`, `eprintln!`, `OutputDebugStringA`).
3. Any module-list-touching API (`GetModuleHandle*`,
   `LoadLibrary*`, `GetProcAddress`).
4. Any `std::sync::Mutex` / `RwLock` (or any locks the game
   might also take).
5. `panic!` and anything that could panic — the panic handler
   can allocate, log, and unwind, all of which violate the
   denylist.

This is a real but tractable discipline. The freeze code should
look like a deliberately tight inner loop with all arguments
captured by value before the suspend call.

---

## 4. Prior art: how production frameworks handle this

### 4.1 Detours (Microsoft)

`DetourTransactionBegin` / `DetourAttach` / `DetourDetach` /
`DetourTransactionCommit`. The wiki page
[microsoft/Detours: DetourUpdateThread](https://github.com/microsoft/Detours/wiki/DetourUpdateThread)
documents the thread-handling step:

> "**Purpose**: This function registers a thread to be updated as
> part of an active detour transaction. ... Enlist a thread for
> update in the current transaction."
>
> "**How it works:** The thread is enlisted for updating when the
> transaction (previously started by `DetourTransactionBegin`) is
> committed. At commit time, Detours ensures that any enlisted
> thread whose instruction pointer falls inside rewritten code —
> either in the original target or the trampoline — gets adjusted
> accordingly."
>
> "**Important caveats:** Threads that aren't enlisted will not
> be fixed up at commit, so they risk running 'an illegal
> combination of old and new code.' Passing a real (non-pseudo)
> handle that refers to the current thread is not supported and
> can cause the application to hang."

Detours' model: caller enumerates threads and registers each with
`DetourUpdateThread`. The commit then suspends each enlisted
thread, applies queued attaches/detaches, rewrites IPs of
threads that landed inside a patched region, and resumes.
**Atomic batch by design.** Our codebase doesn't use Detours
(it uses `retour`), but the model informs the design.

### 4.2 MinHook (TsudaKageyu)

[MinHook README](https://github.com/TsudaKageyu/minhook), v1.2
release notes:

> "Every call to `MH_EnableHook` or `MH_DisableHook` suspends and
> resumes all threads."

And on the queue-and-apply API:

> "Functions to enable or disable multiple hooks in one go ...
> the preferred way of handling multiple hooks. Instead of
> toggling hooks one at a time (each triggering a thread
> suspend/resume cycle), you queue up several enable/disable
> requests and then commit them together with a single apply
> call — batching the thread suspension into one operation."

MinHook proves thread suspension during hook install is the
**default, mainstream** approach in the C++ inline-hooking world.
v1.3.4 (2025-03-28) explicitly mentions "Improved error handling
for enumerating and suspending threads" — they're still
investing here.

### 4.3 SafetyHook (cursey)

The [SafetyHook README](https://github.com/cursey/safetyhook)
states the library:

> "Stops all other threads when creating or deleting hooks"
>
> "Fixes the IP of threads that may be affected by the creation
> or deletion of hooks"
>
> "Fixes IP relative displacements of relocated instructions"
> "Fixes relative offsets of relocated instructions"
> "Widens short branches into near branches"
> "Handles short branches that land within the trampoline"

SafetyHook is the modern, Rust-bindable C++ library specifically
designed around safety. Same conclusion as MinHook and Detours:
suspend threads, fix IPs, resume. The IP-fix step is the part our
`retour` doesn't do — but for our specific case (byte writes to
immediate operands, not function-prologue overwrites) it doesn't
matter; see §5.

### 4.4 EasyHook (community)

EasyHook follows the same pattern. The library walks suspended
threads' contexts and rewrites RIP when it falls inside a hooked
region.

### 4.5 Frida

Frida's gum-stalker / interceptor uses a fundamentally different
model — relocates entire basic blocks rather than patching at
function entry, exception-based mechanism on some platforms.
Less directly comparable.

### 4.6 The retour crate (what we use today)

`retour` (with the static-detour feature, nightly Rust) does
**not** do thread suspension or IP fixing. Each
`GenericDetour::install()` writes a 5-byte rel32 JMP at the
target's prologue using `VirtualProtect` to flip page perms.
There's a window — measured in single-digit microseconds — where
another thread executing in those 5 bytes could see a partially
written instruction. In practice this is a 5-byte write at a
function prologue, often not currently executing because it's
the first instruction of the function (any thread "in" the
function is past the prologue), and on x86 single-byte stores
are atomic.

The community accepts this risk for non-critical hooks. For our
musicdb case we don't even hit this risk — we're patching
**immediate operands**, not instruction prefixes (see §5.3).

### 4.7 Summary

- World-freeze around hook installation is the **standard**
  approach in production C++ inline-hooking libraries.
- The MSDN warning is taken seriously but not as a
  prohibition — it's a directive to be careful with what runs
  inside the freeze region.
- The risk that the suspended thread is mid-execution at a
  patched address is real for inline detours (mitigated by
  IP-rewriting in Detours/SafetyHook/EasyHook). It is **not** a
  risk for our 6-byte musicdb patches; see §5.

---

## 5. VirtualProtect and atomic byte-write details for our case

### 5.1 Does `VirtualProtect` take a process-wide lock?

`VirtualProtect` operates on a per-VAD (Virtual Address
Descriptor) basis. The kernel takes a brief lock on the AVL tree
of VADs for the process to update permissions, but this is held
only inside the syscall and released before it returns. It is
**not** held across user-mode execution. A thread we suspended
cannot be holding the VAD-tree lock in a way that blocks our
`VirtualProtect`, because that lock is kernel-internal and
short-lived.

### 5.2 Is the write atomic?

We're writing a single byte (the high byte of an immediate
operand) at each of 6 addresses. On x86-64:

- All aligned 1/2/4/8-byte stores are atomic (Intel SDM Vol. 3A,
  §8.1.1).
- Single-byte stores are unconditionally atomic regardless of
  alignment.

No torn writes. A thread reads either `0x10` or `0x80`; never an
intermediate state.

### 5.3 What if a game thread is executing at the patch address right now?

The patched bytes are the high byte of a 32-bit immediate operand
inside a `MOV r32, imm32` (the `ALLOC_PATTERN` ends
`BA 00 00 10 00`, where the 4-byte little-endian immediate is
`00 00 10 00` = 0x100000) or inside a `MOV [RSP+0x20], imm32`
(the `READ_PATTERN`).

If a thread is executing **the instruction containing the byte
we patch** at the exact moment of our store:

- Per Intel SDM Vol. 3A §11.6 (Self-Modifying Code), the CPU
  detects the modification and flushes the pipeline — provided
  we issue a serializing instruction (or `FlushInstructionCache`
  generates the necessary IPIs) before the next time that code
  runs.
- Without `FlushInstructionCache`, the *current* execution sees
  whichever value was already decoded; subsequent executions
  see the new value once the i-cache is invalidated.

For our case:

- The `master_loader` -> `musicdb_parser` path runs **once** per
  boot. If our patch lands BEFORE the parser is reached, all 6
  sites use the new value.
- Even without thread suspension, if our patch lands DURING the
  parser's execution of one MOV, the result for that one
  instance is undefined; subsequent reads of the other addresses
  see the new value.

Thread suspension **prevents** any thread from being mid-
execution at any of the 6 sites during our writes. Combined with
`FlushInstructionCache` after the writes, it gives us atomic,
race-free 6-byte updates.

### 5.4 Summary

For our specific musicdb-patch use case:

- `VirtualProtect` and the byte writes themselves are safe under
  freeze.
- The patches are immediate-operand bytes, not instruction
  prefixes, so even an unsuspended thread mid-execution at the
  exact patch site has well-defined CPU behavior.
- Thread suspension gives us deterministic ordering: every
  subsequent instruction fetch at the 6 sites sees the new byte.

The "IP rewriting" complications from §4 (Detours / SafetyHook)
do not apply.

---

## 6. The "surgical freeze" design

### 6.1 Sequence

```
Init thread:
  poll for gamemdx.dll                                 (existing)
  resolve signatures (incl. SongLimitExpansionMod sites) (existing or new fast path)
  precompute the 6 (addr, expected=0x10, new=0x80) tuples
  precompute log strings ("freeze starting...", "freeze done...")
  // ENTER FREEZE
  enumerate threads via Toolhelp32, excluding our TID
  for each: OpenThread(THREAD_SUSPEND_RESUME) + SuspendThread
  for each of 6 sites:
    VirtualProtect(addr, 1, PAGE_EXECUTE_READWRITE, &old)
    if (*addr != 0x10) {
      // unwind: restore page perms, resume threads, log error
    }
    *addr = 0x80
    VirtualProtect(addr, 1, old, &dummy)
  FlushInstructionCache(GetCurrentProcess(), 0, 0)
  for each thread handle: ResumeThread + CloseHandle
  // EXIT FREEZE
  emit pre-formatted log lines
  continue normal init
```

### 6.2 Estimated freeze duration

- Thread enumeration: ~10-50 us.
- Per-thread `OpenThread + SuspendThread`: ~5-10 us each.
  At ~10-20 threads (typical early boot): ~100-200 us total.
- 6 x `VirtualProtect + byte write + VirtualProtect`: ~30 us.
- `FlushInstructionCache`: ~10-100 us (broadcasts IPIs).
- Per-thread `ResumeThread + CloseHandle`: ~5 us each: ~100 us.

**Total freeze window: ~250-500 us.** Under 1 ms. Not audibly
or visually perceptible.

### 6.3 Compared to a "broader freeze"

A naive "freeze through scan + install everything" approach
would last ~50-100 ms. At 60 fps, that's 3-6 dropped frames;
audibly a click. More importantly, every API the
scanner+installer touches becomes a freeze-region constraint.
The surgical approach contains the discipline tax to ~30 lines.

### 6.4 The pre-freeze ordering requirement

The surgical approach **already requires us to know the 6 patch
addresses before we suspend.** That means we already ran the
scan. So thread-suspension is layered on top of, not in place
of, the centralized fast scanner from Strategy A.

**Strategy A and Strategy B compose:**

- Strategy A makes the *scan* fast.
- Strategy B makes the *write* race-free.

Both contribute. Strategy B alone, with today's slow scan, would
still race because the scan itself takes 50-100 ms during which
the parser may run.

---

## 7. The "broader freeze" design (alternative, not recommended)

```
Init thread:
  poll for gamemdx.dll
  // ENTER FREEZE
  suspend all threads
  resolve all signatures
  install all hooks via retour
  apply all byte patches
  // EXIT FREEZE
  resume all threads
```

Why not:

- Freeze is ~50-100 ms (the scan time itself). Audibly noticeable
  as a click; possibly causes the game audio engine to underrun.
- `retour::GenericDetour::install` allocates internally (it
  builds a trampoline). Any allocation under freeze risks the
  heap-lock deadlock from §2.2 — unsafe.
- `retour` does not do IP fixing, so we'd lose the safety property
  Detours/SafetyHook give us in exchange for the freeze.
- Doesn't solve sibling-DLL load timing: `libafp-win64`, etc.
  may not be loaded yet when gamemdx.dll appears; they have to
  be hooked in a separate later phase anyway.

**Conclusion: do not pursue the broader freeze.**

---

## 8. spice2x and DDR-specific concerns

I could not find authoritative public documentation that spice2x
itself uses thread-suspension, anti-debug detection, or
anti-hooking tripwires that would react to our freeze. Public
spice2x source is partial and the documentation around the `-k`
hook flag is sparse on this point. **This is an unverified
finding** — recommended action is one of:

1. Test empirically: deploy a build that does a no-op freeze
   (suspend, sleep 100 us, resume) and see if spice2x or the
   game complain.
2. Ask in the spice2x community / Discord whether thread
   suspension by hook DLLs is known to interact with anything
   spice2x does.

The 250-500 us duration of the surgical freeze (per §6.2) is
short enough that even if spice2x does sample thread state,
it's unlikely to catch us in the act. But this is a known
unknown for the design phase — flag for confirmation.

For DDR / Konami specifically, the game is not known to ship
with anti-cheat that monitors thread state. The arcade build
trusts its execution environment; modding has been ongoing for
years without reports of "the game detected my hook DLL."

---

## 9. Module-load sequencing

We hook 5 modules: `gamemdx.dll`, `libafp-win64`,
`libafputils-win64`, `libavs-win64`, `arkmdxbio2`. They load at
different times during boot; gamemdx.dll appears to be among the
last because our existing polling waits for it as the gating
event. The siblings are typically loaded earlier (they're
dependencies of gamemdx itself).

**Implication for surgical freeze:** the only module relevant
to the musicdb race is gamemdx.dll. The 6 patch sites are all
in gamemdx.dll. We don't need to wait for sibling DLLs before
applying the surgical freeze.

**Implication for non-musicdb hooks:** these can be installed
*after* the freeze, normally, with no special ordering. Most of
them are late-binding-tolerant (see `time-critical-hooks.md`).
If any future hook turns out to be race-sensitive, we can give
it its own surgical freeze; the pattern composes.

---

## 10. Recommendation

**Do this:**

1. Build the centralized fast scanner (Strategy A) regardless.
   It accelerates init for all hooks and is independently
   valuable.
2. Add a surgical thread-suspension wrapper specifically around
   the SongLimitExpansionMod 6-byte patch application.
   - Discover the 6 addresses with the fast scanner first (no
     freeze).
   - Suspend all non-init threads, apply the 6 byte writes with
     `VirtualProtect`, `FlushInstructionCache`, resume.
   - Pre-format any log strings; use no allocation, no logging,
     no module-API calls inside the freeze region.
3. Do NOT broaden the freeze beyond the 6 byte writes. The
   discipline tax (no allocation, no logging, no API surface)
   is acceptable for ~30 lines but not for the full init phase.

**Risks to flag for the design phase:**

- The freeze region must be *audited* — every API call inside it
  has to be cross-checked against the denylist. A future
  refactor that adds logging or allocation inside the freeze
  silently re-introduces deadlock risk. We need a comment block
  and possibly a small abstraction (e.g., a `FreezeGuard`
  RAII type whose body is the freeze-safe operations) to keep
  this enforceable.
- The Toolhelp32 thread snapshot races with thread creation. If
  a new game thread spawns *after* our snapshot but *before* our
  writes, it isn't suspended. For our case this is benign:
  a new thread can't be executing musicdb_parser without going
  through master_loader, which has already started; and our
  writes are atomic single-byte stores. Worth noting but not
  a blocker.
- Preserve the "verify byte was 0x10 before writing 0x80" sanity
  check inside the freeze. If verification fails, abort the patch
  (don't write), resume threads, log the error. This catches
  signature drift across game updates.
- `FlushInstructionCache(GetCurrentProcess(), 0, 0)` is the
  documented "flush everything" form. Verify behavior on the
  target Windows version.
- The MSDN warning ("not intended for synchronization") is a
  warning, not a prohibition. Production frameworks (Detours,
  MinHook, SafetyHook, EasyHook) all do exactly this. We are
  in well-trodden territory.

**Open questions for profiling to answer:**

- What is the actual current init time for the SongLimitExpansionMod
  patches? (i.e., from gamemdx.dll-detected to the 6 bytes
  written.) The 75% crash rate gives us a lower bound on the
  game's boot-to-parser timing, but the profiling diff in
  `scan-bottleneck-analysis.md` will give us the actual time.
- Is the LayeredFS init or widget renderer init blocking us
  unnecessarily before SongLimitExpansionMod can run? The mod
  registration happens AFTER all services init today; if we can
  reorder so SongLimitExpansionMod is the very first thing after
  `resolve_all` completes, we cut another 30-100 ms off the
  race window.

---

## 11. Sources

- MSDN, **SuspendThread function** (Win32):
  <https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-suspendthread>
- Microsoft Detours, **DetourUpdateThread** wiki:
  <https://github.com/microsoft/Detours/wiki/DetourUpdateThread>
- TsudaKageyu, **MinHook** README + release notes:
  <https://github.com/TsudaKageyu/minhook>
- cursey, **SafetyHook** README:
  <https://github.com/cursey/safetyhook>
- Intel Software Developer's Manual, Volume 3A, §8.1.1
  (atomicity of aligned/byte stores) and §11.6 (self-modifying
  code semantics). Canonical reference: intel.com/sdm.
- DanceDanceRevolution World hook DLL,
  `docs/omnimix_song_limit_research.md` for the musicdb patch
  details.
- DanceDanceRevolution World hook DLL,
  `.agents/planning/20260522-dll-init-speedup/research/time-critical-hooks.md`
  for the race timeline.
- DanceDanceRevolution World hook DLL,
  `.agents/planning/20260522-dll-init-speedup/research/scan-bottleneck-analysis.md`
  for current init timing estimates.

Couldn't independently verify:
- spice2x's internal thread-monitoring or anti-debug behavior
  (no authoritative public source found).
- DDR World specifically reacting to thread suspension at the
  game level (no reports found in modding communities).
