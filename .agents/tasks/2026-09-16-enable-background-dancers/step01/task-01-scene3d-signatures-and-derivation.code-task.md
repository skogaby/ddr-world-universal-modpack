# Task: Add the `scene3d` signature group and `derive_scene3d` to the signature store

## Description
Add every AOB signature and the single all-or-nothing derivation the Background Dancers feature needs
from the engine (design §4.2.7, RE record `docs/background_dancers_research.md` §1) to
`src/core/signatures.rs`, plus a `Scene3dSites` bundle getter. No engine-facing code, no mod, no
runtime behaviour beyond boot-log lines — this task makes every engine address/offset the later steps
consume resolvable, identity-gated and published, or cleanly un-resolved with one WARN.

## Background
World's engine still runs the 3D scene graph and model passes every frame with an empty item list.
The mod will hand it hand-built render items through a mod-owned node type, write camera slot 0, look
models up in the ResourceManager, create/release bone textures through the engine texture API, and hide
the 2D background clip. Every one of those touches an engine address or struct offset. Per AGENTS.md
Rust Quality Rule 6 none may be hardcoded in hook code: they are AOB-scanned or RIP/imm-decoded from a
scanned landmark, cross-checked against a second independent site, and published through
`publish_value` so they show in the boot log as `name (derived) = 0x…`.

The RE for this step is COMPLETE and recorded in `docs/background_dancers_research.md` §1.1–§1.7:
the exact byte patterns, their hit counts on all five builds (20250805 / 20260224 / 20260721 /
20260825 / 20260915), every match+N offset to decode, and every identity gate. Implement what that
table says; do not re-derive from Ghidra.

