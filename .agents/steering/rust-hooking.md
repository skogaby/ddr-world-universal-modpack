# Rust Hooking Patterns

Safety rules and patterns for runtime game hooking in this project.

## Memory Allocation

### Game-Owned Objects
Any memory that the game will eventually `free()` **must** be allocated using the same allocator the game will use to free it. The game runs **two distinct heap systems** that both need to be matched on a case-by-case basis:

1. **CRT heap** — MSVC `operator new` / `malloc`, resolved via the `game_malloc` signature. Used for game objects allocated by `new FooProperty(...)` and freed via `delete` or shared_ptr. Uses `HeapAlloc` / `HeapFree` on a private CRT heap.
2. **App heap (agcs allocator)** — AGCS's own heap manager. Used for anything allocated through `agcs::stl::vector<T>` (the engine's allocator-aware STL containers like the per-chart `Notes`, `Measures`, and `Results` vectors), and for objects created via `AGCS_CLASS_NEW` / `AGCS_APP_NEW`. Allocations go through `agcs_heap_malloc(heap_handle, size, align, 0)` — entry point resolved via signature — and freed through `agcs_heap_free(ptr)`, which reads a tracking header at `ptr-0x18 / ptr-0x20` to locate the originating heap.

Allocating with the wrong heap, or with `VirtualAlloc` / Rust's global allocator, causes a heap-mismatch crash when the object's destructor runs — `RtlFreeHeap` for CRT-freed objects, or AGCS's free path walking a mismatched free-list for allocator-aware containers.

- `game_malloc` is resolved via AOB scan (MSVC `operator new` — identical bytes across game versions).
- After `game_malloc`, zero the buffer with `std::ptr::write_bytes(ptr, 0, size)` before calling the constructor.
- `agcs_heap_malloc` + `agcs_heap_free` + `app_heap_handle` are resolved as a triple. `app_heap_handle` is a global `*const *const u8` (address held in `DAT_180460058` in the current build) — dereference once to get the opaque `Heap*` the allocator expects as its first argument. The handle address is derived via RIP-relative decode from a known `std::vector<T>::reserve` landmark (`app_heap_reserve_anchor` signature), which keeps the derivation stable across game updates even though the absolute address of the global varies.
- Use `memory::alloc_zeroed()` (VirtualAlloc) only for **temporary buffers** that the game never frees: functor staging buffers, shared_ptr staging buffers, hook trampolines.

### When to Use Which Allocator
| Allocator | Use For |
|-----------|---------|
| `game_malloc` (CRT) | Objects the game takes ownership of via `new`/`delete` or shared_ptr (FolderProperty, etc.) |
| `agcs_heap_malloc` (app heap) | Growing allocator-aware STL containers the game owns (`agcs::stl::vector<T>` — the per-chart Notes vector is the canonical example) |
| `memory::alloc_zeroed` (VirtualAlloc) | Temporary buffers, code caves, hook trampolines |
| Rust `Vec`/`Box`/`String` | Mod-internal data that never crosses the FFI boundary |

## Hook Callbacks

### Static State
Hook callbacks are `extern "C"` functions — they can't capture state. Use `static mut` variables (accessed via `std::ptr::addr_of!`) to pass function pointers and configuration to hooks. Gate all static access on null/None checks.

### Re-entrancy
`folder_register` is called once per folder during `folder_init`. The hook fires for every folder, not just the one we care about. Use guard flags (e.g., `CUSTOM_FOLDERS_CREATED`) to ensure one-shot logic runs exactly once per `folder_init` pass, and reset the flag when a new pass is detected.

### Calling Convention
All game functions use the Microsoft x64 calling convention (`extern "C"` in Rust):
- First 4 integer/pointer args: RCX, RDX, R8, R9
- Return value: RAX
- Caller-saved: RAX, RCX, RDX, R8, R9, R10, R11
- Callee-saved: RBX, RBP, RDI, RSI, R12-R15

