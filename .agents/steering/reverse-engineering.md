# Reverse Engineering — Project-Specific Context

This file captures the DDR World-specific facts an RE session needs. The universal RE discipline (disassembly-first, Python for hex math, Ghidra/CE address-space hygiene, AOB wildcarding rules, cross-version compatibility) lives in the `sdd-reverse-engineer` agent's system prompt and applies regardless of project.

## Target Modules

Relevant DLLs loaded into the DDR World process. The Ghidra MCP can only inspect one at a time — ask the user to switch modules when you need to examine a different one.

| Module | Role |
|---|---|
| `gamemdx.dll` | Main game binary. Most hooks, signatures, and RTTI walks target this. |
| `libafp-win64.dll` | Konami's AFP animation / BM2D MovieClip system. Texture name resolution, template patching. |
| `libafputils-win64.dll` | AFP utility exports — `afpu_get_texture_bind_id` for BM2D texture ptr → bind ID. |
| `libavs-win64.dll` | AVS filesystem layer. LayeredFS hooks attach here. Six version tables (`avs2_core_*` through `avs2_core_*_v6`) are resolved by name. |
| `arkmdxbio2.dll` (or `arkmdxp3.dll` / `arkmdxp4.dll`) | I/O driver. Arcade button state exports (`arkMDXGetStart`, `arkMDXGet10Key`, etc.). |

## Address Space

- `gamemdx.dll` loads with file-relative base `0x180000000`. Ghidra addresses are file-relative; the hook resolves runtime addresses at load time via module-base arithmetic.
- All addresses published in `docs/` must be file-relative (Ghidra) — the runtime form is derived at hook load time, not baked into documentation.

## Research Document Conventions

Existing research lives under `docs/`. When producing new research, match the tone and structure of:

- `afp_system.md` — system-level overview + key addresses + call sites
- `bpl_battle_mode_research.md` — feature-deep-dive with struct layouts
- `event_flag_system_research.md` — subsystem reverse-engineering with cross-version notes
- `widget_registration_system.md` — hook-point research with calling-convention detail

Common sections: Overview, Key Addresses, Struct Layouts, Signatures, Call Sites / Xrefs, Cross-Version Notes, Gotchas.

## Derivation Anchors (project-specific)

Some global addresses are derived via RIP-relative decode from known landmarks rather than scanned directly. These anchors are referenced by signature name in the hook code:

| Landmark signature | Resolves | Downstream use |
|---|---|---|
| `app_heap_reserve_anchor` | `app_heap_handle` global (currently `DAT_180460058`) | AGCS allocator-aware container allocation |
| `game_malloc` | MSVC `operator new` | CRT-heap allocations for game-owned objects |

When extending this table, document the derivation chain in `docs/` so future RE sessions can audit it.

## Cross-Version Testing

When declaring a signature or struct layout version-agnostic, verify on BOTH currently-supported game versions before publishing. Document both versions' resolved addresses in the research file.