Existing patterns to copy: `derive_bottom_text` (identity-gated, all-or-nothing, un-resolve on any
miss), `derive_smarvelous_burst` (all call sites must agree), `derive_cmovieclip_create` (standalone
pattern), `find_vtable_by_rtti` (RTTI vtable lookup), `publish_value`/`published_value`,
`gameplay_actor_layout()` (a `Copy` struct getter over published values).

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-16-enable-background-dancers/design/detailed-design.md` (§4.2.7 signature table, §4.2.5 node layout, §5.3 camera slot)
- RE record: `docs/background_dancers_research.md` §1.1–§1.7 (THE byte-level spec for this task)

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-16-enable-background-dancers/research/world-background-and-movie.md` §1, §3 (the bg_root / BgMovieActor object graph)
- `docs/background_dancers_feasibility.md` §2.3, §11 (World address table, for cross-reading)
- `scripts/sig_harness/report.py` header comment (how `required_signatures` / `get_address` consumers are discovered — the getter must not use `require_address`)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. New `SignatureDefinition` entries in `SIGNATURES` (names, patterns and descriptions per RE §1.7), grouped under a `// ── Background Dancers (scene3d) ──` comment: `sg_enable_bit_site`, `sg_manager_tick`, `sg_active_camera`, `sg_destroy_flush`, `sg_update_job_run`, `sg_insertion_sort`, `model_registry_release`, `texture_create_site`, `texture_release`, `bgmovie_readiness`, `bgmovie_ready_call_site`, `bg_root_create_site`. Each description must state what is read at `match+N` (so `shape_diff.py` reviewers know what to check) and the expected hit count.
2. One `fn derive_scene3d(&mut self)` called at the END of `resolve_derived` (after `derive_cmovieclip_create` and `derive_strip_hud_anchors` — it consumes `cmovieclip_create`). All-or-nothing: compute everything into locals first; on ANY failure log exactly one `[-] scene3d -- <site>: <reason>` WARN, publish NOTHING, and `self.resolved.remove` every `scene3d_*` name plus the twelve raw AOB names so no consumer can pick up a half-resolved group. On success publish every address/value listed in RE §1.7 under the `scene3d_` prefix and log the `[+]` lines (`publish_value` does this for values; use `log_info!("  [+] {} (derived) @ +0x{:X}", …)` for addresses).
3. Identity gates exactly as RE §1.7: (a) all `sg_enable_bit_site` hits decode the same RIP global; (b) tick CALL @+6 == `sg_destroy_flush` match and CALL @+11 == `sg_active_camera` match; (c) `sg_active_camera` and `sg_update_job_run` RIP globals == (a); (d) the flush body contains `48 8B 01 BA 01 00 00 00 FF 10` and exactly one further `FF 15` after match+64 within 0x180 bytes; (e) `sg_update_job_run` CALL target's first 16 bytes are `40 53 56 57 41 54 41 56 41 57 48 83 EC 48 48 8B 59`; (f) the sort dispatcher (CALL after `41 F6 44 24 ?? 01 74 ??` in the update body) has a CALL rel32 whose target == the `sg_insertion_sort` match; (g) exactly one `model_registry_release` hit contains a `48 8D 0D disp32` whose target == `find_vtable_by_rtti(".?AV?$GpuResource@VModelData@gs@@@Resource@agcs@@", …)`, and its `FF 15` slot == the flush's lock slot; (h) `texture_release`'s `F0 0F C1 05 disp32` global == the create body's first `F0 0F C1 05` global (scan the first 0x80 bytes of `scene3d_texture_create`); (i) ≥1 `bgmovie_ready_call_site` hit decodes (CMP RIP global, CALL target) == (`scene3d_bgmovie_actor`, `bgmovie_readiness` match) — note `48 83 3D disp32 imm8` is an 8-byte instruction: target = `decode_rip_relative(match+3) + 1`; (j) in `bg_root_create_site`'s forward window (≤ 0x100 bytes) the CALL rel32 immediately preceded by `LEA R8,[rip+X]` where `X` points at the NUL-terminated bytes `bg_root` must == `cmovieclip_create`.
4. Plausibility checks on every decoded value (the `derive_gameplay_actor_layout` habit): every derived address inside the module; camera stride in `0x200..=0x800` with `active_off == stride − 4`; every struct offset `< 0x1000`; vector end offsets == begin + 8; `target_off == eye_off + 0xC`, `up_off == target_off + 0xC`; `far_off == near_off + 4`; l/r/b/t four contiguous 4-byte fields; pool stride in `0x100..=0x400`, pool count in `0x100..=0x1000`, clip slot in `0x40..=0x400`.
5. Camera field block derived from the tick's two callees (RE §1.7 last row). View rebuild (CALL @+41 target, body window 0x700 bytes): the first `80 B9 disp32 00` (`CMP byte [rcx+disp32],0`, within the first 0x40 bytes) = `cam_view_dirty_off`; `cam_proj_req_off = cam_view_dirty_off + 1`, attested by a `CMP byte [reg+<cam_view_dirty_off+1>],0` (`80 B8..BF disp32 00`, ANY base register — the body switches from RCX to RDI) somewhere in the body, and by the `MOV word [reg+<cam_view_dirty_off+2>],0x0101` store (`66 C7 8? disp32 01 01`); the first MOVSS load `[rcx+disp32]` = `cam_eye_off`; the first SUBSS `[rcx+disp32]` = `cam_target_off`; `cam_up_off = cam_target_off + 0xC`, attested by a MOVSS load `[reg+cam_up_off]` (any base) in the body. Projection rebuild (CALL @+53 target): the first seven MOVSS loads `[rcx+disp32]` in order = w, near, far, l, r, b, t. Decode these generically — `F3`, optional REX (`41`/`44`/`45`), `0F 10` (MOVSS) / `0F 5C` (SUBSS), ModRM with mod=10 and the required rm, then disp32 — so `F3 44 0F 10 89 …` and `F3 0F 10 B9 …` both match.
6. `#[derive(Clone, Copy, Debug)] pub struct Scene3dSites { … }` with one field per published name (addresses as `*const u8`, offsets as `usize`) and `pub fn scene3d_sites(&self) -> Option<Scene3dSites>` returning `None` unless EVERY field is present (use `get_address`/`published_value`, never `require_address`). Include the two IAT slot addresses (the service dereferences them at call time), `scene_graph_update` (shape/identity only — documented as not called), and both `texture_create`/`texture_release`.
7. The file must keep compiling in the offline harness (`scripts/validate_signatures.sh` mounts `signatures.rs` into a std-only crate — no `windows`/`retour` imports outside the existing `#[cfg(windows)]` blocks; `find_vtable_by_rtti` and the scanner primitives are already host-safe).
8. No new hardcoded addresses anywhere; the only literal bytes are AOB patterns and the identity-gate byte shapes named in RE §1.7. Comments cite the World 20260825 function for each anchor (e.g. `// FUN_180024250 destroy flush`).