### Multiple Hooks on the Same Target Function
Do not install two independent `retour::GenericDetour` handles on the same target function. Retour's trampoline mechanism does not compose when the second detour is installed on top of the first — the second detour's "call original" path captures the first detour's jmp as the original, so the first detour's callback is silently bypassed depending on install order.

When multiple mods need to intercept the same function, build (or extend) a shared dispatcher service that installs exactly one detour and lets each subscriber register pre/post callbacks with a priority. `services::judge_hook` is the canonical example: one detour on `GamePlayActor::judgeNotes`, subscribers register `fn(actor, music_count)` callbacks at `Priority::Early | Normal | Late`, dispatcher runs them in order around the original call. `register_pre` / `register_post` return a `CallbackHandle` that subscribers stash and pass back to `unregister` from their `disable()` path — same shape as `scene_manager::on_scene_change` → `remove_callback`.

## Shared Ptr Interactions

The game uses MSVC `std::shared_ptr` extensively. Key patterns:
- Control block layout: `[+0x00] vtable, [+0x08] strong_refcount, [+0x0C] weak_refcount, [+0x10] managed_object_ptr`
- `shared_ptr` layout: `[+0x00] object_ptr, [+0x08] control_block_ptr`
- When the strong refcount hits 0, the control block's `vtable[0]` is called (destructor for the managed object), then when weak refcount hits 0, `vtable[1]` is called (frees the control block).
- The control block destructor typically calls the game's `free()` on the managed object — this is why allocator matching matters.

## Version-Agnostic Patterns

- Detect struct layouts at runtime from constructor/init function disassembly, not hardcoded offsets.
- Store detected layouts in structs (e.g., `FolderPropertyLayout`) and pass them to hooks via statics.
- When field semantics differ between versions (e.g., enable flags vs restriction flags), detect the semantics from the constructor's default values and store the unlock value alongside the offset.

## Instruction Decoding

Prefer `core/scanner.rs` primitives over inline decode loops. They exist specifically to keep RE code readable and to avoid diverging implementations of the same decode logic.

| Need | Use |
|------|-----|
| Follow a `CALL rel32` (opcode `0xE8`) to its target | `decode_call_rel32(call_addr)` |
| Follow any RIP-relative disp32 (LEA, MOV `[RIP+disp]`, `CALL [RIP+disp]`, JMP table entries, etc.) | `decode_rip_relative(disp_addr)` — takes a pointer to the 4 displacement bytes |
| Find the first `CALL` in a function prologue | `scan_first_call_rel32(start, len)` |
| Find every xref (CALL `E8`) to a specific target across a module | `scan_xrefs_to(base, size, target)` |

Do not re-implement these inline. If you find yourself writing `(p as *const i32).read_unaligned()` followed by `.offset(disp as isize)`, use `decode_rip_relative` instead.

If you need a new decode primitive (e.g., `decode_jmp_rel32`, RIP-relative across multiple ModRM forms), add it to `core/scanner.rs` once there are at least two call sites that would use it. Single-site decoders belong inline.

## Design Rationale

A few non-obvious "why" decisions behind the patterns above, kept here so they aren't re-litigated:

- **Why `retour` (not minhook-rs or iced-x86)?** `retour` provides a safe Rust API over inline hooking with `GenericDetour<T>`, and its `static-detour` feature lets detours live in `static` variables — necessary for callbacks that need to call the original function. The `0.4.0-alpha` line is required for x86_64 support, which is also why the crate pins nightly Rust (its trampoline generation uses unstable intrinsics).

- **Why `static mut` for hook state instead of `thread_local!` or `Lazy<Mutex<…>>`?** Beyond the "callbacks can't capture" constraint above, the alternatives add overhead on *every* hooked call. For hot paths like `judgeNotes` (invoked every frame during gameplay), a per-call mutex lock or TLS lookup is a measurable cost. `static mut` accessed via `std::ptr::addr_of!` with null/None guards is the pragmatic choice — the access discipline (guarded, controlled threads) is what keeps it sound, not the wrapper type.