## Dependencies
- `src/core/signatures.rs` (`SIGNATURES`, `SignatureStore`, `publish_value`, `find_vtable_by_rtti`, `get_all_matches`, `xrefs_to`), `src/core/scanner.rs` (`decode_rip_relative`, `decode_call_rel32`, `scan_pattern_all`)
- Existing derived names consumed: `cmovieclip_create` (from `derive_cmovieclip_create` — must run BEFORE `derive_scene3d`)
- Inferred: `derive_strip_hud_anchors` is NOT a dependency (the ArrowPalette vtable cross-check was dropped in favour of the spin-flag identity — see RE §1.4)

## Implementation Approach
1. Add the twelve `SignatureDefinition`s at the end of `SIGNATURES` with the descriptions.
2. Add `Scene3dSites` next to `GamePlayActorLayout` / `ShutterActorLayout`.
3. Implement `derive_scene3d` as a sequence of small private helpers returning `Option<…>` (one per anchor: `scene3d_manager_global`, `scene3d_tick_sites`, `scene3d_flush_layout`, `scene3d_camera_layout`, `scene3d_update_layout`, `scene3d_model_registry_layout`, `scene3d_texture_api`, `scene3d_bg_sites`) so each failure reason is specific; the top-level fn collects into a `Scene3dResolved` local and only on `Some` publishes.
4. Write a `fail(&mut self, site, reason)` closure/helper that removes every `scene3d_*` key and the raw AOB names, logs the WARN, and returns.
5. Call `self.derive_scene3d()` at the end of `resolve_derived`.
6. Add `scene3d_sites()` reading back every name.
7. `cargo check --target x86_64-pc-windows-msvc` clean; `./scripts/validate_signatures.sh ~/Desktop/ddr_modules` builds and every `scene3d_*` line is `[+]` on all four builds (full sweep/shape review is task 02 — here just confirm the group resolves).
8. `cargo fmt` (whole crate).

## Acceptance Criteria

1. **Group resolves on every supported build**
   - Given the four `gamemdx_*.dll` in the sweep directory
   - When `./scripts/validate_signatures.sh ~/Desktop/ddr_modules --raw` runs
   - Then every `scene3d_*` name appears as `[+] … (derived)` on 20250805 / 20260224 / 20260721 / 20260825 with the values `0x08 / 0x28 / 0x2C / 0x38 / 0x18 / 0x78 / 0xE8 / 0x38 / 0x3F8 / 0x3F4 / 0x2B3 / 0x130 / 0x30 / 0x39 / 0x18 / 0x10 / 0x28 / 0x30 / 0x58 / 0x240 / 0x400 / 0x140 / 0x268 / 0x274 / 0x280 / 0x28C / 0x290 / 0x294 / 0x298 / 0x29C / 0x2A8 / 0x2AC / 0x2B0 / 0x2B1` where the RE doc names them, and the report's exit status is 0

2. **All-or-nothing on a miss**
   - Given a build where any single anchor is absent or an identity gate fails (simulate by temporarily corrupting one pattern in a scratch copy, or by the harness's per-build behaviour)
   - When `resolve_derived` runs
   - Then exactly one `[-] scene3d -- <site>: <reason>` WARN is logged, no `scene3d_*` name is resolved, the twelve raw AOB names are also un-resolved, and `scene3d_sites()` returns `None`

3. **Getter completeness**
   - Given a fully resolved store
   - When `scene3d_sites()` is called
   - Then it returns `Some(Scene3dSites)` whose every field equals the corresponding published value (a host unit test in `signatures.rs` under `#[cfg(test)]` may construct a store over a synthetic buffer is NOT required — verify by the harness log instead)

4. **Host harness still compiles**
   - Given the std-only harness crate generated by `scripts/validate_signatures.sh`
   - When it builds
   - Then `signatures.rs` compiles without `windows` crate references outside `#[cfg(windows)]`

5. **Formatting and type check**
   - Given the finished change
   - When `cargo check --target x86_64-pc-windows-msvc` and `cargo fmt` (no file args) run
   - Then both are clean and `git diff --stat` shows only `src/core/signatures.rs`

## Metadata
- **Complexity**: High
- **Labels**: signatures, reverse-engineering, scene3d, background-dancers, step-1
- **Required Skills**: Rust, x86-64 instruction encoding (REX/ModRM/RIP-relative), the repo's signature-store conventions
- **Generated By**: code-task-generator 2026-09-16
- **Source Plan**: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md`
- **Plan Step**: Step 1: Signatures and derivations for the `scene3d` group + cross-build sweep
