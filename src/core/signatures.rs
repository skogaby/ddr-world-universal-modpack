//! Signature Store — Centralized registry of known function signatures.
//!
//! Each signature maps a logical function name to an AOB pattern.
//! After AOB scanning, `resolve_derived()` computes additional addresses
//! from the found signatures (RIP-relative operands, RTTI walks, string refs).

use crate::core::module_resolver::GameModule;
use crate::core::scanner::{
    decode_call_rel32, decode_rip_relative, find_function_entry, scan_first_call_rel32,
    scan_pattern, scan_pattern_all, scan_patterns_batch, scan_xrefs_to,
};
use crate::{log_info, log_warn};
use std::collections::HashMap;

pub struct SignatureDefinition {
    pub name: &'static str,
    pub pattern: &'static str,
    pub description: &'static str,
}

pub struct ResolveResult {
    pub found: usize,
    pub total: usize,
    pub missing: Vec<String>,
}

const SONG_RATE_CLOCK_ANCHOR_PATTERN: &str = "48 63 89 84 00 00 00 48 8D 35 ?? ?? ?? ?? 33 D2 48 8B 0C CE E8 ?? ?? ?? ?? 48 8B 10 48 8B C8 FF 92 48 02 00 00 44 8D 34 18 4C 8D 67 58 41 0F B7 54 24 2A";
const SONG_RATE_WAVEBANK_CREATE_PATTERN: &str = "48 8B C4 55 41 54 41 55 41 56 41 57 48 8D A8 28 FF FF FF 48 81 EC B0 01 00 00 48 C7 45 90 FE FF FF FF 48 89 58 10 48 89 70 18 48 89 78 20 48 8B 05 ?? ?? ?? ?? 48 33 C4 48 89 85 A0 00 00 00 48 63 F1 4C 8B 35 ?? ?? ?? ?? 49 8B 56 68 49 8B 46 70";
const SONG_RATE_WAVEBANK_UNREGISTER_PATTERN: &str = "48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 20 48 8B 05 ?? ?? ?? ?? 48 8B 35 ?? ?? ?? ?? 48 63 F9 48 8D 14 BF 41 B8 03 00 00 00 48 C1 E2 05 48 03 50 28 0F B6 82 8F 00 00 00 48 8D 4C 10 11 48 8D 15 ?? ?? ?? ?? E8 ?? ?? ?? ?? 85 C0 75 ??";
const SONG_RATE_CLOCK_PATCH_OFFSET: usize = 0x25;
const SONG_RATE_CLOCK_EXPECTED: [u8; 8] = [0x44, 0x8d, 0x34, 0x18, 0x4c, 0x8d, 0x67, 0x58];
// Audio-manager ctor callback-registration region: `lookAheadTime = 0xFA`
// immediate followed by three `LEA RAX,[rip+disp32] / MOV [RBP+disp8],RAX`
// pairs (notification, readFile, getOverlappedResult). LEA disp32s and frame
// disp8s wildcarded; the 0xFA immediate and instruction shape are literal.
const SONG_RATE_IO_CALLBACK_REGSITE_PATTERN: &str = "C7 45 ?? FA 00 00 00 48 8D 05 ?? ?? ?? ?? 48 89 45 ?? 48 8D 05 ?? ?? ?? ?? 48 89 45 ?? 48 8D 05 ?? ?? ?? ?? 48 89 45 ??";
// disp32 positions of the second and third LEAs inside the regsite match
// (readFile and getOverlappedResult callback entries, RIP-decoded).
const SONG_RATE_IO_READFILE_LEA_DISP: usize = 21;
const SONG_RATE_IO_OVERLAPPED_LEA_DISP: usize = 32;
// The readFile callback body's literal prologue up to (and including) its
// first CALL opcode at entry+0x21 — `E8 rel32` to the handle→file_id lookup
// helper. Byte-identical on all four supported builds except the rel32.
const SONG_RATE_IO_READFILE_PREFIX: [u8; 34] = [
    0x48, 0x89, 0x5C, 0x24, 0x10, 0x48, 0x89, 0x74, 0x24, 0x18, 0x48, 0x89, 0x7C, 0x24, 0x20, 0x41,
    0x54, 0x48, 0x83, 0xEC, 0x40, 0x49, 0x8B, 0xD9, 0x41, 0x8B, 0xF0, 0x4C, 0x8B, 0xE2, 0x48, 0x8B,
    0xF9, 0xE8,
];
const SONG_RATE_IO_READFILE_CALL_OFFSET: usize = 0x21;
// Inside the `song_rate_wavebank_unregister` match: `MOV RAX,[rip+disp32]`
// at match+15 loads the audio file-table global (disp32 at match+18). The
// pattern's literal bytes already pin the access shape that global feeds
// (`+0x28` path-rows load, 0xA0-stride row math, `+0x11` path offset).
const SONG_RATE_IO_FILE_TABLE_MOV_OFFSET: usize = 15;
const SONG_RATE_IO_FILE_TABLE_MOV_OPCODE: [u8; 3] = [0x48, 0x8B, 0x05];
const SONG_RATE_IO_FILE_TABLE_DISP: usize = 18;

/// All known AOB signatures for DDR World (gamemdx.dll).
const SIGNATURES: &[SignatureDefinition] = &[
    SignatureDefinition {
        name: "timer_update_jz",
        pattern: "2B F9 33 F6 85 FF 7F ? 8B FE EB ? B8 ? ? ? ? ? ? 0F 4F ? ? ? ? ? ? ? 0F 84",
        description: "TimerActor update — JZ at +28 skips timer display update",
    },
    // ── TimerActor state-1 "show" site ────────────────────────────────
    // `sequence::common::TimerActor::onUpdate` (FUN_18003c790 on 20260616),
    // state 1's bottom block: the ONLY place in the binary that makes the
    // actor's `timer_root` layer visible. The layer is created hidden; when
    // the scene arms the timer (byte at actor+0xBC), this block plays the
    // "in"/"hazard_in" label and calls the play+set-visible helper
    // (layer_play(1.0) + afp_layer_set_attribute(id, 1, visible)) with that
    // byte as the visible flag. The timer-reset path (msg 0x1003 →
    // FUN_18003d3a0 → state 1) re-enters through this same instruction.
    //
    //   80 BD BC 00 00 00 00   CMP  byte [RBP+0xBC], 0     (armed gate)
    //   0F 84 rel32            JZ   epilogue
    //   8B 85 B0 00 00 00      MOV  EAX, [RBP+0xB0]        (total seconds)
    //   48 8D 0D d32           LEA  RCX, ["hazard_in"]
    //   48 8D 15 d32           LEA  RDX, ["in"]
    //   3B 85 B4 00 00 00      CMP  EAX, [RBP+0xB4]        (hazard threshold)
    //   48 0F 4F D1            CMOVG RDX, RCX
    //   41 B1 01 / 45 0F B6 C1 (label-play args)
    //   48 8B 8D A8 00 00 00   MOV  RCX, [RBP+0xA8]        (timer_root wrapper)
    //   E8 rel32               CALL SetFrameLabel helper
    //   0F B6 95 BC 00 00 00   MOVZX EDX, byte [RBP+0xBC]  <- patch at +62
    //
    // The timer-freeze mod rewrites the MOVZX at match+62 to XOR EDX,EDX +
    // 5 NOPs so the helper is always called with visible=0: the timer layer
    // (frame art + digit children — display collection skips the whole clip
    // tree when the layer attribute bit is clear) never shows, while the
    // state machine, countdown, and timeout semantics stay stock. Unique
    // single match on 20260526 (0x18003c162), 20260616 (0x18003cb72) and
    // 20260721 (0x18003c0b2), byte-identical apart from the wildcarded
    // displacements.
    SignatureDefinition {
        name: "timer_show_call",
        pattern: "80 BD BC 00 00 00 00 0F 84 ?? ?? ?? ?? 8B 85 B0 00 00 00 48 8D 0D ?? ?? ?? ?? 48 8D 15 ?? ?? ?? ?? 3B 85 B4 00 00 00 48 0F 4F D1 41 B1 01 45 0F B6 C1 48 8B 8D A8 00 00 00 E8 ?? ?? ?? ?? 0F B6 95 BC 00 00 00",
        description: "TimerActor onUpdate state-1 show site — MOVZX EDX,[actor+0xBC] at +62 feeds the layer set-visible helper; timer-freeze zeroes it to hide the timer display",
    },
    SignatureDefinition {
        name: "premium_free_stage_inc",
        pattern: "48 8B 08 FF 41 0C",
        description: "Per-frame stage counter increment — MOV RCX,[RAX]; INC dword [RCX+0xc]. Patch site at +3 (the 3-byte INC).",
    },
    // ── Per-stage play-record accessor ────────────────────────────────
    // `getStageRecord(side, stage)` — a tiny leaf accessor that returns
    // `PlayerWork + <base> + stage*<stride>` (or the course-mode record when
    // `GameWork+<course_off> != 0`). One match on builds 20260526/20260616,
    // byte-identical apart from the two RIP disp32s. The premium-free mod
    // decodes everything it needs from the matched bytes (bm2d_package
    // precedent — no hardcoded layout constants):
    //
    //   +0   MOV RAX,[rip+d32]        ; d32 at +3  -> game-work ptr global
    //   +7   MOV R8,[RAX]             ; ptr -> GameWork (double indirection)
    //   +10  MOVSXD RAX,ECX
    //   +13  LEA RCX,[rip+d32]        ; d32 at +16 -> player_work_table
    //   +20  CMP qword [R8+d8],0      ; d8 at +23  -> course-mode field (0x70)
    //   +25  MOV RAX,[RCX+RAX*8]      ; table[side] = wrapper*
    //   +29  MOV RAX,[RAX]            ; *wrapper   = PlayerWork*
    //   +32  JZ +7
    //   +34  ADD RAX,imm32            ; course record offset (0x2D8)
    //   +40  RET
    //   +41  MOVSXD RCX,EDX
    //   +44  IMUL RCX,RCX,imm32       ; imm32 at +47 -> record stride (0x2B8)
    //   +51  LEA RAX,[RAX+RCX+d32]    ; d32 at +55  -> record base (0x590)
    //   +59  RET
    SignatureDefinition {
        name: "stage_record_accessor",
        pattern: "48 8B 05 ?? ?? ?? ?? 4C 8B 00 48 63 C1 48 8D 0D ?? ?? ?? ?? 49 83 78 ?? 00 48 8B 04 C1 48 8B 00 74 07 48 05 ?? ?? 00 00 C3 48 63 CA 48 69 C9 ?? ?? 00 00 48 8D 84 08 ?? ?? 00 00 C3",
        description: "getStageRecord(side, stage) accessor — sources the game-work ptr global, player-work table, course-mode field offset, per-stage record stride and base. Consumed by the premium-free stale-record fix.",
    },
    // ── Premium Free ghost cache (same-credit PB ghost under a frozen stage) ──
    // `sequence::dance::GhostActor` init (20260721 `FUN_180056ad0`; 20260616
    // `0x180056b00`, 20260825 `0x180056a40` — match+0x0D..). Resolves the ghost
    // id via the score-DB lookup, then either copies a LOCAL stage slot's
    // grade stream (negative id → `PlayerWork[side] + 0x590 + stage*0x2B8 +
    // 0xB8`), kicks the network load (positive id), or leaves the vector
    // empty (id 0). Fields it pins (byte-identical on the 2026 builds):
    //
    //   40 53 48 83 EC 40          PUSH RBX; SUB RSP,0x40
    //   48 8B 05 ?? ?? ?? ??       MOV RAX,[security cookie]
    //   48 33 C4 48 89 44 24 30    cookie xor/store
    //   48 8B D9                   MOV RBX,RCX            (actor)
    //   8B 89 84 00 00 00          MOV ECX,[RCX+0x84]     (side)
    //   E8 ?? ?? ?? ??             CALL ghost-id lookup
    //   4C 8B D8                   MOV R11,RAX
    //   48 89 83 90 00 00 00       MOV [RBX+0x90],RAX     (ghost id)
    //   48 85 C0 75                TEST RAX,RAX; JNZ
    //
    // Detoured post-original by premium_free's ghost cache: when the game
    // resolved an EMPTY ghost vector (the frozen-stage slot was virginised
    // + re-prepared at song select), inject the cached same-chart stream.
    SignatureDefinition {
        name: "ghost_actor_init",
        pattern: "40 53 48 83 EC 40 48 8B 05 ?? ?? ?? ?? 48 33 C4 48 89 44 24 30 48 8B D9 8B 89 84 00 00 00 E8 ?? ?? ?? ?? 4C 8B D8 48 89 83 90 00 00 00 48 85 C0 75",
        description: "sequence::dance::GhostActor init — ghost id lookup + local-slot / network ghost resolution. Detoured by the premium-free ghost cache.",
    },
    // The local-slot copy site INSIDE `ghost_actor_init` (match+0x1A1 on
    // 20260721): `IMUL R8,R8,0x2B8; MOV RCX,[RAX]; LEA RDX,[R8+RCX+0x648];
    // LEA RCX,[RBX+0x98]; CALL vector<u8>::assign`. 0x648 = 0x590 + 0xB8 (the
    // record's grade stream). The CALL rel32 at +25 resolves the game's own
    // `vector<u8>` copy-assign (`ghost_vec_copy`, derived) — the allocator-
    // correct way to fill `actor+0x98`. Unique on 20260616/0721/0825.
    SignatureDefinition {
        name: "ghost_local_slot_copy_site",
        pattern: "4D 69 C0 B8 02 00 00 48 8B 08 49 8D 94 08 48 06 00 00 48 8D 8B 98 00 00 00 E8",
        description: "GhostActor init local-slot copy site — CALL at +25 is the game's vector<u8> copy-assign (derived as ghost_vec_copy).",
    },
    // The song-end result commit — GamePlayActor vtable +0x28 (20260721
    // `FUN_18005d970`, 20260526 `FUN_18005d180`). Copies the actor's live
    // judge counters / score cluster / grade decision / note + grade + ms
    // streams / gauge map into `PlayerWork + 0x590 + (GameWork+0xC)*0x2B8`
    // with REPLACE semantics. Two early-outs skip the whole commit:
    //
    //   40 53 56 57 48 81 EC 80 00 00 00   prologue
    //   80 B9 80 02 00 00 00               CMP byte [RCX+0x280],0   (skip flag 1)
    //   48 8B F1 0F 85 ?? ?? ?? ??         MOV RSI,RCX; JNZ skip
    //   8B 81 94 01 00 00 03 81 9C 01 00 00  taps + shocks judged
    //   75 ??                              JNZ (else "MDX1529" no-judge report)
    //   48 8D 0D ?? ?? ?? ?? 33 D2 FF 15 ?? ?? ?? ??
    //   48 83 BE 88 02 00 00 00            CMP qword [RSI+0x288],0  (skip flag 2)
    //
    // Detoured post-original by premium_free's ghost cache (snapshot the
    // committed grade stream) + the bug-1 diagnostic (log the early-outs).
    SignatureDefinition {
        name: "result_commit",
        pattern: "40 53 56 57 48 81 EC 80 00 00 00 80 B9 80 02 00 00 00 48 8B F1 0F 85 ?? ?? ?? ?? 8B 81 94 01 00 00 03 81 9C 01 00 00 75 ?? 48 8D 0D ?? ?? ?? ?? 33 D2 FF 15 ?? ?? ?? ?? 48 83 BE 88 02 00 00 00",
        description: "GamePlayActor result commit (vtable +0x28) — writes the per-stage play record at song end. Detoured by the premium-free ghost cache + diagnostic.",
    },
    // The in-song speed-mod adjustment window's kill gate, inside
    // `sequence::dance::ControlSpeedActor`'s message handler (vtable+0x40).
    // Each frame the gameplay sequence broadcasts msg 0x1045 with the elapsed
    // song time; at payload+0x8 >= 10000 ms the actor self-destructs, which is
    // the ONLY thing that ends the stock speed-adjust window:
    //
    //   41 81 78 08 10 27 00 00   CMP dword [R8+0x8], 0x2710  ; elapsed ms vs 10000
    //   0F 8C                     JL  <window still open>
    //
    // Every byte is structurally fixed (payload layout + the 10000 ms game
    // constant); the JL rel32 is excluded. Unique single match on builds
    // 20260421/20260526/20260616/20260721 (handler entry+0x4F on all four).
    // The anytime-speedmod mod rewrites the imm32 at match+4 to 0x7FFFFFFF so
    // the actor lives until the normal msg-0x104A song-end kill (untouched).
    // RE notes: docs/anytime_speedmod_research.md
    SignatureDefinition {
        name: "speedmod_window_gate",
        pattern: "41 81 78 08 10 27 00 00 0F 8C",
        description: "ControlSpeedActor msg-0x1045 self-destruct gate — CMP [R8+8],10000ms; JL. Anytime-speedmod patches the imm32 at +4.",
    },
    SignatureDefinition {
        name: "fps_target_imm32",
        pattern: "C7 44 24 ?? 3C 00 00 00 75 08 C7 44 24 ?? 4B 00 00 00",
        description: "Fullscreen display-refresh ('FPS') target in Application::onBoot — MOV dword [RSP+d],0x3C (default 60); JNZ +8; MOV dword [RSP+d],0x4B (75 if MachineType==1). The patchable imm32 is at match+4 (the 0x3C). FPS-unlock mod overwrites it (u32) before onBoot consumes it. Value is latched into the D3D device once at boot (never re-read). Unique single match, byte-identical on builds 20250805/20260324/20260526.",
    },
    // Landmark in the timing-init publisher: the four consecutive
    // `MOV EDX,[RBP+d]; LEA RCX,[rip+key]; CALL set_int` pairs that publish
    // SOUND/INPUT/RENDER/BOMB_FRAME offsets into the runtime config map. The
    // FIRST match is the SOUND_OFFSET set-pair; the CALL at match+0xA targets
    // the config-map int setter, which `timing_config_set_int` derives via
    // decode_call_rel32. Resolved this way (not by the setter prologue) because
    // the setter shares a byte-identical prologue with a sibling config-map
    // setter for a different map — only the publisher call-site disambiguates
    // it. The timing-offsets mod hooks the derived setter. Pattern matches the
    // overlapping 4-call run (3 hits) only inside the publisher on both builds.
    SignatureDefinition {
        name: "timing_set_call_landmark",
        pattern: "8B 55 ?? 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 8B 55 ?? 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ??",
        description: "Timing-init publisher config-set landmark: consecutive MOV EDX,[RBP+d]; LEA RCX,[rip+OFFSET_key]; CALL set_int pairs. First match = SOUND_OFFSET pair; CALL at +0xA derives timing_config_set_int (the config-map int setter). Used by the timing-offsets mod.",
    },
    SignatureDefinition {
        name: "hud_layout_builder",
        pattern: "48 8B C4 55 41 54 41 55 41 56 41 57 48 8D A8 28 FE FF FF 48 81 EC B0 02 00 00 48 C7 45 20 FE FF FF FF",
        description: "Gameplay HUD/lane layout builder (entry). RCX = builder_root (a LayoutActor). Per-side layout parent at root+0xE0+side*0x48 (the `parent` the layout setter receives). Center-arrows mod hooks this to capture the builder root. Prologue verified identical across both supported builds.",
    },
    SignatureDefinition {
        name: "hud_layout_setter",
        pattern: "4C 8B DC 56 57 41 54 41 55 41 56 48 83 EC 60 48 C7 44 24 20 FE FF FF FF 49 89 5B 18 49 89 6B 20 48 8B 05",
        description: "Named-layout setter: void(parent /*RCX*/, name /*RDX, C-string*/, coord /*R8, 6xi32; [0]=X,[1]=Y*/). Center-arrows mod detours this to shift coord[0] for the active 1P side's lane-relative keys. Pattern ends at the stack-cookie LEA opcode (the differing displacement is excluded); verified to match one site on both supported builds.",
    },
    // Song-info card builder — the branch cluster that picks the card style:
    //   CMP dword [RBP+0xC4],EDI   ; card style field: 0 = single, 1 = double
    //   SETZ R13B                  ; R13B = 1 when single
    //   LEA RAX,["dance_song_info_single"]
    //   LEA R8, ["dance_song_info_double"]
    //   TEST R13B,R13B
    //   CMOVNZ R8,RAX
    // R13B also gates the doubles dark-tint color write at the function tail
    // (TEST R13B / JNZ skip). The community hex patch for 20250805 (file offset
    // 476947: 41 0F 94 C5 -> 41 B5 00 90) forces R13B=0 here so 1P play gets
    // the dark transparent doubles card that doesn't occlude a centered lane.
    // The center-arrows mod reproduces that effect at runtime by detouring the
    // containing function (entry derived via backward prologue scan from this
    // match) and transiently flipping the +0xC4 style field for gated calls.
    // Unique single match on builds 20250805 (0x18007530D), 20260324
    // (0x18007951D), 20260616 (0x18007882D), 20260721 (0x180078C2D);
    // byte-identical apart from the wildcarded string LEA disp32s.
    SignatureDefinition {
        name: "song_info_card_style",
        pattern: "39 BD C4 00 00 00 41 0F 94 C5 48 8D 05 ?? ?? ?? ?? 4C 8D 05 ?? ?? ?? ?? 45 84 ED 4C 0F 45 C0",
        description: "Song-info card builder style branch: CMP [RBP+0xC4],EDI; SETZ R13B; LEA single/double card names; CMOVNZ. Center-arrows mod derives the builder entry from this match (backward prologue scan) and detours it to force the dark doubles card during centered 1P play.",
    },
    SignatureDefinition {
        name: "player_array_anchor",
        pattern: "48 8B 05 ?? ?? ?? ?? 66 C7 05 ?? ?? ?? ?? 00 FF 66 C7 05 ?? ?? ?? ?? 00 FF 66 C7 05 ?? ?? ?? ?? 00 FF",
        description: "Small lamp-state accessor whose first insn `MOV RAX,[RIP+disp32]` loads the 2-elem player-object array (P1=[0], P2=[1] at +8). The center-arrows mod RIP-decodes disp32 at +3 to get the array, then tests `*(*(*slot) + 4)` (per-side 'is playing' bool) for single-player detection. Several near-identical accessors match this pattern; all reference the same array global, so the first match's disp is authoritative.",
    },
    SignatureDefinition {
        name: "wrapper_render",
        pattern: "48 83 EC 28 48 8B 49 18 48 8B 41 08 48 89 05 ? ? ? ? 48 8B 41 10 48 89 05 ? ? ? ? 48 8B 41 18 48 8B 09 48 89 05 ? ? ? ? 48 8B 01 FF 50 08",
        description: "agcs::BmpString vtable[5] — sets font globals before render",
    },
    SignatureDefinition {
        name: "render_function",
        pattern: "4C 8B DC 55 53 49 8D AB 68 FF FF FF 48 81 EC 88 01 00 00 48 8B 05 ? ? ? ? 48 33 C4 48 89 45 C8 48 8B 41 08 48 8B D9 80 78 49 00",
        description: "kt::BmpfontSimpleString vtable[1] — text rendering",
    },
    SignatureDefinition {
        name: "widget_factory",
        pattern: "40 57 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 40 48 89 6C 24 48 48 89 74 24 50 41 8B",
        description: "Creates kt::BmpfontSimpleString instances",
    },
    SignatureDefinition {
        name: "constructor",
        pattern: "48 89 4C 24 08 57 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 50 48 8B F9 48 8D 05 ? ? ? ? 48 89 01 33 DB 48 89 59 08 45 33 C0 BA C0 00 00 00",
        description: "kt::BmpfontSimpleString constructor",
    },
    SignatureDefinition {
        name: "series_mapper_bounds",
        pattern: "40 0F B6 C6 FF C8 83 F8 14 0F 87",
        description: "Series mapper bounds check — movzx eax,sil; dec eax; cmp eax,0x14; ja default",
    },
    SignatureDefinition {
        name: "version_predicate_lea",
        pattern: "48 8B 50 08 48 8B 0A 48 3B CA 74 ? 4C 8D 05",
        description: "Version filter predicate — MOV+MOV+CMP+JZ before LEA R8 table base, LEA at offset 12",
    },
    SignatureDefinition {
        name: "ui_entry_loop",
        pattern: "BE 08 00 00 00 48 8D 1D",
        description: "FilterButton creation loop — MOV ESI,8 + LEA RBX,[last_entry_key]. Count at offset 1, LEA at offset 5.",
    },
    SignatureDefinition {
        name: "thumbnail_arc_loop",
        pattern: "48 FF C6 48 83 FE 15 0F 86",
        description: "Thumbnail ARC loading loop bound — INC RSI; CMP RSI,0x15; JBE. Series limit at offset 6.",
    },
    // Leaf function `fn(category: u32) -> u32` returning the entry count for a
    // filtersort category. Drives the filtersort selection<->bitfield round-trip
    // (the `version` u64 saved to the profile): the save mask-builder and the
    // load apply-loop both bound their per-entry bit loops by this count. The
    // VERSION category is index 1, whose count is hardcoded to 9 (the stock
    // entry count) — so custom series entries (selection-map index >= 9) never
    // get a bit on save and are never restored on load. series_expansion detours
    // this to return 9 + n_custom for category 1. Match is at function entry.
    // `cmp ecx,0xC; ja; lea rdx,[jumptable]; movsxd rax,ecx`.
    SignatureDefinition {
        name: "filter_entry_count_table",
        pattern: "83 F9 0C 77 ?? 48 8D 15 ?? ?? ?? ?? 48 63 C1",
        description: "Per-category filtersort entry-count leaf fn (category:u32)->u32. VERSION=category 1, hardcoded 9. Detour target for the version-bitfield persistence fix.",
    },
    SignatureDefinition {
        name: "filter_button_panel_config",
        pattern: "48 8B C4 55 57 41 54 48 8D 68 A1 48 81 EC B0 00 00 00 48 C7 45 E7 FE FF FF FF 48 89 58 10 48 89 70 18",
        description: "FilterButton panel config — called for EVERY FilterButton (groups + versions). Sets category at +0xF0, finds BM2D template. Params: (RCX=FilterButton*, EDX=category_index).",
    },
    SignatureDefinition {
        name: "bm2d_pool_iter",
        pattern: "FF C3 48 81 C7 40 02 00 00 81 FB 00 04 00 00",
        description: "BM2D pool iteration — INC EBX; ADD RDI,0x240; CMP EBX,0x400. LEA Rxx,[pool_base] is within 64 bytes before match.",
    },
    SignatureDefinition {
        name: "filter_panel_builder",
        pattern: "48 8B C4 55 41 54 41 55 48 8D 68 B8 48 81 EC 30 01 00 00 48 C7 44 24 68 FE FF FF FF",
        description: "Filter category panel builder — unique prologue with stack cookie at [RSP+0x68]. Creates filter_switch_base BM2D template and renders FilterButton entries.",
    },

    // FilterButton::~FilterButton (destructor body). Fires per filter button as
    // the filter category panel tears down on filter-menu close — the moment the
    // game frees the button objects that series_filter_scroll tracks by raw
    // pointer. The bare dtor prologue is the generic MSVC two-vtable shape (4
    // matches), so the signature extends through the body: two
    // `FilterButton::vftable` LEA/writes (`[RCX]` and `[RCX+0x28]`), then the
    // distinctive `CALL <panel release>; NOP; MOV RCX,[RBX+0x1B0]` tail. Wildcards
    // cover the 3 vtable-LEA/CALL disp32s. Verified unique + cross-version:
    // 20260421 (FUN_180134260) and 20260526 (FUN_180133ba0).
    SignatureDefinition {
        name: "filterbutton_dtor",
        pattern: "48 89 4C 24 08 57 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 48 48 89 6C 24 50 48 89 74 24 58 48 8B D9 48 8D 05 ? ? ? ? 48 89 01 48 8D 05 ? ? ? ? 48 89 41 28 E8 ? ? ? ? 90 48 8B 8B B0 01 00 00",
        description: "FilterButton::~FilterButton. Signature: fn(this). Fires per filter button on filter-menu close; series_filter_scroll detours it to drop its tracked panel pointers before they dangle.",
    },
    SignatureDefinition {
        name: "filter_label_builder_count",
        pattern: "48 89 44 24 20 4C 8D 4D AF 4C 8D 45 8F BA 09 00 00 00 48 8D 4D",
        description: "Filter label builder — MOV [RSP+0x20]; LEA R9; LEA R8; MOV EDX,9; LEA RCX. Count byte at offset 13. LEA RCX [table_base] is 0x64 bytes before the count. Multiple instances exist (one per filter category).",
    },
    // Per-song version display name lookup. Two structurally distinct forms
    // emitted by the compiler across builds; whichever resolves is used.
    //
    // Both forms compute `string_table[ raw_series_u8 ]` via:
    //   MOVZX r32, byte ptr [thisreg + 0x138]  ; raw u8 at song_property+0x138
    //   LEA   rreg, [string_table]              ; RIP-relative
    //   MOV   rreg, [rreg + r*8]                ; OOB-prone for u8 >= 22
    //
    // The table at `string_table` only has 22 entries (one per vanilla series
    // value); custom series values >= 22 read past the end and the resulting
    // garbage pointer crashes sprintf_s in the "Version / %s" filter chip
    // builder. Hook target for series-expansion is the LEA's disp32: redirect
    // it to a 256-entry table the mod owns.
    //
    // Standalone form (newer builds): the lookup lives in its own 19-byte
    // function (slot 21 of the song-property accessor vtable). The LEA is
    // 7 bytes into the match.
    SignatureDefinition {
        name: "series_label_lookup_standalone",
        pattern: "0F B6 81 38 01 00 00 48 8D 0D ?? ?? ?? ?? 48 8B 04 C1 C3",
        description: "Standalone song-property accessor (vtable slot 21). MOVZX EAX,[RCX+0x138]; LEA RCX,[table]; MOV RAX,[RCX+RAX*8]; RET. LEA at match+7. disp32 at match+10.",
    },
    // Inlined form (older builds): the same lookup is inlined into the
    // version-label sprintf builder. The MOVZX target is R8D (REX.R prefix),
    // the LEA writes RAX, and the indexed MOV uses R8 (REX.RX). The LEA is
    // 8 bytes into the match.
    SignatureDefinition {
        name: "series_label_lookup_inlined",
        pattern: "44 0F B6 80 38 01 00 00 48 8D 05 ?? ?? ?? ?? 4E 8B 04 C0",
        description: "Inlined per-song version label lookup (older builds). MOVZX R8D,[RAX+0x138]; LEA RAX,[table]; MOV R8,[RAX+R8*8]. LEA at match+8. disp32 at match+11.",
    },
    // ── Flare-skill series classification (CalcFlareSkill) ─────────────
    // The inlined classification walk inside ddr::player::Record::
    // CalcFlareSkill that maps a song's RAW series byte (vtable+0xA0
    // accessor — NOT the mapped value from series_mapper) into a flare-skill
    // version category: >=18 GOLD(3), >=14 WHITE(2), >=1 CLASSIC(1), else 0.
    // The GOLD test has NO upper bound, so custom series (>= 22) count
    // toward GOLD. Both table operands are module-BASE-relative disp32s
    // (RVAs added to a LEA-materialized base register), not RIP-relative.
    //
    //   +0   CALL qword [RDX+0xA0]       ; raw <series> u8 -> AL
    //   +6   MOVZX R8D,AL
    //   +10  XOR ECX,ECX                 ; walk index = 0
    //   +12  NOP dword [RAX+0]
    //   +16  MOV EDX,[RCX+R13+catRVA]    ; disp32 at +20 (category table)
    //   +24  CMP [RCX+R13+thrRVA],R8D    ; disp32 at +28 (threshold table)
    //   +32  JLE +0xC                    ; classified
    //   +34  SUB RCX,4
    //   +38  CMP RCX,-8                  ; imm8 loop bound at +41 (0xF8)
    //   +42  JGE loop
    //   +44  XOR EDX,EDX                 ; fallthrough -> category 0
    //
    // Wildcards cover only the two data-layout-dependent disp32s; register
    // allocation and branch displacements verified byte-identical on builds
    // 20260324/20260616/20260721 (unique match on all three). Full RE:
    // docs/flare_ranking_research.md.
    SignatureDefinition {
        name: "flare_skill_classifier",
        pattern: "FF 92 A0 00 00 00 44 0F B6 C0 33 C9 0F 1F 40 00 42 8B 94 29 ?? ?? ?? ?? 46 39 84 29 ?? ?? ?? ?? 7E 0C 48 83 E9 04 48 83 F9 F8 7D E4 33 D2",
        description: "CalcFlareSkill series->category walk. Cat-table disp32 at +20, threshold-table disp32 at +28, loop-bound imm8 at +41. series_expansion redirects both disp32s at a 4-entry extended table (adds 'series >= 22 -> category 0') and widens the bound -8 -> -12.",
    },
    // ── Folder Expansion signatures ─────────────────────────────────
    // Only folder_register and folder_has_songs are AOB-scanned.
    // All other folder functions are derived from folder_register xrefs
    // (see derive_folder_functions).
    SignatureDefinition {
        name: "afp_layer_init_wrapper",
        pattern: "48 89 5C 24 10 56 48 83 EC 40 41 8B F1 48 8B D9 48 85 D2",
        description: "gamemdx.dll wrapper around libafp stream lookup + afp_layer_create_with_property. Receives stream name in R8. Hook target for AFP redirect.",
    },
    SignatureDefinition {
        name: "folder_register",
        pattern: "40 55 48 8B EC 48 83 EC 60 48 C7 45 C0 FE FF FF FF 48 89 5C 24 78",
        description: "Pushes FolderProperty into folder list (older builds). Hook target for custom folder injection.",
    },
    // Newer builds (20260526+): compiler emits extra RSI/RDI saves before
    // the frame pointer setup, and saves RBX to [RSP+0x88] instead of [RSP+0x78].
    SignatureDefinition {
        name: "folder_register_v2",
        pattern: "40 55 56 57 48 8B EC 48 83 EC 60 48 C7 45 C0 FE FF FF FF 48 89 9C 24 88 00 00 00",
        description: "Pushes FolderProperty into folder list (20260526+ builds). Same function, different prologue.",
    },
    SignatureDefinition {
        name: "folder_has_songs",
        pattern: "48 8B 05 ? ? ? ? 48 63 51 08 48 8B 08 48 8B 05 ? ? ? ? 44 8B 41 04 41 83 F8 01",
        description: "Has-songs predicate — reads bit_index from functor+0x8, checks count array. Hook target.",
    },
    // ── Gameplay object allocation ───────────────────────────────────
    // The gameplay sequence object has a fixed-size shared_ptr array (one slot per
    // non-ALL_MUSIC folder). Custom folders overflow this. We find the allocation
    // to patch the size and the constructor to zero extra bytes.
    // Pattern: MOV ECX,<size>; CALL malloc; MOV [RBP+??],RAX; TEST RAX,RAX; JZ; MOV RCX,RAX; CALL ctor; JMP short
    // The trailing EB (JMP short) distinguishes this from the 0x400 alloc in the same function.
    SignatureDefinition {
        name: "gameplay_obj_alloc",
        pattern: "B9 ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 89 45 ?? 48 85 C0 74 ?? 48 8B C8 E8 ?? ?? ?? ?? EB",
        description: "Gameplay sequence object allocation — MOV ECX,<size>; CALL malloc; null check; CALL ctor; JMP. Size imm32 at +1, ctor CALL at +20.",
    },
    // ── Scene transition (advanceToScene) ─────────────────────────────
    // TS::advanceToScene — the vtable-dispatched function that calls
    // createNextSequence, installs the new gosub child, and writes
    // m_currentID. The TEST EDX,EDX; JNZ+7 shape (conditional
    // getNextID call) is structurally unique across the binary.
    SignatureDefinition {
        name: "advance_to_scene",
        pattern: "48 89 5C 24 08 57 48 83 EC 20 8B DA 48 8B F9 85 D2 75 07",
        description: "TS::advanceToScene. Prologue + TEST EDX,EDX; JNZ +7 (skip getNextID). Detour target for fixing m_currentID after scene redirects.",
    },
    // ── agcs::Sequence::finish ─────────────────────────────────────────
    // The engine's single scene-advance primitive. Called as
    // finish(this, nextSceneId_1INDEXED): sends message 0x201 to the parent
    // TransitionSequence — whose handler is advanceToScene (createNextSequence
    // → our scene hook, install gosub child, update m_currentID) — then flags
    // the calling subtree for destruction (flags |= 4). Frees nothing; the
    // reaper runs next frame, so calling it from the frame thread is safe and
    // the transition is synchronous. NOTE the 1-indexed scene id — the hook
    // DLL's scene tracking is 0-indexed everywhere else.
    SignatureDefinition {
        name: "sequence_finish",
        pattern: "48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 20 48 8B 59 08 48 8B F1 8B FA F6 43 20 20",
        description: "agcs::Sequence::finish(this, nextSceneId_1INDEXED) — sends msg 0x201 (advanceToScene on the TransitionSequence parent) then marks the subtree for destruction. Frees nothing; reaper runs next frame. Verified unique on 20260616 (0x18021DB90) and 20260721 (0x18021DF70). Consumed by the quick-logout mod and the quick-restart/fail fast paths.",
    },
    // ── GameWork session-state probe ─────────────────────────────────
    // The 36-byte tail of the final-stage-override test leaf (FUN_1801DD660
    // on 20260721; designed in docs/quick_logout_research.md §8.2). Matches
    // at function start + 7. Every GameWork session-state constant is in the
    // matched bytes (offsets are LITERAL in the pattern — a layout change
    // means no match, which fails the consumers closed):
    //
    //   -7   MOV RAX,[rip+d32]        ; d32 at -4 -> GameWork ptr-ptr global
    //   +0   MOV RCX,[RAX]
    //   +3   CMP qword [RCX+d8],0     ; d8 at +6  -> course field (0x70)
    //   +8   MOV EDX,[RCX+d8]         ; d8 at +10 -> stage counter (0x0C)
    //   +11  JNZ +0x17
    //   +13  MOV EAX,[RCX+d32]        ; d32 at +15 -> event-mode field (0xD0)
    //   +19  CMP EAX,1 / JE / CMP EAX,2 / JE
    //   +29  CMP EDX,[RCX+d8]         ; d8 at +31 -> final-stage override (0x10)
    //   +32  SETZ AL / RET
    //
    // Unique on 20250805 (0x1801c6e47), 20260616 (0x1801dd1b7) and 20260721
    // (0x1801dd667). stage_records decodes + cross-checks the constants; the
    // quick-fail fast path consumes them (session-continues predicate).
    SignatureDefinition {
        name: "final_stage_probe",
        pattern: "48 8B 08 48 83 79 70 00 8B 51 0C 75 17 8B 81 D0 00 00 00 83 F8 01 74 0C 83 F8 02 74 07 3B 51 10 0F 94 C0 C3",
        description: "GameWork session-state probe (final-stage override test leaf, match = entry+7). Yields the event-mode (+0xD0) and final-stage override (+0x10) offsets plus cross-checkable course/stage offsets and the GameWork global. Consumed by stage_records for the quick-fail fast path.",
    },
    // ── ShutterActor close-request wrapper ───────────────────────────
    // FUN_1800334f0 on 20260721: the whole-function pattern of the
    // shutter-close broadcast wrapper (`requestClose(kind)` — sends msg
    // 0x1007 with the kind to the ShutterActor singleton and its children).
    // Matches at function entry:
    //
    //   +0   MOV [RSP+8],ECX / PUSH RBX / SUB RSP,0x20
    //   +9   MOV RBX,[rip+d32]        ; d32 at +12 -> ShutterActor singleton
    //   +16  TEST RBX,RBX / JZ ...
    //   +21  TEST byte [RBX+0x20],0x20 / JNZ ...   ; tree-flags dispatch guard
    //   +27  MOV RAX,[RBX] / LEA R8,[RSP+0x30]
    //   +35  MOV EDX,0x1007           ; the message imm pins this wrapper
    //   +40  MOV RCX,RBX / CALL [RAX+0x18]         ; onMessage(this,0x1007,&kind)
    //
    // The 0x1007 imm disambiguates against the sibling 0x1008/kind-close
    // wrappers (a shorter tail-only pattern matched 2 sites per build).
    // Unique on 20250805 (0x1800337e0), 20260616 (0x180034020) and 20260721
    // (0x1800334f0). Consumed by derive_shutter_actor_global for the
    // quick-restart/fail bannerless fast path (the 0x100c stage-shutter
    // dismiss + the state gates around it).
    SignatureDefinition {
        name: "shutter_close_request",
        pattern: "89 4C 24 08 53 48 83 EC 20 48 8B 1D ?? ?? ?? ?? 48 85 DB 74 ?? F6 43 20 20 75 ?? 48 8B 03 4C 8D 44 24 30 BA 07 10 00 00 48 8B CB FF 50 18",
        description: "ShutterActor close-request wrapper (msg 0x1007 broadcast). MOV RBX,[rip+d32] at +9 yields the ShutterActor singleton global (derived as shutter_actor_global).",
    },
    // ── Gameplay-entry loader ctor mask imms ─────────────────────────
    // createNextSequence case 0x1c/0x34 (the pre-gameplay stage
    // LoadingSequence ctor args): MOV EDX,0x8000 (load mask) followed by
    // MOV R8D,0x32000 (unload mask). The load imm pins the site — the other
    // unload-0x32000 caller (the course loader, case 0x2c) loads 0xD000.
    // Unique on 20250805 (0x18002fabb), 20260616 (0x1800301a0) and 20260721
    // (0x18002fc0b).
    //
    // The unload imm32 at match+7 is byte-patched 0x32000 → 0x30000 by
    // quick-restart-or-fail (select-residency patch): stock gameplay entry
    // evicts the select-music packages (mask 0x2000), which is what made
    // every gameplay → song-select hop (the quick-fail fast path AND the
    // natural post-results return) spend ~5 s reloading them. Keeping them
    // resident makes the 0-idx 24 loader a residency no-op.
    SignatureDefinition {
        name: "gameplay_loader_masks",
        pattern: "BA 00 80 00 00 41 B8 00 20 03 00",
        description: "Stage-loader ctor args in createNextSequence case 0x1c (MOV EDX,0x8000 + MOV R8D,0x32000). Unload imm32 at +7 patched to 0x30000 to keep select-music packages resident through gameplay.",
    },

    // ── In-place song reset (services::song_reset) ───────────────────
    // RE record: .agents/planning/20260812-inplace-restart/research/run_state_re.md
    // (§6 audio, §6 broadcast shape, §9 messages). All five patterns
    // verified this session: unique on 20250805 / 20260616 / 20260721
    // except dps_timing_anchor_site (2 matches on 20250805, both decoding
    // to the SAME tick global — the derivation requires that agreement).
    //
    // Song play-by-bank wrapper (FUN_1801aa5c0 on 20260721): the whole
    // prologue through the profiling-marker guard and the tail-call setup
    // into the inner play routine. Distinctive: the XORPS XMM2 (pan = 0)
    // before forwarding, and the NOP after the profiling FF 15. Called as
    // (slot /*ECX*/, const char* bankName /*RDX*/) -> i32 handle (-1 =
    // fail). Slot 5 = the per-song bank registered by DPS onSetup.
    SignatureDefinition {
        name: "song_play_by_bank",
        pattern: "40 57 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 40 48 8B DA 8B F9 48 8D 05 ?? ?? ?? ?? 48 89 44 24 50 8B 0D ?? ?? ?? ?? 85 C9 7E 07 FF 15 ?? ?? ?? ?? 90 0F 57 D2 48 8B D3 8B CF E8",
        description: "Song play/prepare wrapper: (slot, bank_name) -> i32 cue handle, -1 on failure. DPS update state 4 calls (5, name) and stores the handle at DPS+0x128. Consumed by song_reset (stop → replay audio rewind).",
    },
    // Song stop-by-handle wrapper (FUN_1801aa7c0 on 20260721): guards
    // handle != -1, computes the manager slot ((handle+5)*0x20 + mgr) and
    // stops via the slot object's vtable. The (handle+5)*0x20 arithmetic
    // (LEA RAX,[RBX+5]; SHL RAX,5; ADD RAX,[rip]) is kept literal — a slot
    // layout change must break the match.
    SignatureDefinition {
        name: "song_stop_by_handle",
        pattern: "40 53 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 8B D9 48 8D 05 ?? ?? ?? ?? 48 89 44 24 48 8B 0D ?? ?? ?? ?? 85 C9 7E 0C FF 15 ?? ?? ?? ?? 8B 0D ?? ?? ?? ?? 83 FB FF 74 ?? 48 8D 43 05 48 C1 E0 05 48 03 05",
        description: "Song stop wrapper: (i32 cue handle). DPS state 8 and DPS::leave stop the song with it (handle from DPS+0x128). Consumed by song_reset Phase 1.",
    },
    // Song is-prepared probe (FUN_1801aa630 on 20260721): returns the
    // per-handle prepared byte (*(mgr + handle*0x20 + 0xB0)). The 0x20
    // stride SHL and the literal 0xB0 displacement pin the layout.
    SignatureDefinition {
        name: "song_is_prepared",
        pattern: "40 53 48 83 EC 20 8B D9 8B 0D ?? ?? ?? ?? 85 C9 7E 0C FF 15 ?? ?? ?? ?? 8B 0D ?? ?? ?? ?? 48 8B 05 ?? ?? ?? ?? 48 8B D3 48 C1 E2 05 0F B6 9C 02 B0 00 00 00",
        description: "Song prepared probe: (i32 cue handle) -> bool. DPS state 5 gates song start on it. Consumed by song_reset Phase 2 (poll before re-anchoring).",
    },
    // Recursive actor-subtree message broadcast (FUN_18022eaa0 on
    // 20260721): broadcast(actor, msg, param, depth) — checks the
    // dispatch-suppressed flag (+0x20 & 0x20), calls the actor's
    // onMessage (vt+0x18), and recurses over first-child/next-sibling.
    // This is the engine's own delivery primitive for every 0x10xx
    // message; DPS states 5/6 use exactly this to send 0x1043/0x1044.
    SignatureDefinition {
        name: "update_broadcast",
        pattern: "48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 57 48 83 EC 20 F6 41 20 20 41 8B E9 49 8B F8 8B F2 48 8B D9 75 ?? 48 8B 01 FF 50 18",
        description: "agcs actor-subtree message broadcast (actor, msg, param, 0). Flag-guarded onMessage + child recursion — the engine's own delivery for the 0x1043/0x1044 timing protocol. Consumed by song_reset Phase 2.",
    },
    // DPS update state 6 — the timing-anchor broadcast site
    // (0x180058a91 on 20260721):
    //
    //   +0   MOV RAX,[rip+d32]        ; d32 at +3 -> frame-clock global
    //   +7   MOV RCX,[RAX+0x1268]     ; current frame tick (ms domain)
    //   +14  MOV [RSP+0x58],RCX       ; the 0x1044 payload
    //   +19  TEST byte [RSI+0x20],0x20
    //   +23  JNZ ...
    //   +25  MOV RAX,[RSI] / LEA R8,[RSP+0x58]
    //   +33  MOV EDX,0x1044           ; the timing-anchor message imm
    //
    // The 0x1268 displacement + the 0x1044 imm pin the site. Two matches
    // on 20250805 (a second state machine shares the shape) — both decode
    // to the same global, which derive_frame_tick_global REQUIRES.
    SignatureDefinition {
        name: "dps_timing_anchor_site",
        pattern: "48 8B 05 ?? ?? ?? ?? 48 8B 88 68 12 00 00 48 89 4C 24 58 F6 46 20 20 75 ?? 48 8B 06 4C 8D 44 24 58 BA 44 10 00 00",
        description: "DPS state-6 timing-anchor read+broadcast site. RIP disp32 at +3 yields frame_tick_global (current tick at +0x1268 — the exact clock the 0x1044 anchor must carry). Consumed via derive_frame_tick_global by song_reset.",
    },

    // GamePlayActor's msg-0x1044 rewind worker (`FUN_18005bac0` on
    // 20260721) — the training-mode seek's rebuild-trio anchor. The two
    // stores right after its step gate are unmistakable:
    //
    //   +0   MOV [RCX+0x160],RDX             ; the timing anchor
    //   +7   MOV dword [RCX+0x190],0xFFFFFFFF
    //
    // The +0x160/+0x190 displacements + the -1 imm make the pair unique;
    // the whole region through the three rebuild calls is byte-identical
    // on 20260616/20260721 (only a rip-disp32 inside differs). From the
    // match, the first three CALL rel32 sites are the judge-record trio —
    // clear(vec@actor+0xB0) / reserve(vec, count) / rebuild(out, begin,
    // end, &{actor, playhead}) — the flash-renderer virtual call between
    // is `FF 50 10`, never E8. Consumed by derive_judge_rebuild_trio.
    SignatureDefinition {
        name: "judge_rebuild_anchor",
        pattern: "48 89 91 60 01 00 00 C7 81 90 01 00 00 FF FF FF FF",
        description: "msg-0x1044 rewind worker's anchor stores (MOV [this+0x160],tick + MOV [this+0x190],-1). The first three CALL rel32s after the match are the judge-record rebuild trio (clear/reserve/rebuild), derived as judge_rebuild_clear/reserve/rebuild for seek-to-T.",
    },

    // FlareGaugeActor ctor field-init tail (`FUN_180075490` on 20260721,
    // right after the base GaugeActor ctor CALL) — a pure LAYOUT
    // ATTESTATION for the in-place reset's flare-state restore. Every
    // flare-specific offset the restore touches is pinned as a literal
    // disp32 in this run of stores:
    //
    //   33 C0                      XOR  EAX,EAX
    //   89 9F E8000000             MOV  [RDI+0xE8],EBX      ; side
    //   48 8B 5C 24 ??             MOV  RBX,[RSP+disp]
    //   89 87 E4000000             MOV  [RDI+0xE4],EAX      ; good-judge streak
    //   48 89 87 EC000000          MOV  [RDI+0xEC],RAX      ; per-grade judge
    //   48 89 87 F4000000          MOV  [RDI+0xF4],RAX      ;   history counters
    //   48 89 87 FC000000          MOV  [RDI+0xFC],RAX      ;   (8 dwords,
    //   48 89 87 04010000          MOV  [RDI+0x104],RAX     ;   0xEC..0x108)
    //   48 B8 1027000010270000     MOV  RAX,0x2710_00002710
    //   C6 87 E0000000 00          MOV  byte [RDI+0xE0],0   ; course-carry flag
    //   4C 8D 1D ????????          LEA  R11,[vftable]       ; rip disp32
    //   4C 89 1F                   MOV  [RDI],R11
    //   48 89 87 0C010000          MOV  [RDI+0x10C],RAX     ; per-level gauge
    //                                                       ;   array head
    //                                                       ;   (11 dwords of
    //                                                       ;   10000)
    //
    // NOTE the class-name/ctor swap vs the run_state_re.md §5 table: the
    // REAL FlareGaugeActor is the 0x138-byte class built for gauge
    // options 1..0xB (1 = FLOATING, 2..10 = FLARE I..IX, 0xB = FLARE EX);
    // option 0xE builds the 0xE8-byte GradeGaugeActor. The CURRENT flare
    // level does NOT live on the actor — it is ddr::player::Option+0x7C
    // (setter vt+0x1A0 / getter vt+0x310, plain field accessors), reached
    // via the derived player_option_table. Verified to match exactly once
    // on 20260324 / 20260421 / 20260526 / 20260616 / 20260721; MISSES on
    // 20250805 by design (older layout, no course-carry fields — 2025
    // builds are unsupported). Fail-open: unresolved ⇒ song_reset refuses
    // in-place resets whenever a FlareGaugeActor is live, and the caller's
    // scene-jump fallback (which re-runs onSetup) restores flare state
    // the slow way.
    SignatureDefinition {
        name: "flare_gauge_ctor_layout",
        pattern: "33 C0 89 9F E8 00 00 00 48 8B 5C 24 ?? 89 87 E4 00 00 00 48 89 87 EC 00 00 00 48 89 87 F4 00 00 00 48 89 87 FC 00 00 00 48 89 87 04 01 00 00 48 B8 10 27 00 00 10 27 00 00 C6 87 E0 00 00 00 00 4C 8D 1D ?? ?? ?? ?? 4C 89 1F 48 89 87 0C 01 00 00",
        description: "FlareGaugeActor ctor field-init tail — layout attestation for song_reset's floating-flare restore (streak +0xE4, per-grade history counters +0xEC..+0x108, per-level array +0x10C..+0x134, side +0xE8). Presence attests the 2026 flare layout; the address itself is unused.",
    },

    // GradeGaugeActor ctor field-init tail (`FUN_180075270` on 20260721,
    // option 0xE) — layout attestation for song_reset's grade-watermark
    // reset, same shape as flare_gauge_ctor_layout:
    //
    //   4C 8D 1D ????????                LEA  R11,[vftable]     ; rip disp32
    //   C7 83 E0000000 00 00 00 80       MOV  dword [RBX+0xE0],0x80000000
    //   4C 89 1B                         MOV  [RBX],R11
    //
    // +0xE0 is the best-EX-score watermark (ctor INT_MIN sentinel): the
    // grade calcJudgePoint (FUN_180075360) multiplies the miss penalty
    // while the current EX score has not grown past it, and clamps /
    // rewrites it on every judge. It survives an in-place reset (EX
    // score restarts at 0, watermark keeps the pre-reset value) —
    // early misses on the restarted run get over-penalized until the
    // first good judge rewrites it. The reset writes the ctor sentinel
    // back. Verified to match exactly once on 20260324 / 20260421 /
    // 20260526 / 20260616 / 20260721; fail-open like the flare AOB
    // (unresolved ⇒ resets refuse while a GRADE gauge is live).
    SignatureDefinition {
        name: "grade_gauge_ctor_layout",
        pattern: "4C 8D 1D ?? ?? ?? ?? C7 83 E0 00 00 00 00 00 00 80 4C 89 1B",
        description: "GradeGaugeActor ctor field-init tail (vftable store + best-EX watermark seed [this+0xE0] = 0x80000000) — layout attestation for song_reset's grade-watermark reset. Presence attests the offset + sentinel; the address itself is unused.",
    },

    // ── CRT functions ───────────────────────────────────────────────
    // MSVC's operator new — statically linked CRT, identical bytes across game versions.
    // Complete function from prologue to RET: retry loop calling _malloc_base then _callnewh.
    SignatureDefinition {
        name: "game_malloc",
        pattern: "53 48 83 EC 40 48 8B D9 EB ?? 48 8B CB E8 ?? ?? ?? ?? 85 C0 74 ?? 48 8B CB E8 ?? ?? ?? ?? 48 85 C0 74 ?? 48 83 C4 40 5B C3",
        description: "MSVC operator new (CRT malloc). Takes size in RCX, returns pointer. Uses HeapAlloc on the game's CRT heap.",
    },
    // ── AGCS heap allocator (app heap — NOT CRT) ─────────────────────
    // Used by the game's allocator-aware STL containers and any agcs::*::new
    // allocation. Distinct from game_malloc (CRT). Memory allocated here must
    // be freed via agcs_heap_free — mixing allocators causes heap mismatch
    // crashes.
    //
    // Byte 30 is wildcarded: MSVC emits either `4C 8B D8` (REX.R MOV RBX,R8)
    // or `49 8B D8` (REX.B MOV RBX,R8) depending on toolchain version — same
    // instruction, different encoding. The wildcard covers both.
    SignatureDefinition {
        name: "agcs_heap_malloc",
        pattern: "48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 48 89 7C 24 20 41 54 48 83 EC 20 48 8B 01 ?? 8B D8 48 8B F2 48 8B F9 FF 50 20 4C 8B 1F",
        description: "AGCS heap allocator entry point. Signature: fn(heap_handle, size, align, _unused) -> *mut u8. Prepends a 0x20-byte tracking header; pair with agcs_heap_free.",
    },
    // Two bytes wildcarded: the function reads the tracking header at [ptr-0x18]
    // and the heap-object vtable at [ptr]. Depending on toolchain version the
    // compiler keeps ptr in RCX (48 8B 59 E8, 48 8B 07) or moves RCX → RDI first
    // (48 8B 5F E8, 48 8B 01). Same behavior either way; the wildcards cover
    // both.
    SignatureDefinition {
        name: "agcs_heap_free",
        pattern: "48 83 EC 28 48 85 C9 74 ?? 48 89 5C 24 30 48 8B ?? E8 48 89 7C 24 20 48 8B 79 E0 48 8B CF 48 8B ?? FF 50 20",
        description: "AGCS heap free. Signature: fn(ptr) — reads tracking header at ptr-0x18/ptr-0x20 to locate heap. Pair with agcs_heap_malloc.",
    },
    // ── App-heap-allocated std::vector<T>::reserve (stride 12) anchor ────
    // Used only as a landmark to derive app_heap_handle (via MOV RCX,[RIP+disp32]
    // at +0x7B) and cross-check agcs_heap_malloc (via CALL at +0x82). The 12-byte
    // stride is identified by the 0x1555555555555555 = SIZE_MAX/12 overflow check
    // constant, which is unique to 12-byte-element reserve functions.
    SignatureDefinition {
        name: "app_heap_reserve_anchor",
        pattern: "41 54 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 40 48 89 74 24 50 48 89 7C 24 58 4C 8B C2 48 8B D9 48 B8 55 55 55 55 55 55 55 15",
        description: "std::vector<T>::reserve for 12-byte-stride T (Measure etc). Landmark for deriving app_heap_handle + agcs_heap_malloc.",
    },
    // ── step::IStepReader::Analyze (post-parse hook point) ─────────────
    // Non-virtual member function on step::IStepReader (concrete runtime
    // type SsqReader per RTTI).
    //   RCX = this (reader pointer)
    //   RDX = per-note-record vector pointer
    //   R8  = per-measure-record vector pointer
    //   R9  = optional result-struct pointer (may be null)
    //   [RSP+0x28] = groove-radar struct pointer
    //   [RSP+0x30] = mode (i32)
    //   [RSP+0x38] = difficulty (i32)
    // Reader member layout: data-blob pointer at +0x08, blob size at +0x10.
    SignatureDefinition {
        name: "step_reader_analyze",
        pattern: "4C 89 4C 24 20 55 53 56 57 41 54 41 55 41 56 48 8D 6C 24 F9 48 81 EC F0 00 00 00",
        description: "step::IStepReader::Analyze (public non-virtual) prologue. Hook point for post-parse mine injection.",
    },
    // GamePlayActor::judgeNotes submits a judgment result for a single
    // note via this helper. Called with:
    //   RCX = GamePlayActor*
    //   RDX = per-active-note result-record pointer
    //   R8D = judge code
    //   R9  = scratch pointer
    // Judge codes observed from the call sites inside judgeNotes:
    //   0x1028+grade  -- normal grade judgment (grade 0..5)
    //   0x102d        -- MISS
    //   0x1030        -- shock MISS (player stepped on shock)
    //   0x1031        -- shock NG (the result's grade dword at +0xC is
    //                    pre-set to 7 before the call)
    //   0x1046        -- cancel / reset
    // Mines reuse the shock-NG code path (0x1031 + grade=7) which already
    // drives combo break, NG display, and life gauge damage via event
    // dispatch. Prologue is distinctive: 4-push frame, stack cookie load,
    // then MOVZX [RCX+0x1e8] — the judgment-suppression byte on
    // GamePlayActor.
    SignatureDefinition {
        name: "judge_submit",
        pattern: "55 57 41 55 41 56 48 8B EC 48 83 EC 78 48 8B 05 ? ? ? ? 48 33 C4 48 89 45 D0 48 8B 02 48 89 74 24 68 48 8B F9 80 38 02 0F B6 89 E8 01 00 00",
        description: "Judgment submitter. Takes (actor, result, judge_code, scratch); dispatches score/gauge/display updates.",
    },
    // ComboActor digit-refresh (s-marvelous combo tint/digits). Prologue
    // anchored; the inline tint-immediates run (marvelous pair
    // 0xA9FEEC/0xDFA6EF written to locals) pins uniqueness. RIP disp of the
    // security-cookie load wildcarded. Event-driven (init + combo-changed
    // msg with combo >= 4) — never per-frame.
    SignatureDefinition {
        name: "combo_digit_refresh",
        pattern: "48 8B C4 55 41 54 41 55 41 56 41 57 48 8D 68 98 48 81 EC 40 01 00 00 48 C7 44 24 50 FE FF FF FF 48 89 58 10 48 89 70 18 48 89 78 20 0F 29 70 C8 0F 29 78 B8 48 8B 05 ? ? ? ? 48 33 C4 48 89 45 18 48 89 4C 24 38 C7 45 F8 EC FE A9 00 C7 45 FC EF A6 DF 00",
        description: "sequence::dance::ComboActor digit refresh (this in RCX). Repaints digit art per layer/place + applies the per-grade tint pairs.",
    },
    // NoteResultActor msg handler (`FUN_18007B300` on 20260721), grade case
    // 0x1028..0x102D: the FAST/SLOW indicator's show/hide gate. After the
    // judgement word is driven, the `dance_fast_slow` clip (this+0xA8) is
    // HIDDEN when either the ms delta (this+0x98) is 0 or the grade
    // (this+0x94) is 0 = Marvelous; otherwise shown at in_fast/in_slow:
    //
    //   83 BF 98 00 00 00 00   CMP dword [RDI+0x98], 0   ; delta == 0 → hide
    //   74 ??                  JZ  hide
    //   83 BF 94 00 00 00 00   CMP dword [RDI+0x94], 0   ; grade == 0 → hide
    //   74 ??                  JZ  hide
    //
    // The s-marvelous mod rewrites the second CMP's imm8 (match+15, `00` →
    // `FF`): grade is 0..=5 in this branch so `grade == -1` never holds and
    // the JZ is never taken — Marvelous judgements show FAST/SLOW like every
    // other grade (the delta==0 hide stays — an exactly-on-time step is
    // neither). Single-byte store, no tearing. Both JZ rel8s wildcarded
    // (7B/72 on 20250805/20260616/20260721/20260825, but structurally
    // free). Unique single match, byte-identical on all four.
    SignatureDefinition {
        name: "note_result_fast_slow_gate",
        pattern: "83 BF 98 00 00 00 00 74 ?? 83 BF 94 00 00 00 00 74 ??",
        description: "NoteResultActor grade-case FAST/SLOW gate — CMP [RDI+0x98],0; JZ; CMP [RDI+0x94],0; JZ. S-Marvelous rewrites the grade CMP imm8 at match+15 (0→-1) so Marvelous shows FAST/SLOW.",
    },
    // FullcomboActor::onMessage (s-marvelous FC splash). Prologue-anchored;
    // the `CMP EDX,0x1034` (its only handled message) pins uniqueness —
    // `81 FA 34 10 00 00` is module-unique on 20260721.
    SignatureDefinition {
        name: "fullcombo_actor_on_message",
        pattern: "40 55 56 57 48 83 EC 60 48 C7 44 24 30 FE FF FF FF 48 89 9C 24 80 00 00 00 0F 29 74 24 50 0F 29 7C 24 40 49 8B F0 48 8B D9 81 FA 34 10 00 00 0F 85",
        description: "sequence::dance::FullcomboActor message handler (this, msg, payload). Handles only 0x1034: SE + splash label goto + play/visible.",
    },
    // PlaydataTab populate/update (s-marvelous results score tab, vslot 7 —
    // runs every frame while a judgement-count tab is visible; the heavy
    // populate is gated on the dirty byte this+0x151, consumed at its
    // start). Prologue-anchored; the giant frame (SUB RSP,0xB70) plus the
    // 0x151/0x110 field reads pin uniqueness — the string
    // "marvelous_num_usr" has exactly one xref, inside this fn
    // (0x1800F6BC0 on 20260721). Security-cookie RIP disp wildcarded.
    SignatureDefinition {
        name: "playdata_tab_update",
        pattern: "48 8B C4 55 41 54 41 55 41 56 41 57 48 8D A8 68 F5 FF FF 48 81 EC 70 0B 00 00 48 C7 45 50 FE FF FF FF 48 89 58 10 48 89 70 18 48 89 78 20 48 8B 05 ? ? ? ? 48 33 C4 48 89 85 60 0A 00 00 4C 8B E1 4C 8B 81 10 01 00 00",
        description: "sequence::result::PlaydataTab populate/update (this). Populates the judgement-count rows behind the +0x151 dirty byte, then lays out every widget in this+0x158 per frame.",
    },
    // PlaydataTab row-write helper (s-marvelous results score tab): builds a
    // sequence::SpriteLayer number widget — make_shared, glyph conversion
    // via "scre_tab_num_%s", parent = ctx wrapper, anchor-name assign,
    // set-names, then PUSHES it into the tab's widget vector (tab+0x158) so
    // the game owns layout + destruction. (ctx, out_shared_ptr, name_str,
    // text_str) -> out_shared_ptr; ctx = {wrapper*, tab*} pair
    // (0x1800F8370 on 20260721). Prologue-anchored, unique on the frame
    // size + homing sequence.
    SignatureDefinition {
        name: "playdata_row_write",
        pattern: "40 53 55 56 57 41 54 41 55 48 81 EC 98 00 00 00 48 C7 44 24 50 FE FF FF FF 48 8B 05 ? ? ? ? 48 33 C4 48 89 84 24 80 00 00 00 49 8B D9 49 8B F0 48 8B EA 4C 8B E1 48 89 54 24 48 45 33 ED 44 89 6C 24 20",
        description: "PlaydataTab row-write helper (ctx {wrapper,tab}, out shared_ptr, anchor-name string, text string). Creates a SpriteLayer row widget and pushes it into tab+0x158.",
    },
    // GraphTab per-frame rebuild (s-marvelous judgement graph, vslot 7 —
    // clears + rebuilds all charts/legend texts every frame). Prologue
    // anchored; the giant 0x12E0 chkstk frame pins uniqueness (verified
    // exactly-once on 20260721 @0x1800ED610 and 20260616 @0x1800ED1B0).
    // The chkstk call rel32 is wildcarded.
    SignatureDefinition {
        name: "graph_tab_rebuild",
        pattern: "40 55 41 54 41 55 41 56 41 57 48 8D AC 24 20 EE FF FF B8 E0 12 00 00 E8 ? ? ? ? 48 2B E0 48 C7 85 E8 07 00 00 FE FF FF FF",
        description: "sequence::result::GraphTab rebuild (this). Rebuilds charts (tab+0x178) and legend texts (tab+0x1A0) per frame while the tab is visible.",
    },
    // Chart single-color series append (s-marvelous judgement graph):
    // (chart, vector<double>*, callable {vft, rgba u32, pad, impl_ptr}) —
    // DEEP-COPIES the data, CONSUMES the callable. Unique on 20260721
    // @0x1801CFF60 / 20260616 @0x1801CF410; cookie disp wildcarded.
    SignatureDefinition {
        name: "graph_chart_append",
        pattern: "40 55 53 56 57 41 54 48 8D 6C 24 C9 48 81 EC 00 01 00 00 48 C7 45 E7 FE FF FF FF 48 8B 05 ? ? ? ? 48 33 C4 48 89 45 2F 49 8B F8 48 8B F1",
        description: "Graph chart series append (chart, &vector<double>, &color callable). Copies the series data into the chart; the callable supplies the bar color.",
    },
    // GraphTab legend text helper (s-marvelous judgement graph):
    // (ctx {rect block*, cursor*, tab*}, &string, rgba) — creates a scaled
    // 0.6 text object, tints it, pushes into tab+0x1A0, advances the
    // cursor by the text width. Unique on 20260721 @0x1800F15E0 /
    // 20260616 @0x1800F1180 (exact bytes, no relocs in the window).
    SignatureDefinition {
        name: "graph_legend_text",
        pattern: "48 8B C4 55 57 41 54 48 8D 68 A1 48 81 EC 90 00 00 00 48 C7 45 E7 FE FF FF FF 48 89 58 10 48 89 70 18 41 8B D8 4C 8B E1 48 8B 41 08 44 8B 08 41 FF C1",
        description: "GraphTab legend-line helper (ctx, string, rgba). Appends one colored legend text to the tab and advances the layout cursor.",
    },
    // Results window build (s-marvelous FC emblems, Step 9 — runs ONCE at
    // results-scene build). Drives the per-stage clear-kind emblem: suffix
    // from the DAT_180486410 table ([10]="mfc"), refer
    // "player_%dp_info_usr/fc_usr", `afp_mc_op(mc, 0xF09, "loop_"+suffix)`.
    // Prologue-anchored; the frame displacement −0xA18 + SUB RSP,0xAF0 +
    // the 0x200 EH slot pin uniqueness — verified exactly-once on 20260721
    // @0x1800B8AA0 AND 20260616 @0x1800B88A0, byte-verified in the on-disk
    // cabinet DLL (the "scre_rank_%s" string's only xref is inside).
    // Security-cookie RIP disp (past the pattern) excluded.
    SignatureDefinition {
        name: "result_window_build",
        pattern: "48 8B C4 55 41 54 41 55 41 56 41 57 48 8D A8 E8 F5 FF FF 48 81 EC F0 0A 00 00 48 C7 85 00 02 00 00 FE FF FF FF 48 89 58 10 48 89 70 18 48 89 78 20",
        description: "sequence::result results-window builder (this). One-shot scene build: rank/emblem/flare bitmaps + fc_usr loop_<kind> label goto per side.",
    },
    // Total-results populate (s-marvelous FC emblems, Step 9). Builds the
    // per-stage "total_result" pane layers (actor+0x1B0+pane*8) and loads
    // the clear-kind badge bitmap "scre_total_player_%s" (suffix table
    // DAT_180486E80, [10]="fc_mfc") into the fullcombo_usr leaves under
    // total_p%d_top_usr. Prologue-anchored; frame displacement −0x5A8 +
    // SUB RSP,0x680 + the XMM spill run pin uniqueness — verified
    // exactly-once on 20260721 @0x1800CB090 AND 20260616 @0x1800CB170,
    // byte-verified in the on-disk cabinet DLL (anchors: the only
    // "total_result" / "fullcombo_usr" xrefs are inside).
    SignatureDefinition {
        name: "total_result_populate",
        pattern: "48 8B C4 55 41 54 41 55 41 56 41 57 48 8D A8 58 FA FF FF 48 81 EC 80 06 00 00 48 C7 85 40 02 00 00 FE FF FF FF 48 89 58 10 48 89 70 18 48 89 78 20 0F 29 70 C8 0F 29 78 B8",
        description: "sequence::result total-results populate (this). Builds per-stage panes and loads the per-side clear-kind badge into fullcombo_usr.",
    },
    // CalcCalorieActor per-frame tick (vtable slot 6, shared Single/Double).
    // Reads the current measurement-window index (+0x92) and its closed flag
    // (+0x68 + idx*8); when closed, calls vtable slot 9 (+0x48) for the
    // per-window kcal increment and accumulates it into the running per-stage
    // kcal total at +0x94. PowerUserStatistics detours this to cache the live
    // per-side kcal for the realtime-calorie display. Body is byte-identical on
    // 20260324 (@0x180053a50) and 20260616 (@0x180053470); the two short-jump
    // displacements are wildcarded. See docs/calorie_weight_profile_research.md §3.1.
    SignatureDefinition {
        name: "calc_calorie_tick",
        pattern: "40 53 48 83 EC 20 0F B7 91 92 00 00 00 48 8B D9 8B 54 D1 68 FF CA 75 ?? 48 8B 01 FF 50 48 85 C0 74 ?? 01 83 94 00 00 00",
        description: "CalcCalorieActor per-frame tick (this). Accumulates per-window kcal into actor+0x94 (the running per-stage calorie total).",
    },
    // ── Training Mode strip HUD (chart-strip timeline, Step 6) ───────
    // The tap quantization→palette-row selector called from the arrow
    // fill (`FUN_180028130` @ 20260616 ≡ `0x180027d10` @ 20260721 ≡
    // `0x180027650` @ 20260324): reads the color-option field at
    // renderer+0xE8 — values 0/5 select beat-DIVISION rows (4th/8th/16th
    // = rows 1/3/2, else 4), anything else the beat-CYCLING mode. The
    // strip synthesis CALLS it with the live ArrowRenderer so a future
    // quantization-granularity hack propagates for free (maintainer
    // constraint — never replicate its math). Signature: fastcall
    // `u32 selector(ArrowRenderer* rcx, i32 beat edx)` — a pure leaf
    // (arithmetic + one field read), safe on the game thread. The body
    // is fully position-independent; this 44-byte head matches UNIQUELY
    // and byte-identically on 20260324/20260616/20260721. RE:
    // docs/chart_strip_hud_research.md §4.
    SignatureDefinition {
        name: "arrow_row_selector",
        pattern: "44 8B 81 E8 00 00 00 8B C2 32 D2 25 FF 03 00 00 45 85 C0 0F B6 CA 41 B9 01 00 00 00 41 0F 44 C9 41 83 F8 05 0F B6 D1 41 0F 44 D1 84 D2",
        description: "Quantization -> palette-row selector (ArrowRenderer* this, i32 beat) -> row 1..4. Called per note by the strip synthesis with the live renderer (never replicated).",
    },
    // ── File / Resource Manager (texture loading) ────────────────────
    // Used by NoteTypesExpansion to load mine PNG textures via the
    // engine's file pipeline (agcs::FileManager dispatches to
    // PngFileCallback, which registers the texture in the resource
    // system). The FileManager singleton pointer is derived from
    // xrefs to file_manager_load.
    SignatureDefinition {
        name: "file_manager_load",
        pattern: "40 53 56 57 48 81 EC F0 00 00 00 48 8B 05 ? ? ? ? 48 33 C4 48 89 84 24 E0 00 00 00 48 8B F1 48 8D 4C 24 40",
        description: "agcs::FileManager member that loads a file by path and returns an i32 handle. Dispatches to registered callbacks by file extension (PngFileCallback for .png). Async — handle is valid immediately but resource registration completes on a worker thread.",
    },
    // The Free counterpart to file_manager_load: enqueues a loaded file's
    // table index onto the same FileManager's release queue (member +0x98),
    // guarded by the identical +0x150/+0x154 busy lock the loader uses. The
    // engine drains the queue on a worker thread; for a registered PNG this
    // fires the callback's OnDetach, which calls ReleaseTextureData(stem).
    // Load is refcounted (loading an already-resident file bumps its +0x24
    // refcount instead of re-reading), so each load handle pairs with exactly
    // one free. Signature: (FileManager* /*RCX*/, i32 index /*EDX*/) -> void.
    // Used by asset_loader to evict on-demand preview textures.
    SignatureDefinition {
        name: "file_manager_free",
        pattern: "48 89 5C 24 08 48 89 74 24 18 89 54 24 10 57 48 83 EC 20 48 8B F9 8B 89 50 01 00 00 8B F2 85 C9 7E 06",
        description: "agcs::FileManager member that releases a file by its i32 handle (the index returned by file_manager_load). Enqueues the index onto the manager's release queue (+0x98); the engine drains it async, firing PngFileCallback::OnDetach → ReleaseTextureData(stem) for registered textures. Args: (this, index).",
    },
    SignatureDefinition {
        name: "resource_manager_get_texture_hash_value",
        pattern: "48 89 5C 24 10 48 89 74 24 18 57 48 83 EC 70 48 8B 05 ? ? ? ? 48 33 C4 48 89 44 24 60 48 8B F1 33 C0 48 83 C9 FF",
        description: "Hashes a texture name string (lowercase + strip underscores + FNV) and returns a u32 hash. Static function — no this pointer. The hash vtable is at a global resolved via the indirect CALL at the function's tail.",
    },
    SignatureDefinition {
        name: "resource_manager_get_texture_data",
        pattern: "48 89 5C 24 08 57 48 83 EC 20 48 8B 1D ? ? ? ? 8B F9 8B 8B 50 01 00 00 85 C9 7E 06 FF 15 ? ? ? ? FF 83 54 01 00 00 48 8B 8B B8 00 00 00 8B 93 54 01 00 00 48 8B 41 08 80 78 41 00 75 17",
        description: "Looks up a gs::TextureData pointer by hash. Returns null if the hash is not registered. Loads the ResourceManager singleton from a global at the function's first RIP-relative MOV.",
    },
    // ── BM2D data manager (on-demand arc → package load) ─────────────
    // `bm2d::data::(anonymous namespace)::Manager` (name from its own log
    // string) owns the registry of loaded BM2D packages (the CFileData
    // vector global, `DAT_1806f1d68` on 20260526 / `DAT_1806ebce8` on
    // 20260324) and the on-demand load path the game itself uses for
    // backgrounds, HUD clips, etc. Used by the background preview overlay
    // to load `background_%04d.arc` packages on demand. All three entry
    // points verified byte-identical (single match) on both supported
    // builds. See `.agents/planning/20260708-background-preview-overlay/
    // progress.md` for the full Ghidra derivation.
    //
    // Request-load: `bool f(const char* dir /*e.g. "custom/background"*/,
    // const char* name /*e.g. "background_0001"*/, u32 flag /*game: 0*/)`.
    // Dedups by name against the registry, resolves the arc-name variant
    // (`%s_v3` → `%s_lite` → `%s` — machine-type-gated), opens the arc via
    // the arc manager and appends a registry entry with a NULL package ptr.
    // The manager's per-frame Update (pumped by the engine main loop)
    // creates the package once every queued arc finishes loading.
    SignatureDefinition {
        name: "bm2d_data_request_load",
        pattern: "48 8B C4 55 57 41 54 41 55 41 56 48 8D 68 A1 48 81 EC E0 00 00 00 48 C7 45 97 FE FF FF FF 48 89 58 18 48 89 70 20 48 8B 05 ? ? ? ? 48 33 C4 48 89 45 27 45 8B E0 48 8B F2 4C 8B E9 48 8B 3D",
        description: "bm2d::data Manager request-load. Args: (dir, name, flag). Non-blocking: queues the arc + appends a pending registry entry; poll bm2d_data_is_ready for completion.",
    },
    // Is-ready: `bool f(const char* name)` — true once the registry entry
    // exists AND its package pointer is non-null. Also the anchor for two
    // derived addresses: the registry global (RIP-decoded from the
    // `MOV RAX,[rip+disp32]` at +6, disp at +9) and the name-lookup helper
    // (`Entry* lookup(Entry* begin, Entry* end, const char* name)`,
    // decoded from the `CALL rel32` at +26). The final instruction
    // `MOV RCX,[RAX+disp8]` reads the entry's package pointer — its disp8
    // (at +39, wildcarded; 0x30 on both 2026 builds) is the entry-layout
    // offset `bm2d_package::init` reads from the matched bytes, so a
    // layout-only change survives instead of desyncing.
    SignatureDefinition {
        name: "bm2d_data_is_ready",
        pattern: "40 53 48 83 EC 20 48 8B 05 ? ? ? ? 4C 8B C1 48 8B 58 08 48 8B 08 48 8B D3 E8 ? ? ? ? 48 3B C3 74 10 48 8B 48 ?",
        description: "bm2d::data Manager is-ready check. Args: (name) -> bool. Anchor for bm2d_package_registry + bm2d_package_lookup derivation + the entry package-pointer offset (disp8 at +39).",
    },
    // Release: `void f(const char* name)` — destroys the package and erases
    // the registry entry. Only dynamic entries (index >= 72) are erased;
    // the manager's 72 permanent common entries are protected, so releasing
    // an on-demand background is always safe.
    SignatureDefinition {
        name: "bm2d_data_release",
        pattern: "40 53 41 54 41 56 48 83 EC 30 4C 8B 25 ? ? ? ? 4C 8B F1 49 8B 1C 24 49 3B 5C 24 08 0F 84 ? ? ? ? 48 89 6C 24 50",
        description: "bm2d::data Manager release-by-name. Args: (name). Destroys the package and erases the registry entry (dynamic entries only).",
    },
    // ── Arrow render pipeline (mine render pass) ─────────────────────
    // The outer per-frame note-rendering member on the arrow renderer
    // object. Runs the shock-arrow pass (silver glyph + lightning overlay)
    // then the normal-arrow pass. Hook target for appending a dedicated
    // mine pass after both vanilla passes complete.
    SignatureDefinition {
        name: "render_notes",
        pattern: "48 8B C4 55 53 56 57 41 54 41 55 41 56 41 57 48 8D A8 28 FF FF FF 48 81 EC 98 01 00 00",
        description: "Arrow renderer's per-frame note draw function. RCX = ArrowRenderer this. Calls the per-pass note collector twice (shock then normal) and emits sprite batches via inlined CommandList writes.",
    },
    // The per-frame LAYER DISPATCHER: iterates the 11-entry layer table
    // (global at the RIP disp32 at match+13, entries {override_ptr,
    // layer_object, list_index} stride 0x18), sets the ScreenRenderer
    // state's active-list index (+0x68 — its ONLY writer), and walks each
    // enabled layer (vtbl+0x28) to record its display quads. Called once
    // per frame unconditionally from the render orchestrator. The
    // overlay-draw animated-background emitter detours this and appends
    // its quad to the widget layer's list PRE-original so the quad sits
    // beneath the menu's own widgets but above every lower layer in every
    // scene. Verified unique on 20260721 (0x18002af10) and 20260616
    // (0x18002b530); the `LEA EDI,[RBX+0xB]` 11-count shortly after is the
    // structural confirmation. RE: docs/overlay_draw_research.md.
    SignatureDefinition {
        name: "layer_dispatcher",
        pattern: "48 89 5C 24 08 57 48 83 EC 60 48 8B 15 ? ? ? ? 4C 8B 05 ? ? ? ? 0F 29 74 24 50 0F 57 F6",
        description: "Per-frame layer dispatcher (walks the 11-entry layer table, writes the active-list index)",
    },
    // The receptor-row (spot) renderer's per-frame draw. Emits one
    // SetShader (the spot shader object @ this+0xA0) + one 4/8-quad
    // ROTATESPRITE batch (mode @ this+0x98) through the shared per-quad
    // fill. Hook target for the player-perspective mod's receptor pass
    // rewrite (the receptors span ~96 px of track depth, so the hallway
    // map foreshortens them like a note at the row).
    SignatureDefinition {
        name: "spot_render",
        pattern: "53 56 57 41 55 41 57 48 83 EC 60 48 8B 15 ?? ?? ?? ?? 83 B9 98 00 00 00 01",
        description: "SpotRenderer per-frame receptor draw. RCX = SpotRenderer this (ArrowSprite base: posX/posY @ +0x30/+0x34, mode @ +0x98, shader @ +0xA0).",
    },
    // JudgeEffectRenderer per-frame draw: ages the effect records
    // (vector @ +0xA0..+0xA8), then emits its OWN tag-0x13 SetShader
    // (shader object @ this+0x98, program hardcoded 0) + one quad batch
    // into the global command list. Draws BOTH the tap hit-burst and the
    // freeze-hold glow (arrow-sheet cells at the receptor row) — the pass
    // player_perspective rewrites to the judge container's perspective
    // program. Pattern = prologue + the two structural vector-field loads
    // (member offsets, no relocatable bytes); verified unique on 20260324
    // (0x1800279b0), 20260616 (0x180028490), 20260721 (0x180028070).
    SignatureDefinition {
        name: "judge_effect_render",
        pattern: "48 89 5C 24 10 57 48 83 EC 40 48 8B 99 A8 00 00 00 4C 8B 89 A0 00 00 00 48 8B F9",
        description: "JudgeEffectRenderer per-frame draw. RCX = renderer this (ArrowSprite base: posX/posY @ +0x30/+0x34, shader @ +0x98, records vector @ +0xA0/+0xA8).",
    },
    // The final overload of the per-sprite filler on the arrow sprite
    // base class. Takes explicit UV, rotation, and color — does not read
    // member UV/twist state. Handles appearance alpha, reverse, and
    // rotation math, then writes vertex positions + UV + color into a
    // ROTATESPRITE entry (0x34 bytes).
    SignatureDefinition {
        name: "render_sprite_final",
        pattern: "48 8B C4 53 48 81 EC C0 00 00 00 F3 0F 10 61 6C",
        description: "ArrowSprite per-quad filler (final overload). Args: (this, &sprite, x, y, w, h, &uv[4], twist, &color). Writes 0x34-byte ROTATESPRITE.",
    },
    // Sets the rotation angle on the arrow sprite base class from a
    // panel direction index (0=left, 1=down, 2=up, 3=right). Maps
    // direction to a quarter-turn twist value stored on the object.
    SignatureDefinition {
        name: "set_direction",
        pattern: "40 53 48 83 EC 30 45 33 C0 81 E2 03 00 00 80",
        description: "ArrowSprite direction setter. Args: (this, dir). Converts dir%4 to a rotation angle and stores it.",
    },
    // ── Playfield styling: lane clip capture (CMovieClip helpers) ────
    // Two generic CMovieClip helpers the playfield-styling mod detours to
    // scale the lane background + lane cover (both AFP-layer clips that do
    // NOT flow through render_sprite_final). Neither is Create'd by lane
    // name: the lane bg is a find-child of the gameplay `dance_root` movie;
    // the lane cover is created via a pool-slot wrapper around
    // CMovieClip::Create. Both are hooked directly (collision-free —
    // overlay-element-styling owns CMovieClip::Create itself, which these
    // bypass). Prologues verified unique on builds 20260616 + 20260324.
    SignatureDefinition {
        name: "cmovieclip_pool_create",
        pattern: "48 89 6C 24 10 48 89 74 24 18 48 89 7C 24 20 41 54 48 83 EC 30 41 8B E9 49 8B F8 4C 8B E2 48 8B F1",
        description: "CMovieClip pool-slot create-from-package wrapper (pool, package, name /*R8, C-string*/, priority, mode) -> clip slot (layer id at slot+0x08), or null. Wraps CMovieClip::Create. Playfield styling captures the lane cover (hidden_cover_* / sudden_cover_*) here.",
    },
    // NoteResultActor setup (RCX = actor). Creates the per-panel hit-flash
    // clips (`dance_effect`, via afp_layer_create_with_property — NOT through
    // Create/pool-create, so the other two hooks miss them) and stores them
    // in the actor's `vector<CMovieClip*>` at actor+0xE8 (begin) / +0xF0
    // (end); each element's AFP layer id is at clip+0x08. Play mode is at
    // actor+0x90 (0 = single/4 panels, else double/8). Playfield styling
    // hooks this to scale the receptor hit flashes. Prologue unique on builds
    // 20260616 (0x18007A230) + 20260324 (0x18007AF20).
    SignatureDefinition {
        name: "note_result_setup",
        pattern: "48 89 4C 24 08 53 55 56 57 41 54 41 55 41 56 41 57 48 83 EC 68 48 8B 81 88 00 00 00 48 8B E9",
        description: "NoteResultActor setup (this). Builds the judge/fast_slow/score_compare clips + the per-panel receptor hit-flash clips (dance_effect) into the actor's vector<CMovieClip*> @ +0xE8..+0xF0. Playfield styling walks that vector to scale the hit flashes.",
    },
    // Scroll-Y computation shared by the shock and normal render
    // passes. Args: (dBeatCount, speed, boost, musicCount) — speed is
    // stored on the arrow renderer as an integer (speed*100), so the
    // formula is: d = (dBeatCount * speed * 96) / 100, then the result
    // is adjusted by the boost/brake/wave enum.
    SignatureDefinition {
        name: "get_offset_y",
        pattern: "48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 20 66 0F EF C0 48 63 C2 41 8B F1 41 8B F8 48 0F AF C1 48 8D 04 40 48 C1 E0 05",
        description: "Scroll-Y pure function. Returns pixel offset from the spot row as a float.",
    },
    // ── Per-side player-work table anchor ────────────────────────────
    // A short accessor function whose first two instructions load the
    // global player-work table for the 1P and 2P sides back-to-back.
    // The two loads in sequence are distinctive enough to make this
    // anchor unique across the whole binary.
    //
    // Used for reading the player's Option struct (inlined on the
    // PlayerWork object at +0xE0). The Option's arrow-shape field sits
    // at +0x60 inside that inlined struct. The path from a gameplay
    // actor is:
    //     actor[+0x84]               = playSide (i32)
    //     player_work_table[playSide] = wrapper*
    //     *wrapper                   = PlayerWork*
    //     PlayerWork[+0xE0]          = Option (inlined)
    //     Option[+0x60]              = arrow_shape (i32 in 0..7)
    //
    // The first RIP-relative MOV (at offset +3 of the anchor) resolves
    // to the player-work table global; derived via
    // `derive_player_work_table` below.
    SignatureDefinition {
        name: "player_work_table_anchor",
        pattern: "48 8B 05 ? ? ? ? 48 8B 08 80 79 04 00 74 09 66 C7 05 ? ? ? ? 7F 04 48 8B 05 ? ? ? ? 48 8B 08 80 79 04 00 74 09 66 C7 05 ? ? ? ? 7F 04",
        description: "Accessor whose first two instructions load slots [0] and [1] of the per-side player-work table global. Landmark for deriving player_work_table via RIP-relative decode.",
    },
    // ── Custom player options framework ──────────────────────────────
    // Signatures for sequence::selectmusic subsystem hook points.
    // Research: docs/custom_player_options_research.md "Signatures — Verified Cross-Version".

    // Row-builder anchor. The prologue itself is not unique (shared shape with
    // 2 unrelated large functions), so we anchor on an internal landmark at
    // The 21-row OptionForm builder has a unique prologue: 5 callee-saved
    // registers + a large stack frame (LEA RBP,[RSP-0x1A40..0x1A20]) + a
    // __chkstk allocation of ~0x1B00..0x1B40. The `?? E5 FF FF B8` sequence
    // (LEA frame byte + SUB size prefix) is structurally unique across all
    // known game versions.
    SignatureDefinition {
        name: "row_builder_fn_prologue",
        pattern: "40 55 41 54 41 55 41 56 41 57 48 8D AC 24 ? E5 FF FF B8",
        description: "21-row OptionForm builder function entry (direct prologue match, all versions).",
    },

    // Tab-state re-renderer FUN_180168d10. Seven-register save + specific frame
    // sizes (0x260/0x360/0x78) are structurally unique to this function in both
    // builds. Prologue-anchored (match address IS function entry).
    SignatureDefinition {
        name: "tab_filter_fn",
        pattern: "40 55 56 57 41 54 41 55 41 56 41 57 48 8D AC 24 A0 FD FF FF 48 81 EC 60 03 00 00 48 C7 44 24 78 FE FF FF FF 48 89 9C 24 B0 03 00 00",
        description: "Tab-state re-renderer (FUN_180168d10). Detour target for Page6 filter reimplementation; iterates all rows and show/hides each based on its PageN metadata tag.",
    },

    // Metadata-set insert. Inserts a hashed std::string key into the
    // metadata-set inlined in an OptionElement at `+0x08..+0x28`. The game's
    // row builder calls this to tag each OptionElement with its `"PageN"`
    // category string. The 4 wildcards cover the internal CALL rel32; the
    // remaining 35 bytes are prologue + shadow-store + first few arg setup
    // moves, structurally invariant across compiler toolchain drift.
    //
    // Signature: fn(OptionElement* row, std::string* key) -> OptionElement*.
    // Calling convention: Microsoft x64 (RCX=row, RDX=key).
    // Side effect: hashes the std::string's contents (FNV-1a) and inserts
    // into the rb-tree at row+0x08.
    SignatureDefinition {
        name: "metadata_insert",
        pattern: "48 89 5C 24 10 57 48 83 EC 30 48 8B F9 48 8B CA E8 ? ? ? ? 48 8B 5F 18 48 8D 4F 08 4C 8D 44 24 40 48 8D 54 24 20",
        description: "OptionElement metadata-set insert. Signature: fn(OptionElement* row, std::string* key) -> OptionElement*. Hashes the key's contents (FNV-1a) and inserts into the rb-tree at row+0x08.",
    },

    // OptionTab row-register helper. Wraps a bare element pointer in a
    // shared_ptr and appends it to the flat row vector at
    // `(parent+0x230)+0x68`, then writes the scene-graph anchor back into
    // the row at `row+0x60` and dispatches the IResourceSharing cleanup
    // lambda for rows that implement that interface.
    //
    // The 30-byte prologue is structurally unique: no wildcards, no other
    // function in gamemdx dereferences a caller-supplied pointer-to-pointer
    // and reads `[+0x230]` this early in its prologue.
    //
    // Signature: fn(&parent_ptr: *mut *mut u8, row: *mut u8) -> *mut u8.
    // Calling convention: Microsoft x64 (RCX = address of a stack-local
    // holding the parent pointer; RDX = row).
    SignatureDefinition {
        name: "option_tab_register",
        pattern: "48 89 5C 24 10 48 89 74 24 18 57 48 83 EC 50 48 8B 01 48 8B FA 48 8B F1 48 8B 98 30 02 00 00",
        description: "OptionTab row-register helper. Signature: fn(*mut *mut u8 parent_slot, *mut u8 row) -> *mut u8. Wraps row in shared_ptr and appends to the flat row vector at (*parent_slot + 0x230) + 0x68.",
    },

    // OptionForm destructor. Fires once per carded-in side when the options
    // overlay closes — the moment the game frees that side's option rows. The
    // custom_options framework detours it to drop its stale RowSlot pointers
    // (clear_side) before any +0xB8-writing path can dereference freed rows.
    //
    // The bare dtor prologue is the generic MSVC 3-vtable shape (shared with
    // 2 unrelated dtors), so the signature extends through the body: three
    // vtable-pointer writes, `ADD RCX, 0xC0`, the sub-object release CALL, then
    // `MOV RBX, [RSI+0x238]` (shared_ptr release of the +0x238 field) — that
    // tail is what makes it unique. Wildcards cover the 3 vtable-LEA disp32s and
    // the CALL rel32. Verified unique on both 20260526 (FUN_18018dda0) and
    // 20250805 stock (FUN_1801786b0); player side is read at OptionForm+0x228.
    SignatureDefinition {
        name: "optionform_dtor",
        pattern: "48 89 4C 24 08 57 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 48 48 89 6C 24 50 48 89 74 24 58 48 8B F1 48 8D 05 ? ? ? ? 48 89 01 48 8D 05 ? ? ? ? 48 89 41 28 48 8D 05 ? ? ? ? 48 89 81 C0 00 00 00 48 81 C1 C0 00 00 00 E8 ? ? ? ? 90 48 8B 9E 38 02 00 00",
        description: "OptionForm::~OptionForm. Signature: fn(this). Fires per carded-in side on options-overlay close; player side at this+0x228. Detoured by custom_options to invalidate stale row pointers.",
    },

    // Component visibility toggle. Used by the tab-filter detour's Phase 3
    // to toggle per-row visibility on tab switch. Writes the visible byte
    // to row+0xB8 (the isActive flag) and propagates to children via the
    // child vector at row+0x68..+0x70.
    //
    // Signature: fn(Component* row, bool visible).
    // Calling convention: Microsoft x64 (RCX=row, DL=visible).
    SignatureDefinition {
        name: "component_set_visible",
        pattern: "48 89 5C 24 10 57 48 83 EC 20 0F B6 FA 48 8B 51 60 48 8B D9 48 85 D2",
        description: "Component visibility toggle. Signature: fn(Component* row, bool visible). Writes visible to row+0xB8 and propagates to children.",
    },

    // Event-callback registration. Native slot-4 (`FUN_180173c10`,
    // `advanceValue`) calls a sibling (`FUN_18017dc40`) that registers four
    // per-direction lambdas against the current input event via repeated
    // calls to this function. The dispatcher (`FUN_180048a90`) also calls
    // this function directly for navigation types (3=up, 4=down). For every
    // input event, the engine calls this function once per registered
    // handler with `(event_obj, type, &lambda)`; only the type matching the
    // event currently in flight fires its lambda. Mod rows reuse this
    // mechanism so left/right are type-gated exactly like native rows.
    //
    // Signature: fn(event_obj: *mut u8, event_type: i32, lambda: *mut u8).
    //   RCX = event_obj (stack-local from the dispatcher; same pointer across
    //         every call in a given dispatch window)
    //   EDX = event_type (1=left, 2=right, 3=up, 4=down, 0=stop)
    //   R8  = lambda (std::tr1::_Impl_no_alloc0 frame, 32 bytes on stack or
    //         16 bytes on the CRT heap, first qword = vtable pointer)
    //
    // 38-byte prologue: MOV [RSP+0x18],R8; PUSH RSI; SUB RSP,0x50;
    // MOV [RSP+0x20], -2; MOV [RSP+0x68],RBX; MOV RBX,R8; MOV R8D,EDX;
    // MOV RSI,RCX; CMP [RCX+0x10], 1; JZ +5 byte. No wildcards needed;
    // every byte is structurally mandated by the calling convention and
    // the control-flow shape. Verified unique on 20260324 and 20250805.
    SignatureDefinition {
        name: "event_register",
        pattern: "4C 89 44 24 18 56 48 83 EC 50 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 68 49 8B D8 44 8B C2 48 8B F1 48 83 79 10 01",
        description: "Event-callback registration (FUN_180045b70). Signature: fn(event_obj: *mut u8, event_type: i32, lambda: *mut u8). Registers a type-gated lambda against the current input event when the Start-modifier is NOT held (event_obj+0x10 == 1). Used by enum rows and by scalar-row fine-step lambdas.",
    },

    // Twin of event_register used by scalar rows to register the coarse-step
    // (Start-held) variant of left/right advance lambdas. Identical registration
    // shape to event_register, but gated on event_obj+0x10 == 2 instead of 1.
    // The third parameter is an auxiliary predicate that the native scalar
    // slot-4 (FUN_180162680) always passes as 0; same behavior is fine for mod
    // rows.
    //
    // 38-byte prologue with no wildcards: two shadow-store MOVs, SUB RSP 0x50,
    // MOV [RSP+0x28], -2, two register saves, three register copies
    // (RBX/R9D/RDI), CMP [RCX+0x10], 2. Verified unique on 20260324 and 20250805.
    SignatureDefinition {
        name: "event_register_no_consume",
        pattern: "4C 89 4C 24 20 57 48 83 EC 50 48 C7 44 24 28 FE FF FF FF 48 89 5C 24 68 49 8B D9 44 8B CA 48 8B F9 48 83 79 10 02",
        description: "Event-callback registration, Start-held variant (FUN_180051130). Signature: fn(event_obj: *mut u8, event_type: i32, predicate_arg: u32, lambda: *mut u8). Registers a lambda that only fires when the Start modifier is held (event_obj+0x10 == 2). Used by scalar-row coarse-step lambdas.",
    },

    // Scene-graph layout flush called at the tail of the native tab-filter.
    // Applies the scene root's pending (x, y) offsets to every row in the
    // flat row vector at `scene_root+0x68..+0x70`, running any pending
    // scroll-position easing in the process. The tab-filter detour calls
    // this as its final step so row positions stay in sync with the
    // scene's layout state after a visibility refresh.
    //
    // Signature: fn(SceneRoot* root, bool commit_immediately).
    // Calling convention: Microsoft x64 (RCX=root, DL=commit_immediately).
    //
    // 19-byte prologue: MOV [RSP+0x20], RBX; PUSH R12; SUB RSP, 0x70;
    // MOVZX R12D, DL; MOV RBX, RCX; CALL rel32. The trailing CALL's 4-byte
    // disp32 is wildcarded because it points at a sibling helper whose
    // address shifts between builds.
    SignatureDefinition {
        name: "scene_layout_flush",
        pattern: "48 89 5C 24 20 41 54 48 83 EC 70 44 0F B6 E2 48 8B D9 E8",
        description: "Scene layout flush. Signature: fn(SceneRoot* root, bool commit_immediately). Walks the row vector at root+0x68..+0x70 and applies scroll offsets; called as the final step of the native tab-filter.",
    },

    // Options-menu focus-advance core. Takes the layout container and a
    // direction (-1 = up, +1 = down) and returns the new focus index
    // (caller writes it to container+0x168). Invoked by BOTH step-up
    // (FUN_1800495a0 passes EDX=-1) and step-down (FUN_180049670 passes
    // EDX=+1) entrypoints, so a single detour here intercepts all
    // cursor-driven focus advances.
    //
    // Detour body can:
    //   - Read EDX to determine direction.
    //   - Pre-advance our scroll window so the target row has +0xB8=1
    //     before the native walk starts (otherwise the native's
    //     `+0xB8 != 0` filter skips hidden rows and the cursor wraps).
    //
    // Signature: fn(container: *mut u8, direction: i32) -> i32.
    // Positional focus-advance: FUN_18004a030 (20260324). The spatial
    // step function called by GridPanel's own up/down navigation lambdas
    // (at container+0x178 / +0x198) when the mode flag at
    // *(lambda+0x08)+0xC0 == 0. Iterates all rows in the vector looking
    // for the nearest selectable row (checking +0xB8 != 0) in the given
    // direction, comparing positions. Returns the target focus index.
    // The caller writes the return value into container+0x168.
    // Calling convention: Microsoft x64 (RCX=container, EDX=direction
    // where +1=down, -1=up). Returns i32 (new focus index).
    SignatureDefinition {
        name: "grid_positional_step_fn",
        pattern: "89 54 24 10 55 53 41 ? 48 8D 6C 24 B9 48 81 EC ? 00 00 00 48 8B D9 48 8B 49 68 4C 8B ? 70 4C",
        description: "GridPanel positional focus-advance. fn(container: *mut u8, direction: i32) -> i32. Returns next selectable focus index.",
    },

    // ── TextLayer (value display via native text pipeline) ───────────────
    // TextLayer objects render digit-composed or bitmap-composed value
    // strings through the game's own UI pipeline rather than per-frame
    // mc_load_bitmap calls. Three functions are needed:
    //
    //   textlayer_ctor      — constructs a TextLayer object in-place;
    //                         matches function start directly.
    //   textlayer_bind      — binds the TextLayer to a parent MC and a
    //                         named child path. AOB match is at fn+0x33;
    //                         function entry derived by subtracting 0x33.
    //   textlayer_set_text  — sets the text/bitmap key on the layer each
    //                         frame; matches function start directly.
    SignatureDefinition {
        name: "textlayer_ctor",
        pattern: "48 83 EC 28 66 0F 57 C0 C6 41 08 00 45 33 C0 48 BA 00 00 00 00 00 00 F0 3F",
        description: "TextLayer constructor. Signature: fn(this: *mut u8) -> *mut u8. Initializes a 0x150-byte TextLayer object; match is at function start.",
    },
    SignatureDefinition {
        name: "textlayer_bind_anchor",
        pattern: "C6 41 60 01 48 89 51 70 48 8D 79 78 49 83 C9 FF 45 33 C0",
        description: "Internal landmark at textlayer_bind+0x33 (older builds). Function entry derived by subtracting 0x33.",
    },
    // Newer builds (20260526+): the function was simplified — no security
    // cookie, 0x20 frame, store-to-+0x70 moved before set-+0x60.
    SignatureDefinition {
        name: "textlayer_bind_direct",
        pattern: "48 89 5C 24 08 57 48 83 EC 20 49 8B C0 48 89 51 70 48 8D 79 78",
        description: "textlayer_bind function entry (20260526+ builds). Direct prologue match.",
    },
    SignatureDefinition {
        name: "textlayer_set_text",
        pattern: "48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 20 48 8D 99 A8 00 00 00 41 8B F0 48 8B F9",
        description: "TextLayer set-text function. Signature: fn(this: *mut u8, text_sso: *mut u8, mode: i32) -> *mut u8. Sets the text/bitmap key on the layer; match is at function start.",
    },
    SignatureDefinition {
        name: "pacemaker_render_input",
        pattern: "49 63 76 08 48 8B 97 B0 00 00 00",
        description: "Score-render pacemaker case (0x1036) — movsxd rsi,[r14+8]; mov rdx,[rdi+0xb0]. 11-byte patch site for ms-error swap.",
    },
    SignatureDefinition {
        name: "real_speed_bpm_anchor",
        pattern: "F2 0F 5E 01 48 8D 4C 24 40",
        description: "ddr::player::Option::SetScrollSpeed — divsd xmm0,[rcx]; lea rcx,[rsp+0x40]. Anchor for R24/R25/R26 BPM divisor swap patches.",
    },
    // NOTE: the former `real_speed_logf_anchor` (R15/R16 logf-guard) was
    // retired 2026-09-01: its AOB (`0F 28 C7 E8 ? ? ? ? F3 0F 58 C6`)
    // actually lands inside NoteResultActor::onMessage case 0x1036 — the
    // PACEMAKER readout, not any scroll-speed code (single match, same
    // function, on 20250805/20260616/20260721/20260825). Its R15 byte
    // rewrote the pacemaker zero-branch JMP and broke the exact-0 digit
    // render. See src/mods/real_speed_fix/mod.rs.
    // ── Music Wheel Song Length signatures ──────────────────────────
    // sequence::SpriteLayer — the game's "row of bitmaps by texture name"
    // widget class that renders the song-select header's BPM digits (and
    // dozens of other bitmap strings). The mod constructs one instance of
    // its own and drives it through these two functions; the per-frame
    // layout call goes through the constructed object's own vtable slot 0.
    // RE notes: .agents/planning/2026-08-16-music-wheel-song-length/research.md §3.
    SignatureDefinition {
        name: "spritelayer_ctor",
        pattern: "33 D2 48 8D 05 ? ? ? ? 48 89 01 48 89 51 48 48 89 51 50 48 89 51 08 48 89 51 10 48 89 51 18 48 89 51 28 48 89 51 30 48 89 51 38",
        description: "sequence::SpriteLayer constructor (FUN_1801d2e00 on 20260721). fn(this: *mut u8) -> *mut u8; pure field init on a 0xF8 struct (vftable LEA wildcarded). Unique single hit on 20260324/20260526/20260616/20260721.",
    },
    SignatureDefinition {
        name: "spritelayer_set_names",
        pattern: "41 56 41 57 48 81 EC 88 00 00 00 4C 8B F1 48 83 C1 28 E8 ? ? ? ? 49 8B CE E8 ? ? ? ? 4D 8B 46 28 49 8B 56 30 49 2B D0 48 B8 67 66 66 66",
        description: "sequence::SpriteLayer::SetBitmaps (FUN_1801d3070 on 20260721). fn(this, names: *const StdStringVec) -> *mut u8; COPY-assigns the names vector (source stays caller-owned), releases old CBitmaps, allocates new ones from the CBitmap pool by texture name, ends with a virtual layout call (vtable slot 0). Unique single hit on all four builds.",
    },
    SignatureDefinition {
        name: "selectmusic_model_anchor",
        pattern: "4C 8B 1D ? ? ? ? 48 C7 44 24 38 00 00 00 00 33 F6 48 89 74 24 40 49 8B 93 B8 01 00 00 4D 8B 83 B0 01 00 00",
        description: "MusicCard per-frame tick (FUN_180160910 on 20260721) reading the select-music model global: MOV R11,[rip+d32] then the highlighted-song shared_ptr loads at [R11+0x1B8]/[R11+0x1B0] (offsets pinned as imm bytes). d32 at match+3 → `selectmusic_model` derived global. Unique single hit on all four builds.",
    },
    // ── Overlay Element Styling signatures ──────────────────────────
    // BM2D CMovieClip pool-wrapper methods (see
    // docs/gameplay_overlay_elements_research.md §6).
    //
    // NOTE: `cmovieclip_create` is deliberately NOT in this batch array. Its
    // longest literal run (the 20-byte prologue) shares its first 18 bytes
    // with the existing `afp_layer_init_wrapper` signature — they resolve the
    // SAME function (CMovieClip::Create @ FUN_180257770). The batch scanner
    // builds one Aho-Corasick automaton over all needles and iterates
    // NON-overlapping matches, so at that address the shorter (afp) needle is
    // consumed first and the longer (create) needle is never reported —
    // `cmovieclip_create` would spuriously report "pattern not found". It is
    // instead resolved standalone in `derive_cmovieclip_create` (a
    // single-needle scan can't collide), with full-pattern verification.
    //
    // Wrapper SetPosition (CMovieClip vtable +0x38):
    // `fn(this, x: i32, y: i32)` — converts to floats, forwards to
    // afp_layer_set_position. Complete function; wildcards cover the
    // security-cookie load disp32, the afp_layer_set_position IAT disp32,
    // and the __security_check_cookie CALL rel32. Unique match on both
    // builds: 0x180258DE0 (20260616) / 0x18021CD20 (20260324) —
    // Ghidra-verified 2026-07-12. Used for versus side-binding (first-
    // position x-discrimination); non-fatal if missing.
    SignatureDefinition {
        name: "cmovieclip_set_position",
        pattern: "48 83 EC 38 48 8B 05 ? ? ? ? 48 33 C4 48 89 44 24 28 8B 49 08 66 0F 6E C2 66 41 0F 6E C8 48 8D 54 24 20 0F 5B C0 0F 5B C9 F3 0F 11 44 24 20 F3 0F 11 4C 24 24 FF 15 ? ? ? ? 48 8B 4C 24 28 48 33 CC E8 ? ? ? ? 48 83 C4 38 C3",
        description: "CMovieClip wrapper SetPosition (vtable +0x38) — fn(this, x:i32, y:i32). Versus side-binding detour target for overlay-element-styling.",
    },
    // ── Non-Native OS Support: background-movie DirectShow graph builder ──
    // `me::movie::impl::DShowPlayer::BuildGraph(this, request)` — the ONLY
    // function in gamemdx that touches DirectShow: CoCreateInstance(
    // CLSID_FilterGraph, IID_IGraphBuilder) → AddFilter(custom renderer) →
    // IGraphBuilder::RenderFile (vtbl+0x68). Under Wine/CrossOver, RenderFile's
    // intelligent-connect enumerates audio renderers via devenum → builtin
    // winmm, which access-violates (crash RA = match+0x2B0, right after the
    // RenderFile call). The non-native-os-support mod detours the function
    // entry and, WITHOUT calling the original, fakes the success epilogue's
    // one observable side effect — player state dword +0x8 = 3 ("opened") —
    // then returns 0. The state write is load-bearing: Dx9Movie::update's
    // status machine only advances past "opening" when getState() reads 3,
    // and the demo/gameplay sequences poll that status before starting the
    // song (a plain error-returning stub soft-locks the attract demo at a
    // black screen — live-tested 2026-07-21). The `opened` byte (+0x14) stays
    // 0, so every per-frame path keeps to its guarded early-return and none
    // of the (null) COM interface pointers is ever touched.
    //
    // Pattern = complete prologue + home-stores + the request-flag extraction
    // (`[RDX+0x14] bit0 → this+0x16, bit2 → this+0x17; LEA RSI,[RCX+0x48]`).
    // Every byte is structural (opcodes + struct-offset immediates); no
    // relocations, calls, or jumps in range. Unique single match on both
    // supported builds: 0x18023AE40 (20260616) / 0x180256EB0 (20260324) —
    // Ghidra-verified 2026-07-21. RE record:
    // .agents/planning/20260721-non-native-os-support/.
    SignatureDefinition {
        name: "movie_build_graph",
        pattern: "4C 8B DC 56 57 41 54 41 55 41 56 48 83 EC 40 48 C7 44 24 30 FE FF FF FF 49 89 5B 18 49 89 6B 20 48 8B EA 48 8B F9 8B 42 14 24 01 88 41 16 8B 42 14 C1 E8 02 24 01 88 41 17 48 8D 71 48",
        description: "DShowPlayer::BuildGraph — sole DirectShow filter-graph builder (CLSID_FilterGraph + RenderFile). Detoured by non-native-os-support to fake a successful open (player state 3, no COM) so movies are skipped without crashing Wine (builtin winmm AVs during audio-renderer enumeration) and without soft-locking the movie-status pollers.",
    },
    // ── Announcer / in-game voice dispatcher ──────────────────────────
    // The per-frame announcer body (docs/hex_edit_porting.md Hack 1,
    // 32-bit analog FUN_10047ab0): plays combo callouts
    // (`vo_ingame_combo_%04d`/`_other`), score-state cues
    // (`vo_ingame_state_NN_*`) and stage-clear cheers (`se_kansei_*`) from
    // a single cabinet-wide instance (it reads BOTH sides' combo counters
    // via max()). The announcer-mute mod detours the entry and, when mute
    // is effective, returns without calling the original — silencing every
    // family the research enumerates in one place.
    //
    // Pattern = full entry prologue (MOV RAX,RSP; PUSH RBP; LEA
    // RBP,[RAX-0x5F]; SUB RSP,0xE0; gap-slot store; home stores; XMM6/7
    // saves) + security-cookie load (disp32 wildcarded) + the distinctive
    // state guard `MOVZX EAX,word [RCX+0x82]; MOV ECX,[RCX+RAX*8+0x58];
    // TEST ECX,ECX; JZ`. Unique single match on all four supported builds:
    // 0x180055A50 (20260324 — matches the research doc's dispatcher
    // exactly), 0x180054C80 (20260526), 0x180055470 (20260616),
    // 0x180055430 (20260721). Ghidra-verified 2026-08-16.
    SignatureDefinition {
        name: "announcer_dispatcher",
        pattern: "48 8B C4 55 48 8D 68 A1 48 81 EC E0 00 00 00 48 C7 45 C7 FE FF FF FF 48 89 58 10 48 89 70 18 48 89 78 20 0F 29 70 E8 0F 29 78 D8 48 8B 05 ?? ?? ?? ?? 48 33 C4 48 89 45 2F 48 8B D9 0F B7 81 82 00 00 00 8B 4C C1 58 85 C9 0F 84",
        description: "In-game announcer/voice dispatcher entry — combo callouts, score-state cues and stage-clear cheer SFX. Detoured by the announcer-mute mod (conditional early-return).",
    },
    // ── Song playback speed: identity-only runtime transaction ────────
    // Byte-level evidence and per-build addresses live in
    // `.agents/planning/2026-08-05-song-playback-speed/research/runtime-integration.md`.
    // `derive_song_rate_runtime_sites` re-scans all three for uniqueness and
    // validates the clock's exact eight-byte redirect window before publishing
    // the derived patch address. These signatures are inert until Step 3's
    // later installation tasks consume them.
    SignatureDefinition {
        name: "song_rate_clock_anchor",
        pattern: SONG_RATE_CLOCK_ANCHOR_PATTERN,
        description: "Per-frame authoritative music_count calculation. The exact eight-byte `LEA R14D,[RAX+RBX]; LEA R12,[RDI+0x58]` redirect window is at match+0x25 and is derived only after literal-byte validation.",
    },
    SignatureDefinition {
        name: "song_rate_wavebank_create",
        pattern: SONG_RATE_WAVEBANK_CREATE_PATTERN,
        description: "audio wavebank_create(i32 file_id) -> bool entry. Returns true only after native open, XACT streaming-bank acceptance, manager insertion, and DoWork.",
    },
    SignatureDefinition {
        name: "song_rate_wavebank_unregister",
        pattern: SONG_RATE_WAVEBANK_UNREGISTER_PATTERN,
        description: "Manager-level wave-bank unregister(int file_id) entry called by audio::XwbFileCallback's unload vslot after file-manager unload; releases XACT/native bookkeeping for the exact file id.",
    },
    // ── Song playback speed: XACT file-IO callback registration site ──
    // The audio-manager constructor builds XACT_RUNTIME_PARAMETERS with
    // `lookAheadTime = 0xFA` (250 ms) followed by three LEA/MOV pairs
    // storing the notification, readFile, and getOverlappedResult callback
    // pointers. The streaming rate engine (design req 9) detours the second
    // and third — RIP-decoded from this match by
    // `derive_song_rate_io_callbacks`, which also derives the handle→file_id
    // lookup helper (first CALL inside the readFile body) and the audio
    // file-table global (from the unregister match). Verified to match
    // EXACTLY ONCE on four builds (20260324 0x1801a81a4 / 20260421
    // 0x1801a8e74 / 20260616 0x1801a9ca4 / 20260721 0x1801aad34) —
    // Ghidra-verified 2026-08-10. Cross-version table + evidence:
    // `docs/xact_streaming_research.md` §6. Fail-open: never in the required
    // set — unresolved means the streaming integration stays structurally
    // absent and the DLL boots stock.
    SignatureDefinition {
        name: "song_rate_io_callback_regsite",
        pattern: SONG_RATE_IO_CALLBACK_REGSITE_PATTERN,
        description: "Audio-manager ctor XACT callback registration (lookAheadTime 0xFA + 3x LEA/MOV). LEAs 2+3 RIP-decode to the readFile (FUN_1801aa250 on 20260721) and getOverlappedResult (FUN_1801aa350) callbacks — the streaming rate engine's detour pair (design req 9).",
    },
    // ── Audio (XACT 2) — assist-tick bank registration + cue playback ──
    //
    // The game's audio is COM-instantiated Microsoft XACT 2, wrapped by an
    // in-house "audio manager" singleton owning six sound-bank slots plus a
    // small "play a sound effect" façade. `services::game_audio` parks a
    // mod-owned sound bank in a free slot and plays cues through the façade.
    //
    // All three patterns verified to match EXACTLY ONCE on four builds
    // (20260721 / 20260616 / 20260421 / 20260324); `derive_game_audio_addresses`
    // re-checks the match count at boot. Wildcards are deliberate: every
    // address, RIP displacement, stack-frame displacement and branch
    // displacement is `??`, while semantic immediates are literal so a
    // meaningful change to the game breaks the match instead of silently
    // mis-resolving. RE record (byte-level authority for these patterns, with
    // per-build match addresses):
    // .agents/planning/20260725-assist-tick/research/bank-slot-and-anchors.md
    // → "Proposed signatures" (S1/S2/S3) and "Derivation chains" (A/B/C).
    SignatureDefinition {
        name: "se_play",
        pattern: "40 57 48 83 EC 40 48 C7 44 24 ?? FE FF FF FF 48 89 5C 24 ?? 0F 29 74 24 ?? 0F 28 F2 48 8B FA 8B D9 44 8B C1 41 FF C8 74 ?? 41 83 F8 04 74 ?? C7 44 24 ?? 05 00 00 00 48 8D 4C 24 ?? FF 15",
        description: "Public sound-effect play façade — u32(i32 bank_id /*ECX*/, const char* cue /*RDX*/, f32 pan /*XMM2, NOT R8D*/); returns a handle, 0xFFFFFFFF on failure. Match is the function entry. The distinctive core is the `bank_id ∈ {1,5} skips the SE mute filter` ladder (DEC R8D; JZ; CMP R8D,4; JZ) followed by the literal mute-filter state 5; pattern stops at the `FF 15` opcode so the mute-filter disp32 is excluded. Cross-checked by decoding its first CALL rel32, which must land on the derived se_play_inner.",
    },
    SignatureDefinition {
        name: "se_play_inner_body",
        pattern: "48 8B 35 ?? ?? ?? ?? 48 63 F9 0F 29 74 24 ?? 48 8D 47 01 0F 28 F2 48 03 C0 48 8B 1C C6 48 85 DB 74 ?? 48 8B 03 48 8B CB FF 10 B9 FF FF 00 00 66 3B C1 74 ?? 4C 8B 13 48 8D 4C 24 ?? 45 33 C9 48 89 4C 24 ?? 45 33 C0 0F B7 D0 48 8B CB 48 C7 44 24 ?? 00 00 00 00 41 FF 52 20",
        description: "Landmark inside se_play_inner (match = entry + 0xF), the sole anchor for the audio-manager global: `MOV RSI,[rip+audio_manager]` with the disp32 at match+3 — that global's absolute address MOVES ON EVERY GAME BUILD, so it must be RIP-decoded from here and never scanned or hardcoded. Also yields se_play_inner itself at match-0xF. The pattern must run all the way to `41 FF 52 20` (SoundBank::Play, vtable +0x20): the neighbouring se_prepare_inner is byte-for-byte identical for its first ~0x65 bytes apart from that displacement and its vtable index (`41 FF 52 18`), so a shorter pattern matches both. The `48 8D 47 01 / 48 03 C0 / 48 8B 1C C6` triple encodes the 0x10 slot-array stride and is kept literal so a layout change fails the match instead of mis-indexing.",
    },
    SignatureDefinition {
        name: "bank_slot_of_file_loop",
        pattern: "4C 8B 0E 48 83 C9 FF 33 C0 49 8B F9 48 8B D5 F2 AE 48 F7 D1 4C 8D 41 FF 49 8B C9 E8 ?? ?? ?? ?? 85 C0 74 ?? FF C3 48 83 C6 10 83 FB ?? 72 ?? B8 05 00 00 00",
        description: "Name-match loop of bank_slot_of_file(file_id) -> slot, which maps a loaded bank file's basename to a manager slot and returns {0,1,2,3} on a hit or the literal fallback 5 on a miss — so slot 4 is unreachable, which is what makes it claimable. The imm8 at match+0x2C is the NUMBER OF NAMED BANKS and must read 4: a build that added a fifth named bank would map it to slot 4 and silently collide with our bank. Read as a boot-time safety gate (guard G1) rather than assumed. The `B8 05 00 00 00` fallback is literal so a change there breaks the match.",
    },
    // ── Per-side player Option (assist-tick JUDGMENT TIMING) ─────────
    // The per-frame gameplay count computation (FUN_18005f100 on 20260324)
    // reaches each side's `ddr::player::Option` exactly like this:
    //
    //   MOVSXD RCX,[RBP+0x84]              ; actor's play side (0/1)
    //   MOV    [RBP+0x168],EAX             ; beat count store
    //   MOV    EAX,[RBP+0x184]             ; RENDER_OFFSET
    //   LEA    R12,[rip+disp32]            ; -> the MODULE BASE (validated)
    //   SUB    EAX,[RBP+0x170]             ; − INPUT_OFFSET
    //   XOR    EDX,EDX
    //   ADD    EAX,ESI
    //   MOV    [RBP+0x17C],EAX             ; dispMusicCount
    //   MOV    RCX,[R12+RCX*8+disp32]      ; ctx-table[side]; disp32 = table RVA
    //   CALL   option_accessor             ; returns *ctx + 0xE0 (the Option)
    //
    // The actor-field displacements (0x84/0x168/0x170/0x17C/0x184) are kept
    // literal — they are the same layout facts the timing-offsets mod rests
    // on, and a layout change SHOULD fail this match. The LEA/table/call
    // displacements are wildcarded (they move every build); the derivation
    // (`derive_player_option_table`) validates the LEA target against the
    // module base before trusting the table RVA. Verified to match exactly
    // once on 20260324 / 20260421 / 20260616 / 20260721.
    SignatureDefinition {
        name: "player_option_ctx_load",
        pattern: "48 63 8D 84 00 00 00 89 85 68 01 00 00 8B 85 84 01 00 00 4C 8D 25 ?? ?? ?? ?? 2B 85 70 01 00 00 33 D2 03 C6 89 85 7C 01 00 00 49 8B 8C CC ?? ?? ?? ?? E8",
        description: "Per-side context-table load inside the per-frame count computation. Yields the derived player_option_table (base + the MOV's disp32); each side's ddr::player::Option is *(table[side]) + 0xE0, JUDGMENT TIMING (timing_music, ±100 ms) at Option+0x24. RE record: .agents/planning/20260729-assist-tick-premixed-track/research/ra-rb-timing-chain.md.",
    },
    // ── Song-select preview restart (preview design §Components 5–6) ──
    // The four addresses the live-edit restart executor is built on: two
    // vftable identity gates (View / AudioLoader — the runtime guards that
    // make the compile-time struct offsets fail-closed across builds) and
    // two stock functions the executor calls in their stock roles (the
    // cue-handle stop and the load-completion create router, whose XWB arm
    // lands on the detoured wavebank_create so the re-create composes with
    // the preview bind branch for free).
    //
    // All four patterns verified to match EXACTLY ONCE on four builds
    // (20260721 / 20260616 / 20260421 / 20260324); `derive_preview_restart`
    // re-checks the match counts at boot. Wildcards per house style: every
    // RIP disp32, CALL rel32, stack-frame displacement and branch
    // displacement is `??`; semantic immediates and struct-field offsets
    // stay literal so a layout change breaks the match instead of silently
    // mis-resolving. Byte-level authority (annotated disassembly, per-build
    // match table): .agents/planning/2026-08-15-song-preview-rate/research/
    // preview-retrigger-re.md §9. Any miss disables only the preview
    // feature's restart half (declared through `preview::init_restart`,
    // never a mod's `required_signatures` — design R9/R11).
    SignatureDefinition {
        name: "audio_loader_ctor",
        pattern: "48 8D 05 ?? ?? ?? ?? 48 89 01 48 C7 41 08 FF FF FF FF C7 41 10 FF FF FF FF C6 41 14 00 0F B6 45 ?? 88 41 15 89 51 18",
        description: "Field-init cluster inside sequence::AudioLoader's ctor (match = entry+0x3F on 20260721): the vftable install LEA (disp32 at match+3 — the derivation's RIP-decode source) followed by the loader-layout facts the restart executor's constants rest on, kept literal as the layout gate: XWB/XSB file ids = -1,-1 (+0x08 qword), cue handle = -1 (+0x10), failed = 0 (+0x14), mode from the stack arg (+0x15, its RBP disp8 wildcarded), slot (+0x18). Yields the derived audio_loader_vftable (ONE virtual slot: the per-frame tick that fires se_play exactly once and re-arms when the handle is set back to -1).",
    },
    SignatureDefinition {
        name: "selectmusic_view_ctor",
        pattern: "48 89 5C 24 ?? 57 48 83 EC 20 48 8B D9 E8 ?? ?? ?? ?? 33 FF 48 8D 05 ?? ?? ?? ?? 4C 8D 1D ?? ?? ?? ?? 48 8D 8B E8 01 00 00 48 89 43 28 4C 89 1B 48 89 BB C0 00 00 00 48 8D 05 ?? ?? ?? ?? 48 89 83 C8 00 00 00 48 89 BB D0 00 00 00 C6 83 D8 00 00 00 01 48 C7 83 F8 00 00 00 0F 00 00 00",
        description: "sequence::selectmusic::View ctor head (match = entry). The View vftable is the SECOND LEA (`4C 8D 1D`, R11, disp32 at match+30) stored bare to [RBX] by `4C 89 1B` — the FIRST LEA (disp32 at match+23) is an inner interface vftable stored at +0x28, not the View's own. Literal layout pins: the +0x28 store, the [RBX+0x1E8] member LEA, the +0xC0/+0xD0 pointer clears, the third LEA's store to +0xC8 — the embedded sequence::AudioPlayer, THE load-bearing offset the loader-chain walk (child+0xB8 -> View -> +0xC8+0x08 loader) rests on — plus +0xD8=1 and the +0xF8=0xF string-capacity init. Yields the derived selectmusic_view_vftable (the walk's identity gate).",
    },
    SignatureDefinition {
        name: "cue_handle_stop",
        pattern: "40 53 48 83 EC 30 48 C7 44 24 ?? FE FF FF FF 8B D9 48 8D 05 ?? ?? ?? ?? 48 89 44 24 ?? 8B 0D ?? ?? ?? ?? 85 C9 7E ?? FF 15 ?? ?? ?? ?? 8B 0D ?? ?? ?? ?? 83 FB FF 74 ?? 48 8D 43 05 48 C1 E0 05 48 03 05 ?? ?? ?? ?? 74 ?? 4C 8B 00 4D 85 C0 74 ?? 49 8B 00 33 D2 49 8B C8 FF 50 08 EB ?? 48 83 78 08 00 74 ?? 48 8B 48 08 48 8B 01 BA 01 00 00 00 FF 50 10",
        description: "cue_handle_stop(i32 handle) entry — the game's own teardown stop (AudioLoader::release uses it on the stored handle; a dead/stale handle is a safe no-op inside it). Distinctive body kept literal: the `CMP EBX,-1` guard, the handle-table indexing `LEA RAX,[RBX+5]; SHL RAX,5` ((h+5)*0x20 against the rip-loaded table global, disp wildcarded), and both dispatch arms — live cue => vt+0x08 Stop(0) (`FF 50 08`), dead entry => soundbank vt+0x10 with flags=1 (`BA 01 00 00 00` + `FF 50 10`). The lock prologue's globals/import disps are wildcarded. Restart executor step 2.",
    },
    SignatureDefinition {
        name: "sound_bank_create_router",
        pattern: "40 53 48 83 EC 30 48 C7 44 24 ?? FE FF FF FF 48 63 D9 48 8D 05 ?? ?? ?? ?? 48 89 44 24 ?? 8B 0D ?? ?? ?? ?? 85 C9 7E ?? FF 15 ?? ?? ?? ?? 90 48 8D 0C 9B 48 C1 E1 05 48 8B 05 ?? ?? ?? ?? 48 03 48 28 0F B6 81 8F 00 00 00 48 8D 4C 08 11 41 B8 03 00 00 00 48 8D 15 ?? ?? ?? ?? E8 ?? ?? ?? ?? 8B CB 85 C0 75 ?? E8 ?? ?? ?? ?? 0F B6 D8 EB ?? E8 ?? ?? ?? ?? 0F B6 D8",
        description: "sound_bank_create_router(i32 file_id) entry — the FileManager 'sound'-category load-completion callback's bank creator: path extension 'xsb' => sound-bank create, anything else => wavebank_create (the detoured entry — calls into the router land on the patched function, so the restart's re-create re-qualifies through the preview bind branch for free). Distinctive FileManager row walk kept literal: `LEA RCX,[RBX+RBX*4]; SHL RCX,5` (0xA0 row stride), rows base at [mgr+0x28], path length byte at row+0x8F, the extension backset LEA (+0x11), and the strncmp('xsb', 3) setup; both dispatch CALL rel32s and every global disp wildcarded (the post-lock `90` NOP is present on all four builds). Restart executor step 4.",
    },
];

pub struct SignatureStore {
    base: *const u8,
    size: usize,
    resolved: HashMap<String, *const u8>,
    /// Cache of `CALL rel32` xrefs into the module, keyed by the call's
    /// target address. Populated once at the start of `resolve_derived`
    /// for the targets that derivation methods will look up. Allows
    /// O(N×targets×M) work to collapse into O(M).
    xref_cache: HashMap<*const u8, Vec<*const u8>>,
}

unsafe impl Send for SignatureStore {}
unsafe impl Sync for SignatureStore {}

impl SignatureStore {
    pub fn new(game_module: &GameModule) -> Self {
        Self {
            base: game_module.base,
            size: game_module.size,
            resolved: HashMap::new(),
            xref_cache: HashMap::new(),
        }
    }

    /// Look up cached `CALL rel32` xrefs to `target`, or scan the module
    /// if the target wasn't pre-batched. Derivation methods should call
    /// this instead of `scan_xrefs_to` directly so a single batched walk
    /// services every consumer.
    fn xrefs_to(&self, target: *const u8) -> Vec<*const u8> {
        if let Some(cached) = self.xref_cache.get(&target) {
            return cached.clone();
        }
        unsafe { scan_xrefs_to(self.base, self.size, target) }
    }

    /// Scan for all known signatures. Call once at startup.
    ///
    /// All ~50 patterns are resolved in a single pass over the module
    /// using the multi-pattern Aho-Corasick engine. Per-signature
    /// `[+]/[-]` log lines are emitted in `SIGNATURES` array order so
    /// log output is comparable across boots.
    pub fn resolve_all(&mut self) -> ResolveResult {
        let pattern_pairs: Vec<(&str, &str)> =
            SIGNATURES.iter().map(|s| (s.name, s.pattern)).collect();
        let mut results = scan_patterns_batch(self.base, self.size, &pattern_pairs);

        let mut missing = Vec::new();
        for sig in SIGNATURES {
            match results.remove(sig.name) {
                Some(result) => {
                    self.resolved.insert(sig.name.to_string(), result.address);
                    log_info!("  [+] {} @ +0x{:X}", sig.name, result.offset);
                }
                None => {
                    missing.push(sig.name.to_string());
                    log_warn!("  [-] {} -- pattern not found", sig.name);
                }
            }
        }

        ResolveResult {
            found: self.resolved.len(),
            total: SIGNATURES.len(),
            missing,
        }
    }

    /// Resolve derived addresses from already-found signatures.
    pub fn resolve_derived(&mut self) {
        // Promote version-specific alternatives to their canonical names
        // so downstream code can look up a single stable name.
        if self.get_address("folder_register").is_none() {
            if let Some(addr) = self.get_address("folder_register_v2") {
                self.resolved.insert("folder_register".into(), addr);
            }
        }

        self.populate_xref_cache();

        self.find_sprite_vtable();
        self.find_check_step_data_actor();
        self.derive_ultrafast_boot();
        self.find_scene_transition();
        self.find_auto_foot_panel();
        self.find_judge_notes();
        self.find_gameplay_actor_vtable();
        self.derive_folder_functor_ctors();
        self.derive_gameplay_obj_addresses();
        self.derive_app_heap_handle();
        self.derive_file_manager_singleton();
        self.derive_render_globals();
        self.derive_layer_table();
        self.derive_player_work_table();
        self.derive_max_stage_global();
        self.derive_shutter_actor_global();
        self.derive_selectmusic_model();
        self.derive_row_builder_fn();
        self.find_option_tab_vtable();
        self.derive_option_element_ctor(
            ".?AV?$OptionElement@W4KIND@ArrowColor@option@player@ddr@@@selectmusic@sequence@@",
            "option_element_arrowcolor_ctor",
            "option_element_arrowcolor_primary_vtable",
        );
        self.derive_option_element_ctor(
            ".?AV?$OptionElement@H@selectmusic@sequence@@",
            "option_element_int_ctor",
            "option_element_int_primary_vtable",
        );
        self.derive_string_assign_via_pair();
        self.derive_event_lambda_vtable_slots();
        self.derive_textlayer_bind();
        self.derive_customize_offset();
        self.derive_timing_config_setter();
        self.derive_bm2d_package_addresses();
        self.derive_cmovieclip_create();
        self.derive_cmovieclip_color_twins();
        self.derive_playfield_styling();
        self.derive_game_audio_addresses();
        self.derive_player_option_table();
        self.derive_strip_hud_anchors();
        self.derive_frame_tick_global();
        self.find_gauge_vtables();
        self.derive_judge_rebuild_trio();
        self.derive_song_rate_runtime_sites();
        // Must follow derive_song_rate_runtime_sites: consumes the
        // uniqueness-revalidated wavebank_unregister match.
        self.derive_song_rate_io_callbacks();
        self.derive_preview_restart();
        self.derive_smarv_results_course_gate();
        self.derive_ghost_vec_copy();
    }

    /// Derive `results_course_gate_global` — the global the PlaydataTab
    /// populate consults to pick the record it displays (`DAT_1806F14F8`
    /// on 20260721): `**global + 0x70 != 0` ⇒ the tab reads the COURSE
    /// record (`PlayerWork+0x2D8`), else the per-stage array
    /// (`PlayerWork+0x590 + stage*0x2B8`). The s_marvelous results row
    /// replicates the exact branch so its recompute always reads the SAME
    /// record the tab renders (S-Marvelous is mode-agnostic — normal,
    /// course/Dan, training all show it).
    ///
    /// Derivation: inside the first 0x100 bytes of `playdata_tab_update`,
    /// the (single) sequence
    ///   MOV RAX,[rip+disp32]  ; 48 8B 05 ..    the gate global
    ///   MOV RCX,[RAX]         ; 48 8B 08
    ///   CMP [RCX+0x70],RDI    ; 48 39 79 70
    /// — verified byte-identical shape on 20260616/20260721. Fails closed.
    fn derive_smarv_results_course_gate(&mut self) {
        let populate = match self.get_address("playdata_tab_update") {
            Some(a) => a,
            None => {
                log_warn!("  [-] results_course_gate_global -- playdata_tab_update unresolved");
                return;
            }
        };
        const PATTERN: &str = "48 8B 05 ? ? ? ? 48 8B 08 48 39 79 70";
        let hits = scan_pattern_all(populate, 0x100, PATTERN);
        if hits.len() != 1 {
            log_warn!(
                "  [-] results_course_gate_global -- expected 1 gate match, found {}",
                hits.len()
            );
            return;
        }
        unsafe {
            let global = decode_rip_relative(hits[0].address.add(3));
            let off = global.offset_from(self.base) as usize;
            if off >= self.size {
                log_warn!("  [-] results_course_gate_global -- derived address outside module");
                return;
            }
            self.resolved
                .insert("results_course_gate_global".into(), global);
            log_info!("  [+] results_course_gate_global (derived) @ +0x{:X}", off);
        }
    }

    /// `ghost_vec_copy` — the game's `std::vector<u8>` copy-assign, decoded
    /// from the CALL rel32 at `ghost_local_slot_copy_site + 25`. Cross-checked
    /// to sit inside `ghost_actor_init`'s body (the site is that function's
    /// local-slot branch), and the target must lie inside the module. Any miss
    /// leaves the ghost cache fail-open (no injection).
    fn derive_ghost_vec_copy(&mut self) {
        let (Some(site), Some(init)) = (
            self.get_address("ghost_local_slot_copy_site"),
            self.get_address("ghost_actor_init"),
        ) else {
            log_warn!("  [-] ghost_vec_copy -- copy site / GhostActor init unresolved");
            return;
        };
        // The copy site must be a forward reference inside the init function
        // (its body is ~0x2B7 bytes on 20260721; allow generous slack).
        let rel = (site as usize).wrapping_sub(init as usize);
        if rel == 0 || rel > 0x800 {
            log_warn!(
                "  [-] ghost_vec_copy -- copy site {:p} not inside GhostActor init {:p}",
                site,
                init
            );
            return;
        }
        unsafe {
            if *site.add(25) != 0xE8 {
                log_warn!("  [-] ghost_vec_copy -- expected CALL rel32 at site+25");
                return;
            }
            let target = decode_call_rel32(site.add(25));
            let off = (target as usize).wrapping_sub(self.base as usize);
            if off >= self.size {
                log_warn!("  [-] ghost_vec_copy -- derived target outside module");
                return;
            }
            self.resolved.insert("ghost_vec_copy".into(), target);
            log_info!("  [+] ghost_vec_copy (derived) @ +0x{:X}", off);
        }
    }

    fn derive_song_rate_runtime_sites(&mut self) {
        for (name, pattern) in [
            ("song_rate_clock_anchor", SONG_RATE_CLOCK_ANCHOR_PATTERN),
            (
                "song_rate_wavebank_create",
                SONG_RATE_WAVEBANK_CREATE_PATTERN,
            ),
            (
                "song_rate_wavebank_unregister",
                SONG_RATE_WAVEBANK_UNREGISTER_PATTERN,
            ),
        ] {
            let matches = scan_pattern_all(self.base, self.size, pattern);
            if matches.len() != 1 {
                self.resolved.remove(name);
                log_warn!(
                    "  [-] {} -- expected exactly one match, found {}",
                    name,
                    matches.len()
                );
                continue;
            }
            self.resolved.insert(name.into(), matches[0].address);
        }

        let Some(anchor) = self.get_address("song_rate_clock_anchor") else {
            return;
        };
        let anchor_offset = anchor as usize - self.base as usize;
        let Some(patch_end) = anchor_offset
            .checked_add(SONG_RATE_CLOCK_PATCH_OFFSET)
            .and_then(|offset| offset.checked_add(SONG_RATE_CLOCK_EXPECTED.len()))
        else {
            self.resolved.remove("song_rate_clock_anchor");
            log_warn!("  [-] song_rate_clock_patch -- derived range overflow");
            return;
        };
        if patch_end > self.size {
            self.resolved.remove("song_rate_clock_anchor");
            log_warn!("  [-] song_rate_clock_patch -- derived range outside module");
            return;
        }
        let patch = unsafe { anchor.add(SONG_RATE_CLOCK_PATCH_OFFSET) };
        let actual = unsafe { std::slice::from_raw_parts(patch, SONG_RATE_CLOCK_EXPECTED.len()) };
        if actual != SONG_RATE_CLOCK_EXPECTED {
            self.resolved.remove("song_rate_clock_anchor");
            log_warn!("  [-] song_rate_clock_patch -- redirect bytes changed");
            return;
        }
        self.resolved.insert("song_rate_clock_patch".into(), patch);
        log_info!(
            "  [+] song_rate_clock_patch @ +0x{:X}",
            patch as usize - self.base as usize
        );
    }

    /// Derive the streaming rate engine's IO-callback addresses from the
    /// `song_rate_io_callback_regsite` match (design req 9; evidence chain
    /// and cross-version table in `docs/xact_streaming_research.md` §2/§6):
    ///
    /// - `song_rate_readfile_callback` / `song_rate_overlapped_callback` —
    ///   RIP-decoded from the match's second and third LEAs (the detour
    ///   pair; both or neither, they are only ever installed together).
    /// - `song_rate_handle_lookup` — the stock handle→file_id lookup helper
    ///   (fastcall: HANDLE in RCX, returns file_id in EAX, -1 on miss; takes
    ///   the AVS mutex itself), decoded from the readFile body's first CALL
    ///   at entry+0x21 behind a 34-byte literal-prologue validation. The
    ///   read detour calls it to replicate the stock locked sorted-vector
    ///   walk exactly (design req 11).
    /// - `song_rate_file_table` — the audio file-table global (data rows at
    ///   `[*global+0x8] + file_id*0x40`: buffer ptr +0x8, size u32 +0x14;
    ///   path rows at `[*global+0x28] + file_id*0xA0 + 0x11`), RIP-decoded
    ///   from the already-validated `song_rate_wavebank_unregister` match
    ///   (its literal bytes pin the access shape). Sources the binding's
    ///   `SourceView` (the FileManager RAM copy) and the dance-path check.
    ///
    /// Fail-closed derivation, fail-open feature: any validation failure
    /// removes EVERYTHING this function may publish (plus the regsite name
    /// itself) with one WARN; none of the names is in the required set, so
    /// absence just leaves the streaming integration structurally off and
    /// the DLL booting stock (design req 40).
    ///
    /// MUST run after `derive_song_rate_runtime_sites` — it consumes the
    /// uniqueness-revalidated unregister match published there.
    fn derive_song_rate_io_callbacks(&mut self) {
        const PUBLISHED: [&str; 5] = [
            "song_rate_io_callback_regsite",
            "song_rate_readfile_callback",
            "song_rate_overlapped_callback",
            "song_rate_handle_lookup",
            "song_rate_file_table",
        ];
        macro_rules! fail {
            ($($arg:tt)*) => {{
                for name in PUBLISHED {
                    self.resolved.remove(name);
                }
                log_warn!($($arg)*);
                return;
            }};
        }

        let module_start = self.base as usize;
        let module_end = module_start + self.size;
        let in_module = |p: *const u8| {
            let a = p as usize;
            a >= module_start && a < module_end
        };

        let matches = scan_pattern_all(self.base, self.size, SONG_RATE_IO_CALLBACK_REGSITE_PATTERN);
        if matches.len() != 1 {
            fail!(
                "  [-] song_rate_io_callback_regsite -- expected exactly one match, found {}",
                matches.len()
            );
        }
        let regsite = matches[0].address;

        unsafe {
            let readfile = decode_rip_relative(regsite.add(SONG_RATE_IO_READFILE_LEA_DISP));
            let overlapped = decode_rip_relative(regsite.add(SONG_RATE_IO_OVERLAPPED_LEA_DISP));
            if !in_module(readfile) || !in_module(overlapped) {
                fail!("  [-] song_rate_io_callbacks -- decoded callback outside module");
            }

            // Validate the readFile prologue (ends at the E8 opcode of the
            // handle-lookup CALL) before trusting the rel32 decode.
            let prefix = std::slice::from_raw_parts(readfile, SONG_RATE_IO_READFILE_PREFIX.len());
            if prefix != SONG_RATE_IO_READFILE_PREFIX {
                fail!("  [-] song_rate_handle_lookup -- readFile prologue bytes changed");
            }
            let handle_lookup = decode_call_rel32(readfile.add(SONG_RATE_IO_READFILE_CALL_OFFSET));
            if !in_module(handle_lookup) {
                fail!("  [-] song_rate_handle_lookup -- decoded target outside module");
            }

            // File-table global from the unregister match (uniqueness already
            // revalidated by derive_song_rate_runtime_sites).
            let Some(unregister) = self.get_address("song_rate_wavebank_unregister") else {
                fail!("  [-] song_rate_file_table -- wavebank_unregister unresolved");
            };
            let mov = std::slice::from_raw_parts(
                unregister.add(SONG_RATE_IO_FILE_TABLE_MOV_OFFSET),
                SONG_RATE_IO_FILE_TABLE_MOV_OPCODE.len(),
            );
            if mov != SONG_RATE_IO_FILE_TABLE_MOV_OPCODE {
                fail!("  [-] song_rate_file_table -- global-load opcode changed");
            }
            let file_table = decode_rip_relative(unregister.add(SONG_RATE_IO_FILE_TABLE_DISP));
            if !in_module(file_table) {
                fail!("  [-] song_rate_file_table -- decoded global outside module");
            }

            for (name, addr) in [
                ("song_rate_readfile_callback", readfile),
                ("song_rate_overlapped_callback", overlapped),
                ("song_rate_handle_lookup", handle_lookup),
                ("song_rate_file_table", file_table),
            ] {
                self.resolved.insert(name.into(), addr);
                log_info!(
                    "  [+] {} (derived) @ +0x{:X}",
                    name,
                    addr as usize - module_start
                );
            }
        }
    }

    /// Song-select preview restart derivations (preview design §Components
    /// 6, byte authority: `.agents/planning/2026-08-15-song-preview-rate/
    /// research/preview-retrigger-re.md` §9): re-validate the four preview
    /// patterns' uniqueness, then RIP-decode the two vftable identity gates
    /// (`selectmusic_view_vftable`, `audio_loader_vftable`) from their ctor
    /// matches. The two function signatures (`cue_handle_stop`,
    /// `sound_bank_create_router`) ARE their yields — match = entry — and
    /// need no decode here.
    ///
    /// Fail-closed per piece: the restart executor pokes live game objects
    /// through these addresses, so a non-unique pattern or an out-of-module
    /// decode is refused loudly (one WARN naming the piece, nothing
    /// published) rather than half-trusted. A refused piece disables only
    /// the preview feature's restart half (`preview::init_restart` is
    /// all-or-nothing) — wheel-settle preview binds and the gameplay rate
    /// feature run untouched.
    fn derive_preview_restart(&mut self) {
        /// Offset of the `LEA RAX,[rip+AudioLoader::vftable]` displacement
        /// within the `audio_loader_ctor` match.
        const LOADER_VFT_DISP: usize = 3;
        /// Offset of the `LEA R11,[rip+View::vftable]` displacement within
        /// the `selectmusic_view_ctor` match — the SECOND LEA (`4C 8D 1D`,
        /// stored bare to `[RBX]`): the first LEA (disp at match+23) is an
        /// inner interface vftable stored at `+0x28`, not the View's own.
        const VIEW_VFT_DISP: usize = 30;

        // Uniqueness re-validation (the audio-family style): `resolve_all`
        // has first-match semantics, so a second match anywhere in the
        // module would silently poke the wrong object at restart time.
        let mut unique = true;
        for name in [
            "audio_loader_ctor",
            "selectmusic_view_ctor",
            "cue_handle_stop",
            "sound_bank_create_router",
        ] {
            let count = self.get_all_matches(name).len();
            if count > 1 {
                log_warn!(
                    "  [!] {} matched {} times -- not unique on this build; preview restart derivations refused (verify against preview-retrigger-re.md §9)",
                    name,
                    count
                );
                unique = false;
            }
        }
        if !unique {
            return;
        }

        let module_start = self.base as usize;
        let in_module =
            |p: *const u8| (p as usize) >= module_start && (p as usize) < module_start + self.size;

        // (anchor signature, derived name, disp offset, check slot 0)
        // Both vftables must decode in-module; slot 0 must additionally be
        // an in-module function pointer (the AudioLoader's single virtual
        // slot is the per-frame tick, the View's slot 0 its first virtual)
        // — the same cheap "is this really a vftable" corroboration both
        // identity gates rest on at restart time.
        for (anchor_name, derived_name, disp) in [
            ("audio_loader_ctor", "audio_loader_vftable", LOADER_VFT_DISP),
            (
                "selectmusic_view_ctor",
                "selectmusic_view_vftable",
                VIEW_VFT_DISP,
            ),
        ] {
            let Some(anchor) = self.get_address(anchor_name) else {
                log_warn!(
                    "  [-] {} -- {} anchor unresolved",
                    derived_name,
                    anchor_name
                );
                continue;
            };
            unsafe {
                let vftable = decode_rip_relative(anchor.add(disp));
                if !in_module(vftable) {
                    log_warn!(
                        "  [-] {} -- decoded vftable {:p} outside the module; refusing",
                        derived_name,
                        vftable
                    );
                    continue;
                }
                let slot0 = (vftable as *const *const u8).read_unaligned();
                if !in_module(slot0) {
                    log_warn!(
                        "  [-] {} -- vftable slot 0 {:p} outside the module (not a vftable); refusing",
                        derived_name,
                        slot0
                    );
                    continue;
                }
                self.resolved.insert(derived_name.into(), vftable);
                log_info!(
                    "  [+] {} (derived, ctor LEA) @ +0x{:X}",
                    derived_name,
                    vftable as usize - module_start
                );
            }
        }
    }

    /// Derive the BM2D package registry global and name-lookup helper from
    /// the `bm2d_data_is_ready` anchor (see the signature's comment). The
    /// anchor's body is exactly:
    ///
    /// ```text
    /// +0   PUSH RBX; SUB RSP,0x20
    /// +6   MOV RAX, [rip+disp32]      ; disp32 at +9 -> registry global
    /// +13  MOV R8, RCX
    /// +16  MOV RBX, [RAX+8]           ; end
    /// +20  MOV RCX, [RAX]             ; begin
    /// +23  MOV RDX, RBX
    /// +26  CALL lookup                ; E8 rel32 -> bm2d_package_lookup
    /// ```
    ///
    /// `bm2d_package_registry` is the address of the global *pointer* to the
    /// heap-allocated registry object ([0]=begin, [8]=end) — dereference at
    /// use time (it is created lazily during boot).
    fn derive_bm2d_package_addresses(&mut self) {
        let anchor = match self.get_address("bm2d_data_is_ready") {
            Some(a) => a,
            None => {
                log_warn!("  [-] bm2d_package_registry/lookup -- anchor unresolved");
                return;
            }
        };
        unsafe {
            let registry = decode_rip_relative(anchor.add(9));
            self.resolved
                .insert("bm2d_package_registry".into(), registry);
            log_info!(
                "  [+] bm2d_package_registry (derived) @ +0x{:X}",
                registry.offset_from(self.base) as usize
            );

            let call_site = anchor.add(26);
            if *call_site != 0xE8 {
                log_warn!(
                    "  [-] bm2d_package_lookup -- expected E8 at anchor+26, got 0x{:02X}",
                    *call_site
                );
                return;
            }
            let lookup = decode_call_rel32(call_site);
            self.resolved.insert("bm2d_package_lookup".into(), lookup);
            log_info!(
                "  [+] bm2d_package_lookup (derived) @ +0x{:X}",
                lookup.offset_from(self.base) as usize
            );
        }
    }

    /// Resolve `cmovieclip_create` (CMovieClip::Create @ FUN_180257770) with a
    /// standalone single-pattern scan.
    ///
    /// It CANNOT go in the batch `SIGNATURES` array: its 20-byte prologue
    /// literal run shares its first 18 bytes with `afp_layer_init_wrapper`
    /// (the same function's shorter, pre-existing signature). The batch
    /// scanner builds one Aho-Corasick automaton and iterates NON-overlapping
    /// matches, so at 0x…257770 the shorter afp needle is consumed first and
    /// this longer needle is never reported. A standalone scan uses a
    /// single-needle automaton (no cross-pattern collision) and still verifies
    /// the full pattern — including the `MOV [RCX+0x23C],EAX` /
    /// `MOV EDX,[RDX+0x314]` anchors that make it specific to Create.
    ///
    /// `Create(this, package*, name: *const c_char /*R8*/, priority: i32,
    /// mode: i32)`. Unique full-pattern match on builds 20260616
    /// (0x180257770) and 20260324 (0x18021B6A0) — Ghidra-verified 2026-07-12.
    fn derive_cmovieclip_create(&mut self) {
        const PATTERN: &str = "48 89 5C 24 10 56 48 83 EC 40 41 8B F1 48 8B D9 48 85 D2 0F 84 ? ? ? ? 4D 85 C0 0F 84 ? ? ? ? 83 79 08 00 0F 85 ? ? ? ? 8B 02 89 81 3C 02 00 00 8B 92 14 03 00 00";
        match scan_pattern(self.base, self.size, PATTERN) {
            Some(r) => {
                self.resolved.insert("cmovieclip_create".into(), r.address);
                log_info!("  [+] cmovieclip_create (standalone) @ +0x{:X}", r.offset);
            }
            None => {
                log_warn!("  [-] cmovieclip_create -- standalone pattern not found");
            }
        }
    }

    /// Resolve the CMovieClip wrapper SetColor detour targets for the
    /// overlay-element-styling mod
    /// (`docs/gameplay_overlay_elements_research.md` §6.2/§6.3).
    ///
    /// Each pattern matches EXACTLY two byte-identical function bodies: the
    /// multiplicative set_color form and its additive set_acolor twin — and
    /// the twin ORDER FLIPS between supported builds (20260616 vs 20260324),
    /// so "first match" would silently pick the wrong one on one build. The
    /// only reliable discriminator is each body's `CALL [RIP+disp32]` IAT
    /// slot: decode it, read the loader-patched function pointer, and compare
    /// against the libafp exports resolved by name (libafp is a static import
    /// of gamemdx, so it is guaranteed loaded — and its IAT slots patched —
    /// by the time `resolve_derived` runs). Publishes:
    ///
    ///   - `cmovieclip_set_color_float` — vtable +0x90,
    ///     `fn(this, a: f32, r: f32, g: f32, b: f32)` — **alpha is the FIRST
    ///     float arg** (forwarded as `afp_layer_set_color(id, r, g, b, a)`).
    ///   - `cmovieclip_set_color_int` — vtable +0xB0,
    ///     `fn(this, a_pct: i32, r: f32, g: f32, b: f32)` (alpha percent,
    ///     divided by 100.0 before forwarding).
    ///
    /// ANY ambiguity (≠2 matches, missing exports, unexpected opcode, an IAT
    /// target matching neither export, or both matches resolving to the same
    /// export) leaves the name unresolved: misidentifying set_color vs
    /// set_acolor would silently write the wrong color-transform channel.
    fn derive_cmovieclip_color_twins(&mut self) {
        // (published name, pattern, `FF 15` opcode offset, disp32 offset).
        // Offsets Ghidra-verified on both builds 2026-07-12.
        const TWINS: &[(&str, &str, usize, usize)] = &[
            (
                "cmovieclip_set_color_float",
                "48 83 EC 38 8B 49 08 0F 28 C3 F3 0F 10 5C 24 60 0F 28 E2 F3 0F 11 4C 24 20 0F 28 D0 0F 28 CC FF 15 ? ? ? ? 48 83 C4 38 C3",
                0x1F,
                0x21,
            ),
            (
                "cmovieclip_set_color_int",
                "48 83 EC 38 8B 49 08 0F 28 CB F3 0F 10 5C 24 60 0F 28 E2 66 0F 6E C2 0F 28 D1 0F 5B C0 0F 28 CC F3 0F 5E 05 ? ? ? ? F3 0F 11 44 24 20 FF 15",
                0x2E,
                0x30,
            ),
        ];

        let set_color = resolve_libafp_export("afp_layer_set_color");
        let set_acolor = resolve_libafp_export("afp_layer_set_acolor");
        let (set_color, set_acolor) = match (set_color, set_acolor) {
            (Some(c), Some(a)) => (c, a),
            _ => {
                log_warn!(
                    "  [-] cmovieclip_set_color_* -- libafp set_color/set_acolor exports unavailable"
                );
                return;
            }
        };

        for &(name, pattern, opcode_off, disp_off) in TWINS {
            let matches = scan_pattern_all(self.base, self.size, pattern);
            if matches.len() != 2 {
                log_warn!(
                    "  [-] {} -- expected exactly 2 twin matches, got {}",
                    name,
                    matches.len()
                );
                continue;
            }

            let mut color_match: Option<*const u8> = None;
            let mut acolor_match: Option<*const u8> = None;
            let mut ambiguous = false;

            for m in &matches {
                let addr = m.address;
                unsafe {
                    if *addr.add(opcode_off) != 0xFF || *addr.add(opcode_off + 1) != 0x15 {
                        log_warn!(
                            "  [-] {} -- expected FF 15 at match+0x{:X}, got {:02X} {:02X}",
                            name,
                            opcode_off,
                            *addr.add(opcode_off),
                            *addr.add(opcode_off + 1)
                        );
                        ambiguous = true;
                        break;
                    }
                    let iat_slot = decode_rip_relative(addr.add(disp_off));
                    let target = (iat_slot as *const *const u8).read_unaligned();
                    if target == set_color {
                        ambiguous |= color_match.replace(addr).is_some();
                    } else if target == set_acolor {
                        ambiguous |= acolor_match.replace(addr).is_some();
                    } else {
                        log_warn!(
                            "  [-] {} -- match +0x{:X}: IAT target {:p} is neither set_color nor set_acolor",
                            name,
                            addr.offset_from(self.base) as usize,
                            target
                        );
                        ambiguous = true;
                    }
                }
            }

            match (ambiguous, color_match, acolor_match) {
                (false, Some(color), Some(acolor)) => {
                    self.resolved.insert(name.into(), color);
                    unsafe {
                        log_info!(
                            "  [+] {} (twin-disambiguated) @ +0x{:X} (acolor sibling @ +0x{:X})",
                            name,
                            color.offset_from(self.base) as usize,
                            acolor.offset_from(self.base) as usize
                        );
                    }
                }
                _ => {
                    log_warn!(
                        "  [-] {} -- twin disambiguation failed (color={}, acolor={}) -- unresolved",
                        name,
                        color_match.is_some(),
                        acolor_match.is_some()
                    );
                }
            }
        }
    }

    /// Derive `timing_config_set_int` — the config-map int setter the timing-
    /// init publisher calls to publish SOUND/INPUT/RENDER/BOMB_FRAME offsets.
    ///
    /// The setter cannot be resolved by its own prologue: it shares a
    /// byte-identical prologue with a sibling FNV-map int setter for a
    /// different config map (they differ only in the RIP-relative map global
    /// loaded near the tail). Instead we anchor on the publisher's first
    /// config-set pair (`timing_set_call_landmark`, whose first match is the
    /// SOUND_OFFSET pair) and decode the `CALL rel32` at landmark+0xA — the
    /// int-setter the game calls to set "SOUND_OFFSET" is, by definition, the
    /// timing setter.
    fn derive_timing_config_setter(&mut self) {
        let landmark = match self.get_address("timing_set_call_landmark") {
            Some(a) => a,
            None => {
                log_warn!("  [-] timing_config_set_int -- landmark unresolved");
                return;
            }
        };
        unsafe {
            // Pair layout: MOV EDX,[RBP+d] (3) + LEA RCX,[rip+disp] (7) = 10
            // bytes, then the CALL. Verify the opcode before decoding.
            let call_site = landmark.add(0x0A);
            if *call_site != 0xE8 {
                log_warn!(
                    "  [-] timing_config_set_int -- expected E8 at landmark+0xA, got 0x{:02X}",
                    *call_site
                );
                return;
            }
            let setter = decode_call_rel32(call_site);
            self.resolved.insert("timing_config_set_int".into(), setter);
            let offset = setter.offset_from(self.base) as usize;
            log_info!("  [+] timing_config_set_int (derived) @ +0x{:X}", offset);
        }
        self.derive_timing_config_map_global();
    }

    /// Derive `timing_config_map_global` — the address of the process-global
    /// pointer to the config-map root that the int setter dereferences (the
    /// `DAT_1806ebcf0`/`DAT_1806f1d70` analog). The boot publisher null-guards
    /// on `*global != 0` before publishing the offsets, so the timing-offsets
    /// mod observes this same pointer to know when the map is live (rather than
    /// only latching off the first hook hit — lets the boot-seed fallback fire
    /// even if hook-install ordering is ever violated).
    ///
    /// Derivation: the setter loads the map root via the first
    /// `MOV RDX, qword ptr [rip+disp32]` (`48 8B 15` ..) in its body — verified
    /// the first such instruction on both supported builds (it sits just past
    /// the inlined FNV-1a key-hash loop). Scan the setter prologue for that
    /// opcode and RIP-decode its displacement to the global's address.
    fn derive_timing_config_map_global(&mut self) {
        let setter = match self.get_address("timing_config_set_int") {
            Some(a) => a,
            None => return, // setter unresolved → nothing to derive from
        };
        unsafe {
            // Scan a generous window covering the prologue + FNV loop; the
            // `MOV RDX,[rip+disp]` map-root load is ~0x46 in on both builds. The
            // instruction is 7 bytes (`48 8B 15` + disp32), so stop 6 bytes shy
            // of the window end to keep the whole candidate inside the window.
            const WINDOW: usize = 0x80;
            for i in 0..(WINDOW - 6) {
                let p = setter.add(i);
                if *p == 0x48 && *p.add(1) == 0x8B && *p.add(2) == 0x15 {
                    let global = decode_rip_relative(p.add(3));
                    self.resolved
                        .insert("timing_config_map_global".into(), global);
                    let offset = global.offset_from(self.base) as usize;
                    log_info!("  [+] timing_config_map_global (derived) @ +0x{:X}", offset);
                    return;
                }
            }
            log_warn!(
                "  [-] timing_config_map_global -- MOV RDX,[rip] not found in setter prologue"
            );
        }
    }

    /// Derive the playfield-styling mod's target set from already-resolved
    /// signatures (`render_notes`, `get_offset_y`) + RTTI. Publishes, on
    /// success:
    ///
    ///   - `note_collector` — the per-pass note collector (called from
    ///     `render_notes`; iterates the judge Results vector).
    ///   - `collector_cull_site` — the `MOVSS XMM15,[RIP+disp32]`
    ///     (`F3 44 0F 10 3D`) instruction inside the collector that loads the
    ///     720.0f top-cull bound. Patch target (disp32 redirect).
    ///   - `guideline_draw` — the measure-guideline draw function.
    ///   - `guideline_cull_site` — the `MOVSS XMM9,[RIP+disp32]`
    ///     (`F3 44 0F 10 0D`) 720.0f load inside the guideline draw.
    ///   - `guideline_bulk_emitter` — the guideline's private bulk sprite
    ///     emitter (writes a tag-0x01 DRAWSPRITES command, 0x14-byte record
    ///     stride; exactly ONE caller module-wide).
    ///   - `arrow_renderer_vtable` / `spot_renderer_vtable` /
    ///     `judge_effect_renderer_vtable` — offset-0 vftables via RTTI walk,
    ///     used by the fill hook to classify renderer instances.
    ///
    /// Every step verifies instruction bytes and content before publishing;
    /// ANY ambiguity (no match, multiple matches, wrong constant value)
    /// leaves the name unresolved so the mod's all-or-nothing gate fails
    /// closed (never patch unverified bytes).
    ///
    /// NOTE (verified in Ghidra on builds 20260616 + 20260324): the naive
    /// "first CALL rel32 in render_notes" heuristic is WRONG for the
    /// collector — stray 0xE8 bytes occur earlier as MOV displacement bytes,
    /// and the true first CALL targets a per-pass helper, not the collector.
    /// The collector is instead identified by content: it is the unique
    /// render_notes callee whose body contains the XMM15-form 720.0f load.
    fn derive_playfield_styling(&mut self) {
        self.derive_note_collector();
        self.derive_guideline_targets();

        // Renderer vtables (offset-0 vftables; each class has exactly one
        // COL with offset 0 and one vtable meta-pointer — Ghidra-verified).
        const RENDERER_VTABLES: &[(&str, &str)] = &[
            (".?AVArrowRenderer@screen@@", "arrow_renderer_vtable"),
            (".?AVSpotRenderer@screen@@", "spot_renderer_vtable"),
            (
                ".?AVJudgeEffectRenderer@screen@@",
                "judge_effect_renderer_vtable",
            ),
        ];
        for &(rtti, name) in RENDERER_VTABLES {
            if let Some(vt) = self.find_vtable_by_rtti(rtti, name) {
                self.resolved.insert(name.into(), vt);
                log_info!(
                    "  [+] {} (RTTI) @ +0x{:X}",
                    name,
                    unsafe { vt.offset_from(self.base) } as usize
                );
            }
            // find_vtable_by_rtti logs its own [-] on failure.
        }
    }

    /// Scan a byte window for `prefix` followed by a RIP-relative disp32
    /// whose target reads `expect` (f32). Returns the matching instruction
    /// addresses (of the first prefix byte). `prefix` is the full opcode
    /// prefix up to (excluding) the disp32.
    fn find_rip_f32_loads(
        &self,
        start: *const u8,
        window: usize,
        prefix: &[u8],
        expect: f32,
    ) -> Vec<*const u8> {
        let mut out = Vec::new();
        let insn_len = prefix.len() + 4;
        let end = (start as usize + window).min(self.base as usize + self.size);
        let window = end.saturating_sub(start as usize);
        if window < insn_len {
            return out;
        }
        unsafe {
            let bytes = std::slice::from_raw_parts(start, window);
            for i in 0..=(window - insn_len) {
                if &bytes[i..i + prefix.len()] != prefix {
                    continue;
                }
                let target = decode_rip_relative(start.add(i + prefix.len()));
                let t = target as usize;
                if t < self.base as usize || t + 4 > self.base as usize + self.size {
                    continue;
                }
                if (target as *const f32).read_unaligned() == expect {
                    out.push(start.add(i));
                }
            }
        }
        out
    }

    /// Derive `note_collector` + `collector_cull_site` (see
    /// [`Self::derive_playfield_styling`]). The collector is the unique
    /// CALL-rel32 target within `render_notes`' first 0x400 bytes whose own
    /// first 0x100 bytes contain the `MOVSS XMM15,[RIP+disp]` 720.0f load.
    fn derive_note_collector(&mut self) {
        /// `MOVSS XMM15, dword ptr [RIP+disp32]` opcode prefix.
        const CULL_PREFIX_XMM15: &[u8] = &[0xF3, 0x44, 0x0F, 0x10, 0x3D];
        const RENDER_NOTES_WINDOW: usize = 0x400;
        const COLLECTOR_WINDOW: usize = 0x100;

        let render_notes = match self.get_address("render_notes") {
            Some(a) => a,
            None => {
                log_warn!("  [-] note_collector -- render_notes unresolved");
                return;
            }
        };

        // Every E8 byte in the window is a candidate CALL opcode; decoding a
        // displacement byte as a call yields an out-of-module target (or one
        // that fails the content check), so candidates self-filter.
        let mut verified: Vec<(*const u8, *const u8)> = Vec::new(); // (fn, cull insn)
        unsafe {
            let mod_lo = self.base as usize;
            let mod_hi = mod_lo + self.size;
            for i in 0..RENDER_NOTES_WINDOW {
                let p = render_notes.add(i);
                if *p != 0xE8 {
                    continue;
                }
                let target = decode_call_rel32(p);
                let t = target as usize;
                if t < mod_lo || t + COLLECTOR_WINDOW > mod_hi || target == render_notes {
                    continue;
                }
                let culls =
                    self.find_rip_f32_loads(target, COLLECTOR_WINDOW, CULL_PREFIX_XMM15, 720.0);
                match culls.len() {
                    0 => {}
                    1 => {
                        if !verified.iter().any(|&(f, _)| f == target) {
                            verified.push((target, culls[0]));
                        }
                    }
                    n => {
                        log_warn!(
                            "  [-] note_collector -- candidate +0x{:X} has {} XMM15 720.0 loads (expected 1)",
                            target.offset_from(self.base) as usize,
                            n
                        );
                        return;
                    }
                }
            }
        }

        if verified.len() != 1 {
            log_warn!(
                "  [-] note_collector -- expected exactly 1 verified callee, got {}",
                verified.len()
            );
            return;
        }
        let (collector, cull_site) = verified[0];
        self.resolved.insert("note_collector".into(), collector);
        self.resolved
            .insert("collector_cull_site".into(), cull_site);
        unsafe {
            log_info!(
                "  [+] note_collector (derived) @ +0x{:X}; collector_cull_site @ +0x{:X}",
                collector.offset_from(self.base) as usize,
                cull_site.offset_from(self.base) as usize
            );
        }
    }

    /// Derive `guideline_draw`, `guideline_cull_site`, and
    /// `guideline_bulk_emitter` (see [`Self::derive_playfield_styling`]).
    ///
    /// The guideline draw's prologue AOB matches 3 functions on both
    /// supported builds, so candidates are classified by content: the real
    /// one (and only it) contains, within its first 0x800 bytes, BOTH the
    /// XMM9-form 720.0f load AND a `CALL get_offset_y`. Its bulk emitter is
    /// then the unique callee in the same window whose body starts with the
    /// verified command-header sequence (`ADD [RCX+0xC],0x10` …) and carries
    /// the tag-0x01 write + `count*0x14` stride math — and which has exactly
    /// ONE CALL xref module-wide (the transform detour assumes a private
    /// caller).
    fn derive_guideline_targets(&mut self) {
        /// Shared prologue of the guideline draw (and 2 unrelated functions):
        /// `MOV RAX,RSP; PUSH RBP/R12..R15; LEA RBP,[RAX-0x68]; SUB RSP,0x140`.
        const GUIDELINE_PROLOGUE: &str =
            "48 8B C4 55 41 54 41 55 41 56 41 57 48 8D 68 98 48 81 EC 40 01 00 00";
        /// `MOVSS XMM9, dword ptr [RIP+disp32]` opcode prefix.
        const CULL_PREFIX_XMM9: &[u8] = &[0xF3, 0x44, 0x0F, 0x10, 0x0D];
        const DRAW_WINDOW: usize = 0x800;
        /// Emitter body opening: `ADD dword [RCX+0xC],0x10; MOV EAX,[RCX+0xC]`.
        const EMITTER_HEAD: &[u8] = &[0x83, 0x41, 0x0C, 0x10, 0x8B, 0x41, 0x0C];
        /// Emitter tag/stride core: `MOV EAX,1; MOV [R10],AX` (command tag
        /// 0x01) + `LEA ECX,[R11+R11*4]; SHL ECX,2` (count*0x14 stride).
        const EMITTER_CORE: &[u8] = &[
            0xB8, 0x01, 0x00, 0x00, 0x00, 0x66, 0x41, 0x89, 0x02, 0x43, 0x8D, 0x0C, 0x9B, 0xC1,
            0xE1, 0x02,
        ];
        const EMITTER_SCAN: usize = 0x40;

        let get_offset_y = match self.get_address("get_offset_y") {
            Some(a) => a,
            None => {
                log_warn!("  [-] guideline_draw -- get_offset_y unresolved");
                return;
            }
        };

        let candidates = scan_pattern_all(self.base, self.size, GUIDELINE_PROLOGUE);
        let mut hits: Vec<(*const u8, *const u8)> = Vec::new(); // (draw fn, cull insn)
        for c in &candidates {
            let draw = c.address;
            let culls = self.find_rip_f32_loads(draw, DRAW_WINDOW, CULL_PREFIX_XMM9, 720.0);
            if culls.len() != 1 {
                continue;
            }
            let calls_offset_y = unsafe {
                let mod_lo = self.base as usize;
                let mod_hi = mod_lo + self.size;
                let end = (draw as usize + DRAW_WINDOW).min(mod_hi) - draw as usize;
                (0..end.saturating_sub(5)).any(|i| {
                    let p = draw.add(i);
                    *p == 0xE8 && decode_call_rel32(p) == get_offset_y
                })
            };
            if calls_offset_y {
                hits.push((draw, culls[0]));
            }
        }

        if hits.len() != 1 {
            log_warn!(
                "  [-] guideline_draw -- expected exactly 1 classified candidate, got {} (of {} prologue matches)",
                hits.len(),
                candidates.len()
            );
            return;
        }
        let (draw, cull_site) = hits[0];

        // Locate the bulk emitter among the draw's CALL targets.
        let mut emitters: Vec<*const u8> = Vec::new();
        unsafe {
            let mod_lo = self.base as usize;
            let mod_hi = mod_lo + self.size;
            let end = (draw as usize + DRAW_WINDOW).min(mod_hi) - draw as usize;
            for i in 0..end.saturating_sub(5) {
                let p = draw.add(i);
                if *p != 0xE8 {
                    continue;
                }
                let target = decode_call_rel32(p);
                let t = target as usize;
                if t < mod_lo || t + EMITTER_SCAN > mod_hi {
                    continue;
                }
                let body = std::slice::from_raw_parts(target, EMITTER_SCAN);
                if body.starts_with(EMITTER_HEAD)
                    && body.windows(EMITTER_CORE.len()).any(|w| w == EMITTER_CORE)
                    && !emitters.contains(&target)
                {
                    emitters.push(target);
                }
            }
        }
        if emitters.len() != 1 {
            log_warn!(
                "  [-] guideline_bulk_emitter -- expected exactly 1 verified callee, got {}",
                emitters.len()
            );
            return;
        }
        let emitter = emitters[0];

        // The transform detour assumes the emitter is private to the
        // guideline draw: require exactly one CALL xref module-wide.
        let xrefs = self.xrefs_to(emitter);
        if xrefs.len() != 1 {
            log_warn!(
                "  [-] guideline_bulk_emitter -- expected exactly 1 caller, got {}",
                xrefs.len()
            );
            return;
        }

        self.resolved.insert("guideline_draw".into(), draw);
        self.resolved
            .insert("guideline_cull_site".into(), cull_site);
        self.resolved
            .insert("guideline_bulk_emitter".into(), emitter);
        unsafe {
            log_info!(
                "  [+] guideline_draw (derived) @ +0x{:X}; cull_site @ +0x{:X}; bulk_emitter @ +0x{:X} (1 caller)",
                draw.offset_from(self.base) as usize,
                cull_site.offset_from(self.base) as usize,
                emitter.offset_from(self.base) as usize
            );
        }
    }

    /// Pre-compute `CALL rel32` xrefs to every target that any derivation
    /// chain in `resolve_derived` will need. One pass over the module
    /// instead of one pass per target.
    fn populate_xref_cache(&mut self) {
        // Collect (name, address) pairs for the targets we know derivation
        // methods will look up. Listed here once so adding a new derived
        // method that needs xrefs is a one-liner.
        const XREF_TARGETS: &[&str] = &["folder_register", "file_manager_load", "metadata_insert"];

        let targets: Vec<*const u8> = XREF_TARGETS
            .iter()
            .filter_map(|name| self.get_address(name))
            .collect();

        if targets.is_empty() {
            return;
        }

        let results =
            unsafe { crate::core::scanner::scan_xrefs_to_batch(self.base, self.size, &targets) };

        // Re-zip: targets[i] → results[i]. Some XREF_TARGETS entries may
        // have been filtered out as missing, so iterate XREF_TARGETS and
        // pull from `targets` / `results` only the ones that resolved.
        let mut t_iter = targets.into_iter();
        let mut r_iter = results.into_iter();
        for name in XREF_TARGETS {
            if self.get_address(name).is_some() {
                if let (Some(t), Some(r)) = (t_iter.next(), r_iter.next()) {
                    self.xref_cache.insert(t, r);
                }
            }
        }
    }

    pub fn get_address(&self, name: &str) -> Option<*const u8> {
        self.resolved.get(name).copied()
    }

    pub fn require_address(&self, name: &str) -> *const u8 {
        self.get_address(name)
            .unwrap_or_else(|| panic!("Required signature '{}' was not resolved", name))
    }

    /// Scan for ALL matches of a named signature. Returns empty Vec if not found.
    pub fn get_all_matches(&self, name: &str) -> Vec<*const u8> {
        let sig = match SIGNATURES.iter().find(|s| s.name == name) {
            Some(s) => s,
            None => return Vec::new(),
        };
        scan_pattern_all(self.base, self.size, sig.pattern)
            .into_iter()
            .map(|r| r.address)
            .collect()
    }

    // ── Derived address resolution ──────────────────────────────────

    fn derive_folder_functor_ctors(&mut self) {
        let folder_register = match self.get_address("folder_register") {
            Some(a) => a,
            None => return,
        };

        // Step 1: Find folder_init — the function that calls folder_register the most.
        // The xrefs were pre-computed by `populate_xref_cache` so this is an
        // O(1) HashMap lookup.
        let call_sites = self.xrefs_to(folder_register);

        if call_sites.len() < 6 {
            log_warn!(
                "  [-] folder_init -- expected >=6 calls to folder_register, found {}",
                call_sites.len()
            );
            return;
        }

        // Walk backwards from the first call site to find the function prologue (48 8B C4 = MOV RAX,RSP).
        let folder_init = unsafe {
            let first_site = call_sites[0];
            let mut found: Option<*const u8> = None;
            for back in 0..0x2000usize {
                let ci = first_site.sub(back);
                if ci < self.base {
                    break;
                }
                if *ci == 0x48 && *ci.add(1) == 0x8B && *ci.add(2) == 0xC4 {
                    // Verify it's a real prologue: next byte should be PUSH (0x55, 0x57, or 0x41 5x)
                    let next = *ci.add(3);
                    if next == 0x55 || next == 0x57 || next == 0x41 {
                        found = Some(ci);
                        break;
                    }
                }
            }
            found
        };

        let folder_init = match folder_init {
            Some(addr) => {
                self.resolved.insert("folder_init".into(), addr);
                let offset = unsafe { addr.offset_from(self.base) as usize };
                log_info!(
                    "  [+] folder_init (derived from folder_register xrefs) @ +0x{:X}",
                    offset
                );
                addr
            }
            None => {
                log_warn!("  [-] folder_init -- could not find function prologue");
                return;
            }
        };

        // Step 2: Estimate folder_init size from its epilogue.
        let init_size = {
            let bytes = unsafe { std::slice::from_raw_parts(folder_init, 0x4000) };
            // Look for POP; POP; POP; RET pattern near the end
            let mut size = 0x4000;
            for i in (0..0x4000 - 4).rev() {
                if bytes[i] == 0xC3 && (bytes[i - 1] == 0x5D || bytes[i - 1] == 0x5F) {
                    size = i + 1;
                    break;
                }
            }
            size
        };

        // Step 3: Within folder_init, find folder_store_ptr.
        // It's the CALL target immediately before each folder_register call.
        // Pattern: CALL folder_store_ptr; MOV RDX,RAX; LEA RCX,...; CALL folder_register
        unsafe {
            // For each folder_register call site within folder_init, find the preceding CALL
            for &site in &call_sites {
                let site_off = site.offset_from(folder_init) as usize;
                if site_off >= init_size {
                    continue;
                }

                // Scan backwards from the folder_register call to find the nearest E8 CALL
                for back in 5..64usize {
                    let prev = site.sub(back);
                    if prev < folder_init {
                        break;
                    }
                    if *prev == 0xE8 {
                        let target = decode_call_rel32(prev);
                        if target != folder_register {
                            if self.get_address("folder_store_ptr").is_none() {
                                self.resolved.insert("folder_store_ptr".into(), target);
                                let offset = target.offset_from(self.base) as usize;
                                log_info!("  [+] folder_store_ptr (derived) @ +0x{:X}", offset);
                            }
                            break;
                        }
                    }
                }
                if self.get_address("folder_store_ptr").is_some() {
                    break;
                }
            }

            // Step 4: Find folder_property_ctor.
            // Pattern: TEST RAX,RAX; JZ xx; MOV RCX,RAX; CALL folder_property_ctor
            // Bytes: 48 85 C0 74 ?? 48 8B C8 E8
            for i in 0..init_size.saturating_sub(13) {
                let p = folder_init.add(i);
                if *p == 0x48
                    && *p.add(1) == 0x85
                    && *p.add(2) == 0xC0
                    && *p.add(3) == 0x74
                    && *p.add(5) == 0x48
                    && *p.add(6) == 0x8B
                    && *p.add(7) == 0xC8
                    && *p.add(8) == 0xE8
                {
                    let target = decode_call_rel32(p.add(8));
                    self.resolved.insert("folder_property_ctor".into(), target);
                    let offset = target.offset_from(self.base) as usize;
                    log_info!("  [+] folder_property_ctor (derived) @ +0x{:X}", offset);
                    break;
                }
            }

            // Step 5: Find folder_functor_ctor and folder_filter_functor_ctor.
            // They're called in pairs: CALL functor_ctor; ...; CALL filter_functor_ctor
            // Both are called with the same EDX value (bit_index).
            // For FIRST STEP: XOR EDX,EDX (33 D2) precedes each.
            // Find the first pair of CALLs preceded by XOR EDX,EDX.
            let mut functor_pair: Vec<*const u8> = Vec::new();
            let mut i = 0;
            while i < init_size.saturating_sub(10) && functor_pair.len() < 2 {
                let p = folder_init.add(i);
                // Look for: 33 D2 (XOR EDX,EDX) ... E8 (CALL) within 16 bytes
                if *p == 0x33 && *p.add(1) == 0xD2 {
                    for j in 2..16usize {
                        if i + j + 5 > init_size {
                            break;
                        }
                        if *p.add(j) == 0xE8 {
                            let target = decode_call_rel32(p.add(j));
                            // Skip known targets (string builders, alloc, etc.)
                            let store_ptr = self.get_address("folder_store_ptr");
                            let prop_ctor = self.get_address("folder_property_ctor");
                            if Some(target) != store_ptr
                                && Some(target) != prop_ctor
                                && target != folder_register
                            {
                                functor_pair.push(target);
                                i += j + 5; // skip past this CALL
                            }
                            break;
                        }
                    }
                }
                i += 1;
            }

            if functor_pair.len() >= 2 {
                self.resolved
                    .insert("folder_functor_ctor".into(), functor_pair[0]);
                let offset = functor_pair[0].offset_from(self.base) as usize;
                log_info!("  [+] folder_functor_ctor (derived) @ +0x{:X}", offset);

                self.resolved
                    .insert("folder_filter_functor_ctor".into(), functor_pair[1]);
                let offset = functor_pair[1].offset_from(self.base) as usize;
                log_info!(
                    "  [+] folder_filter_functor_ctor (derived) @ +0x{:X}",
                    offset
                );
            } else {
                log_warn!(
                    "  [-] folder functor ctors -- expected 2 targets after XOR EDX,EDX, found {}",
                    functor_pair.len()
                );
            }
        }
    }

    fn derive_gameplay_obj_addresses(&mut self) {
        let alloc_site = match self.get_address("gameplay_obj_alloc") {
            Some(a) => a,
            None => return,
        };

        unsafe {
            // Size imm32 is at offset +1 from the B9 (MOV ECX, imm32)
            let size_addr = alloc_site.add(1);
            self.resolved
                .insert("gameplay_obj_alloc_size".into(), size_addr);
            let original_size = (size_addr as *const u32).read_unaligned();
            let offset = size_addr.offset_from(self.base) as usize;
            log_info!(
                "  [+] gameplay_obj_alloc_size (derived) @ +0x{:X} (current=0x{:X})",
                offset,
                original_size
            );

            // Constructor CALL is at offset +22 (E8 xx xx xx xx)
            let ctor_call = alloc_site.add(22);
            if *ctor_call != 0xE8 {
                log_warn!(
                    "  [-] gameplay_obj_ctor -- expected E8 at alloc+20, got 0x{:02X}",
                    *ctor_call
                );
                return;
            }
            let ctor_addr = decode_call_rel32(ctor_call);
            self.resolved.insert("gameplay_obj_ctor".into(), ctor_addr);
            let offset = ctor_addr.offset_from(self.base) as usize;
            log_info!("  [+] gameplay_obj_ctor (derived) @ +0x{:X}", offset);
        }
    }

    fn find_sprite_vtable(&mut self) {
        let vtable = match self.find_vtable_by_rtti(".?AVSprite@agcs@@", "sprite_vtable") {
            Some(v) => v,
            None => return,
        };
        self.resolved.insert("sprite_vtable".into(), vtable);
        let offset = unsafe { vtable.offset_from(self.base) as usize };
        log_info!("  [+] sprite_vtable (RTTI) @ +0x{:X}", offset);
    }

    fn find_check_step_data_actor(&mut self) {
        let rtti_name = ".?AVCheckStepDataActor@common@sequence@@";
        let vtable = match self.find_vtable_by_rtti(rtti_name, "check_step_data") {
            Some(v) => v,
            None => return,
        };

        self.resolved
            .insert("check_step_data_vtable".into(), vtable);
        let vt_off = unsafe { vtable.offset_from(self.base) as usize };
        log_info!("  [+] check_step_data_vtable (RTTI) @ +0x{:X}", vt_off);

        unsafe {
            // vtable[6] = per-frame update function
            let update_func = *(vtable.add(6 * 8) as *const *const u8);
            if update_func.is_null() {
                log_warn!("  [-] check_step_data_update -- vtable[6] is NULL");
                return;
            }
            let fn_off = update_func.offset_from(self.base) as usize;
            if fn_off >= self.size {
                log_warn!("  [-] check_step_data_update -- vtable[6] points outside module");
                return;
            }
            self.resolved
                .insert("check_step_data_update".into(), update_func);
            log_info!("  [+] check_step_data_update (vtable[6]) @ +0x{:X}", fn_off);

            // Find global table pointer via SHL+MOV pattern
            if let Some(global_ptr) = self.find_rip_load_near_shl(update_func, 256) {
                self.resolved
                    .insert("step_data_global_table".into(), global_ptr);
                log_info!(
                    "  [+] step_data_global_table (instruction scan) @ {:p}",
                    global_ptr
                );
            } else {
                log_warn!("  [-] step_data_global_table -- SHL+MOV pattern not found");
            }
        }
    }

    /// Derive the ultrafast-boot replay/pacing addresses from the resolved
    /// `CheckStepDataActor::onUpdate` body. All four are decoded from
    /// instructions inside onUpdate (no hardcoded absolutes) — see
    /// `docs/ultrafast_boot_research.md` §2 and the ultrafast-boot design's
    /// derivation table. Confirmed byte-exact on gamemdx 20260721.
    ///
    /// Soft on every seam: a missing anchor logs `[-]` and inserts nothing.
    /// The ultrafast-boot cache/pacing path gates on presence, so plain
    /// fast-bootup batching is unaffected by any miss here.
    fn derive_ultrafast_boot(&mut self) {
        // onUpdate body is ~0x8DA bytes on 20260721; a 0xC00 window covers it
        // without spilling far into the next function.
        const WINDOW: usize = 0xC00;
        /// `MOV byte ptr [RAX+0x1b0],1` — the corruption-flag write. Unique
        /// in the body; the 5 bytes before it are the `find_music_by_mcode`
        /// CALL.
        const FLAG_WRITE: [u8; 7] = [0xC6, 0x80, 0xB0, 0x01, 0x00, 0x00, 0x01];

        let update = match self.get_address("check_step_data_update") {
            Some(a) => a,
            None => {
                log_warn!("  [-] ultrafast_boot -- check_step_data_update unresolved");
                return;
            }
        };
        let mgr = match self.get_address("step_data_global_table") {
            Some(a) => a,
            None => {
                log_warn!("  [-] ultrafast_boot -- step_data_global_table unresolved");
                return;
            }
        };

        let base = self.base;
        let mod_lo = base as usize;
        let mod_hi = mod_lo + self.size;
        let in_module = |p: *const u8| (p as usize) >= mod_lo && (p as usize) < mod_hi;
        let end = (update as usize + WINDOW).min(mod_hi) - update as usize;

        // Small insert+log helper (keeps the four decodes uniform).
        macro_rules! record {
            ($name:literal, $opt:expr) => {
                match $opt {
                    Some(t) if in_module(t) => {
                        self.resolved.insert($name.into(), t);
                        log_info!("  [+] {} @ +0x{:X}", $name, t.offset_from(base) as usize);
                    }
                    _ => log_warn!("  [-] {} -- derivation anchor not found", $name),
                }
            };
        }

        unsafe {
            // (1) music_db_global = &DAT_1806f2d78: the first `MOV RCX,[rip]`
            //     (48 8B 0D) in the body. The manager global shares this
            //     opcode later on, so "first occurrence, and != manager"
            //     pins the music DB.
            let mut music_db: Option<*const u8> = None;
            for i in 0..end.saturating_sub(7) {
                let p = update.add(i);
                if *p == 0x48 && *p.add(1) == 0x8B && *p.add(2) == 0x0D {
                    let target = decode_rip_relative(p.add(3));
                    if in_module(target) && target != mgr {
                        music_db = Some(target);
                        break;
                    }
                }
            }
            record!("music_db_global", music_db);

            // (2) variable_bpm_threshold = &DAT_180393f40: the unique
            //     `MOVSD XMM8,[rip]` (F2 44 0F 10 05) in the prologue.
            let mut threshold: Option<*const u8> = None;
            for i in 0..end.saturating_sub(9) {
                let p = update.add(i);
                if *p == 0xF2
                    && *p.add(1) == 0x44
                    && *p.add(2) == 0x0F
                    && *p.add(3) == 0x10
                    && *p.add(4) == 0x05
                {
                    threshold = Some(decode_rip_relative(p.add(5)));
                    break;
                }
            }
            record!("variable_bpm_threshold", threshold);

            // (3) find_music_by_mcode = 0x1801b4290: the CALL immediately
            //     preceding the unique 0x1b0 corruption-flag write.
            let mut find_mcode: Option<*const u8> = None;
            'outer: for i in 5..end.saturating_sub(FLAG_WRITE.len()) {
                for (j, b) in FLAG_WRITE.iter().enumerate() {
                    if *update.add(i + j) != *b {
                        continue 'outer;
                    }
                }
                let call = update.add(i - 5);
                if *call == 0xE8 {
                    find_mcode = Some(decode_call_rel32(call));
                }
                break;
            }
            record!("find_music_by_mcode", find_mcode);

            // (4) step_data_release = 0x1801ff1b0: the `MOV RCX,[rip]` whose
            //     target IS the manager global and whose next instruction is
            //     a CALL (the post-side-loop release site).
            let mut release: Option<*const u8> = None;
            for i in 0..end.saturating_sub(12) {
                let p = update.add(i);
                if *p == 0x48
                    && *p.add(1) == 0x8B
                    && *p.add(2) == 0x0D
                    && decode_rip_relative(p.add(3)) == mgr
                    && *p.add(7) == 0xE8
                {
                    release = Some(decode_call_rel32(p.add(7)));
                    break;
                }
            }
            record!("step_data_release", release);
        }
    }

    fn find_scene_transition(&mut self) {
        let needle = "sequence::TransitionSequence::createNextSequence";
        if let Some(func_addr) = self.find_function_by_debug_string(needle, "scene_transition") {
            self.resolved.insert("scene_transition".into(), func_addr);
            let offset = unsafe { func_addr.offset_from(self.base) as usize };
            log_info!("  [+] scene_transition (string ref) @ +0x{:X}", offset);
        }
    }

    fn find_auto_foot_panel(&mut self) {
        let vtable = match self.find_vtable_by_rtti(".?AVAutoFootPanel@input@@", "auto_foot_panel")
        {
            Some(v) => v,
            None => return,
        };

        self.resolved
            .insert("auto_foot_panel_vtable".into(), vtable);
        let vt_off = unsafe { vtable.offset_from(self.base) as usize };
        log_info!("  [+] auto_foot_panel_vtable (RTTI) @ +0x{:X}", vt_off);

        unsafe {
            let update_func = *(vtable.add(8) as *const *const u8);
            if update_func.is_null() {
                log_warn!("  [-] auto_foot_panel_update -- vtable[1] is NULL");
                return;
            }
            self.resolved
                .insert("auto_foot_panel_update".into(), update_func);
            let fn_off = update_func.offset_from(self.base) as usize;
            log_info!("  [+] auto_foot_panel_update (vtable[1]) @ +0x{:X}", fn_off);
        }
    }

    fn find_judge_notes(&mut self) {
        let needle = "sequence::dance::GamePlayActor::judgeNotes";
        if let Some(func_addr) = self.find_function_by_debug_string(needle, "judge_notes") {
            self.resolved.insert("judge_notes".into(), func_addr);
            let offset = unsafe { func_addr.offset_from(self.base) as usize };
            log_info!("  [+] judge_notes (string ref) @ +0x{:X}", offset);
        }
    }

    /// Find the `GamePlayActor` vtable via RTTI. Used to filter actor-tree
    /// children to GamePlayActor instances when dispatching the
    /// `gauge::GAME_OVER` message for Quick Fail / Quick Restart.
    fn find_gameplay_actor_vtable(&mut self) {
        let rtti_name = ".?AVGamePlayActor@dance@sequence@@";
        let vtable = match self.find_vtable_by_rtti(rtti_name, "gameplay_actor_vtable") {
            Some(v) => v,
            None => return,
        };
        self.resolved.insert("gameplay_actor_vtable".into(), vtable);
        let offset = unsafe { vtable.offset_from(self.base) as usize };
        log_info!("  [+] gameplay_actor_vtable (RTTI) @ +0x{:X}", offset);
    }

    /// Derive `app_heap_handle` from `app_heap_reserve_anchor`.
    ///
    /// The reserve function for 12-byte-stride vectors has a fixed prologue
    /// shape. At `anchor + 0x7B` it does `MOV RCX, [RIP+disp32]` to load the
    /// heap-handle pointer, followed by `CALL agcs_heap_malloc` at `anchor +
    /// 0x82`. We decode the RIP-relative displacement to get the heap handle
    /// global, and cross-check that the CALL target matches the separately
    /// resolved `agcs_heap_malloc` signature.
    fn derive_app_heap_handle(&mut self) {
        let anchor = match self.get_address("app_heap_reserve_anchor") {
            Some(a) => a,
            None => {
                log_warn!("  [-] app_heap_handle -- reserve anchor not resolved");
                return;
            }
        };

        unsafe {
            // Expected instruction at anchor+0x7B: 48 8B 0D disp32 (MOV RCX,[RIP+disp32])
            let mov_site = anchor.add(0x7B);
            if *mov_site != 0x48 || *mov_site.add(1) != 0x8B || *mov_site.add(2) != 0x0D {
                log_warn!(
                    "  [-] app_heap_handle -- expected MOV RCX,[RIP+disp32] at anchor+0x7B, got {:02X} {:02X} {:02X}",
                    *mov_site, *mov_site.add(1), *mov_site.add(2)
                );
                return;
            }
            let handle_addr = decode_rip_relative(mov_site.add(3));
            self.resolved.insert("app_heap_handle".into(), handle_addr);
            let offset = handle_addr.offset_from(self.base) as usize;
            log_info!("  [+] app_heap_handle (derived) @ +0x{:X}", offset);

            // Cross-check: CALL at anchor+0x82 should target agcs_heap_malloc.
            let call_site = anchor.add(0x82);
            if *call_site == 0xE8 {
                let call_target = decode_call_rel32(call_site);
                if let Some(malloc) = self.get_address("agcs_heap_malloc") {
                    if call_target != malloc {
                        log_warn!(
                            "  [!] app_heap_reserve_anchor CALL target ({:p}) != agcs_heap_malloc ({:p}) -- one of the signatures may be mismatched",
                            call_target, malloc
                        );
                    }
                }
            }
        }
    }

    /// Derive the XACT-2 audio addresses `services::game_audio` consumes, from
    /// the three audio anchors. Nothing here calls a game function — it is
    /// address arithmetic over the module's own bytes.
    ///
    /// Three independent stages, each degrading on its own so one missing
    /// anchor cannot cost the others:
    ///
    /// - **match-count diagnostic** — `resolve_all` has first-match-per-name
    ///   semantics, so a second match anywhere in the module would be silent.
    ///   For `se_play_inner_body` that matters more than usual: its neighbour
    ///   `se_prepare_inner` is byte-for-byte identical for ~0x65 bytes, and
    ///   binding Prepare instead of Play would look like "no audio" several
    ///   steps later rather than failing here.
    /// - [`Self::derive_audio_manager_and_play`] — the manager global and the
    ///   inner play entry.
    /// - [`Self::derive_audio_named_bank_count`] — the free-slot safety gate.
    fn derive_game_audio_addresses(&mut self) {
        // Three whole-module single-needle scans. A deliberate boot cost:
        // these patterns' uniqueness is the assumption the rest of the audio
        // binding rests on, and this is the only place it is observable.
        let n_play = self.get_all_matches("se_play").len();
        let n_inner = self.get_all_matches("se_play_inner_body").len();
        let n_slot = self.get_all_matches("bank_slot_of_file_loop").len();
        log_info!(
            "  [+] audio signature match counts: se_play={} se_play_inner_body={} bank_slot_of_file_loop={}",
            n_play, n_inner, n_slot
        );
        // A count of 0 is already reported as `[-]` by `resolve_all`.
        for (name, count) in [
            ("se_play", n_play),
            ("se_play_inner_body", n_inner),
            ("bank_slot_of_file_loop", n_slot),
        ] {
            if count > 1 {
                log_warn!(
                    "  [!] {} matched {} times -- the pattern is not unique on this build and resolve_all took the first match; verify against the per-build table in research/bank-slot-and-anchors.md",
                    name, count
                );
            }
        }

        self.derive_audio_manager_and_play();
        self.derive_audio_named_bank_count();
    }

    /// Chains A and B of the audio derivation (see
    /// `.agents/planning/20260725-assist-tick/research/bank-slot-and-anchors.md`
    /// → "Derivation chains").
    ///
    /// **Chain A.** The `se_play_inner_body` match sits at `se_play_inner + 0xF`,
    /// on the instruction that loads the audio-manager singleton:
    ///
    /// ```text
    /// -0x0F  48 89 5C 24 08 …        ; se_play_inner prologue (15 bytes)
    /// +0x00  MOV RSI,[rip+disp32]    ; disp32 at +3 -> audio_manager_global
    /// +0x07  MOVSXD RDI,ECX          ; bank_id
    /// +0x0F  LEA RAX,[RDI+1] …       ; bank = *(mgr + ((bank_id+1)*2)*8)
    /// +0x56  CALL [R10+0x20]         ; SoundBank::Play — Play-vs-Prepare discriminator
    /// ```
    ///
    /// The manager global's absolute address **moves on every game build** (four
    /// distinct addresses across the four verified builds), so it is RIP-decoded
    /// from this anchor rather than scanned for or hardcoded. The `-0xF` entry
    /// offset is only trusted after the prologue bytes are confirmed there.
    ///
    /// **Chain B.** `se_play`'s first `CALL rel32` must land on the derived inner
    /// entry — the same style of corroboration
    /// [`Self::derive_app_heap_handle`] does on its `CALL` target. Disagreement
    /// means one of the two patterns mis-resolved, and is a warning rather than a
    /// failure because nothing consumes these addresses until `game_audio` asks
    /// for them.
    fn derive_audio_manager_and_play(&mut self) {
        /// `se_play_inner`'s prologue, byte-identical on all four verified builds.
        const INNER_PROLOGUE: &[u8] = &[
            0x48, 0x89, 0x5C, 0x24, 0x08, 0x48, 0x89, 0x74, 0x24, 0x10, 0x57, 0x48, 0x83, 0xEC,
            0x40,
        ];
        /// Distance from the `se_play_inner_body` match back to the function entry.
        const BODY_TO_ENTRY: usize = 0x0F;
        /// Offset of the `MOV RSI,[rip+disp32]` displacement within the match.
        const MGR_DISP: usize = 3;
        /// Window searched for `se_play`'s first `CALL rel32` (it is at entry+0x73).
        const CALL_WINDOW: usize = 0x80;

        let anchor = match self.get_address("se_play_inner_body") {
            Some(a) => a,
            None => {
                log_warn!(
                    "  [-] audio_manager_global/se_play_inner -- se_play_inner_body anchor unresolved"
                );
                return;
            }
        };

        let base = self.base;
        let size = self.size;
        let module_end = (base as usize).saturating_add(size);
        // Plain integer arithmetic, so it stays valid for a pointer that turned
        // out not to be in the module (which is exactly the case being logged).
        let rel = move |p: *const u8| (p as usize).wrapping_sub(base as usize);
        let in_module =
            move |p: *const u8, len: usize| p as usize >= base as usize && rel(p) + len <= size;

        unsafe {
            // The three bytes before the displacement are literal in the
            // pattern, so a match guarantees the instruction shape; what it
            // cannot guarantee is that the displacement lands in the module.
            let mgr = decode_rip_relative(anchor.add(MGR_DISP));
            if !in_module(mgr, std::mem::size_of::<usize>()) {
                log_warn!(
                    "  [-] audio_manager_global -- RIP target {:p} is outside the module [{:p}, 0x{:X})",
                    mgr, base, module_end
                );
                return;
            }
            self.resolved.insert("audio_manager_global".into(), mgr);
            log_info!(
                "  [+] audio_manager_global (derived, se_play_inner_body RIP disp32) @ +0x{:X}",
                rel(mgr)
            );

            let mut verified: Option<*const u8> = None;
            if rel(anchor) >= BODY_TO_ENTRY {
                let candidate = anchor.sub(BODY_TO_ENTRY);
                let actual = std::slice::from_raw_parts(candidate, INNER_PROLOGUE.len());
                if actual == INNER_PROLOGUE {
                    verified = Some(candidate);
                } else {
                    log_warn!(
                        "  [!] se_play_inner -- prologue mismatch at se_play_inner_body-0x{:X}: got {:02X?}; falling back to find_function_entry",
                        BODY_TO_ENTRY, actual
                    );
                }
            }
            let inner = match verified {
                Some(p) => {
                    log_info!(
                        "  [+] se_play_inner (derived, prologue verified) @ +0x{:X}",
                        rel(p)
                    );
                    p
                }
                None => {
                    let p = find_function_entry(anchor, base);
                    log_info!(
                        "  [+] se_play_inner (derived via find_function_entry) @ +0x{:X}",
                        rel(p)
                    );
                    p
                }
            };
            self.resolved.insert("se_play_inner".into(), inner);

            // Chain B. A missing `se_play` is already reported by `resolve_all`,
            // so it gets no second warning here.
            if let Some(se_play) = self.get_address("se_play") {
                match scan_first_call_rel32(se_play, CALL_WINDOW) {
                    Some(target) if target == inner => {}
                    Some(target) => log_warn!(
                        "  [!] se_play (+0x{:X}) first CALL rel32 targets +0x{:X} but se_play_inner derived as +0x{:X} -- one of the two audio signatures has mis-resolved",
                        rel(se_play), rel(target), rel(inner)
                    ),
                    None => log_warn!(
                        "  [!] se_play (+0x{:X}) -- no CALL rel32 within 0x{:X} bytes; cannot corroborate se_play_inner",
                        rel(se_play), CALL_WINDOW
                    ),
                }
            }
        }
    }

    /// Chain C: publish the address of `bank_slot_of_file`'s named-bank count
    /// (the `CMP EBX,imm8` bound of its name-match loop) and report the value.
    ///
    /// The mapper returns `{0,1,2,3}` for the four named banks and the literal
    /// `5` for anything else, which is what leaves slot 4 permanently free and
    /// claimable. A build that added a fifth named bank would map it to slot 4
    /// and silently collide with our bank, so `register_bank` re-reads this byte
    /// and declines when it is not 4 (guard G1). The **address** is published
    /// rather than the value because the store maps names to addresses; keeping
    /// the offset here keeps it out of the service.
    fn derive_audio_named_bank_count(&mut self) {
        /// Offset of the `CMP EBX,imm8` immediate within the match.
        const COUNT_IMM8: usize = 0x2C;
        /// bgm_menu, se_system, se_normal, voice.
        const EXPECTED: u8 = 4;

        let anchor = match self.get_address("bank_slot_of_file_loop") {
            Some(a) => a,
            None => {
                log_warn!(
                    "  [-] audio_named_bank_count_site -- bank_slot_of_file_loop anchor unresolved"
                );
                return;
            }
        };

        let base = self.base;
        unsafe {
            let site = anchor.add(COUNT_IMM8);
            if (site as usize).wrapping_sub(base as usize) >= self.size {
                log_warn!(
                    "  [-] audio_named_bank_count_site -- {:p} is outside the module",
                    site
                );
                return;
            }
            let count = *site;
            self.resolved
                .insert("audio_named_bank_count_site".into(), site);
            log_info!(
                "  [+] audio_named_bank_count_site @ +0x{:X} (named bank count = {})",
                site.offset_from(base) as usize,
                count
            );
            if count != EXPECTED {
                log_warn!(
                    "  [!] named bank count is {}, expected {} -- a game build has added a named sound bank, so the free-slot assumption may no longer hold and assist tick should decline to register its own bank",
                    count, EXPECTED
                );
            }
        }
    }

    /// Resolve the Training-Mode strip HUD's runtime-validation anchors:
    /// the `screen::ArrowPalette` and `screen::ArrowRenderer` vftable
    /// addresses (RTTI walk). The strip's per-song snapshot reads the
    /// GamePlayActor's palette manager (`actor+0x130`) and arrow renderer
    /// (`actor+0x148` — the actor-init decompile's `param_1[0x26]` /
    /// `param_1[0x29]` stores) and requires each object's vptr to equal
    /// the matching vftable before ANY use — offset drift on a future
    /// build shows up as a vtable mismatch (⇒ the flat-color fallback
    /// ladder), never a wild virtual call. Both optional: a miss only
    /// degrades the strip's coloring. RE: docs/chart_strip_hud_research.md
    /// §4 + the 2026-08-14 actor-init decompile (task-02 record).
    fn derive_strip_hud_anchors(&mut self) {
        for (rtti, name) in [
            (".?AVArrowPalette@screen@@", "arrow_palette_vtable"),
            (".?AVArrowRenderer@screen@@", "arrow_renderer_vtable"),
        ] {
            if let Some(vt) = self.find_vtable_by_rtti(rtti, name) {
                self.resolved.insert(name.into(), vt);
                log_info!("  [+] {} (RTTI) @ {:p}", name, vt);
            }
            // find_vtable_by_rtti logs its own [-] on failure.
        }
    }

    /// Derive `player_option_table` — the per-side context table whose
    /// entries lead to each side's `ddr::player::Option` — from the
    /// `player_option_ctx_load` anchor (see the signature's comment for the
    /// instruction sequence and the RE record reference).
    ///
    /// The count function reads the table as `[R12 + side*8 + disp32]` with
    /// R12 pre-loaded to the **module base** via `LEA R12,[rip+disp32]`, so
    /// the table's address is `base + disp32` — but only if that LEA really
    /// resolves to the base. That is validated here rather than assumed: a
    /// compiler change that anchored R12 elsewhere would silently shift
    /// every table read, so a mismatch drops the derivation (and with it the
    /// assist-tick mod, which requires this name — fail-closed, NFR-4).
    ///
    /// Consumption (assist-tick, per song at build time):
    /// `Option(side) = *( *(table + side*8) ) + 0xE0`, JUDGMENT TIMING
    /// (`timing_music`, ±100 ms) at `Option + 0x24`.
    fn derive_player_option_table(&mut self) {
        /// Offset of the `LEA R12,[rip+disp32]` displacement within the match.
        const LEA_DISP: usize = 22;
        /// Offset of the table-load `MOV RCX,[R12+RCX*8+disp32]` displacement
        /// (the `49 8B 8C CC` opcode+ModRM+SIB spans match+42..46; the disp32
        /// follows). Off-by-one here reads the 0xCC SIB byte into the value —
        /// which is precisely what the out-of-module check below caught on
        /// the first deploy of this derivation.
        const TABLE_DISP: usize = 46;

        let anchor = match self.get_address("player_option_ctx_load") {
            Some(a) => a,
            None => {
                log_warn!("  [-] player_option_table -- player_option_ctx_load anchor unresolved");
                return;
            }
        };
        let base = self.base;
        unsafe {
            let lea_target = decode_rip_relative(anchor.add(LEA_DISP));
            if lea_target != base {
                log_warn!(
                    "  [-] player_option_table -- the anchor's LEA resolves to {:p}, not the module base {:p}; refusing to derive",
                    lea_target,
                    base
                );
                return;
            }
            let disp = (anchor.add(TABLE_DISP) as *const u32).read_unaligned() as usize;
            if disp >= self.size {
                log_warn!(
                    "  [-] player_option_table -- table displacement 0x{:X} is outside the module (size 0x{:X})",
                    disp,
                    self.size
                );
                return;
            }
            let table = base.add(disp);
            self.resolved.insert("player_option_table".into(), table);
            log_info!(
                "  [+] player_option_table (derived, base-validated LEA + disp32) @ +0x{:X}",
                disp
            );
        }
    }

    /// Derive `file_manager_singleton` from xrefs to `file_manager_load`.
    ///
    /// The engine's file-loading call sites follow a consistent pattern:
    ///
    /// ```text
    /// MOV Rxx, [RIP+disp32]     ; load FileManager singleton pointer
    /// ...                        ; (0–16 bytes of setup)
    /// MOV RCX, Rxx              ; this = singleton
    /// CALL file_manager_load
    /// ```
    ///
    /// We find the first CALL xref to file_manager_load, scan backwards
    /// for the RIP-relative MOV that loads the singleton, and decode the
    /// displacement.
    fn derive_file_manager_singleton(&mut self) {
        let fm_load = match self.get_address("file_manager_load") {
            Some(a) => a,
            None => return,
        };

        let call_sites = self.xrefs_to(fm_load);
        if call_sites.is_empty() {
            log_warn!("  [-] file_manager_singleton -- no xrefs to file_manager_load");
            return;
        }

        // Scan backwards from each call site for MOV Rxx, [RIP+disp32].
        // Encoding: 48 8B {0D|1D|3D|...} disp32  (REX.W MOV reg, [RIP+disp32])
        // The ModRM byte's low 3 bits = 101 (RIP-relative), bits 3-5 = dest reg.
        unsafe {
            for &site in &call_sites {
                for back in 5..48usize {
                    let p = site.sub(back);
                    if p < self.base {
                        break;
                    }
                    // REX.W prefix (48 or 4C) + MOV opcode (8B) + ModRM with mod=00, rm=101
                    let rex = *p;
                    if (rex != 0x48 && rex != 0x4C) || *p.add(1) != 0x8B {
                        continue;
                    }
                    let modrm = *p.add(2);
                    if (modrm & 0xC7) != 0x05 {
                        // Not [RIP+disp32] addressing mode
                        continue;
                    }
                    let singleton_addr = decode_rip_relative(p.add(3));
                    // Sanity: must point into the module's .data/.rdata section
                    let off = singleton_addr.offset_from(self.base) as usize;
                    if off >= self.size {
                        continue;
                    }
                    self.resolved
                        .insert("file_manager_singleton".into(), singleton_addr);
                    log_info!(
                        "  [+] file_manager_singleton (derived from file_manager_load xref) @ +0x{:X}",
                        off,
                    );
                    return;
                }
            }
        }
        log_warn!("  [-] file_manager_singleton -- no RIP-relative MOV found near file_manager_load call sites");
    }

    /// Derive render-pipeline globals from the `render_notes` function body.
    ///
    /// The function contains inlined CommandList writes and calls to helper
    /// functions. We locate three derived addresses:
    ///
    /// - `screen_renderer_state`: the global that holds the CommandList
    ///   pointer array, found via `MOV Rxx, [RIP+disp32]` after the
    ///   blend-mode write.
    /// - `default_shader`: the shader used for the silver shock-arrow
    ///   glyph pass, found inside the shock-arrow render helper (the
    ///   CALL target just before the blend-mode write).
    /// - `set_render_state`: the function that flushes blend-mode changes
    ///   to the CommandList, found as the CALL immediately after the
    ///   blend-mode write.
    fn derive_render_globals(&mut self) {
        let render_notes = match self.get_address("render_notes") {
            Some(a) => a,
            None => return,
        };

        // Scan for `MOV dword [RSI+0x2C], 0x2` — the additive blend mode
        // write that precedes the lightning overlay. Encoding: C7 46 2C 02 00 00 00.
        // This is distinctive: literal 2 written to a fixed struct offset.
        const BLEND_WRITE: [u8; 7] = [0xC7, 0x46, 0x2C, 0x02, 0x00, 0x00, 0x00];
        const SCAN_LEN: usize = 0x300;

        let body = unsafe { std::slice::from_raw_parts(render_notes, SCAN_LEN) };
        let blend_off = match body.windows(7).position(|w| w == BLEND_WRITE) {
            Some(off) => off,
            None => {
                log_warn!(
                    "  [-] render globals -- blend-mode write pattern not found in render_notes"
                );
                return;
            }
        };

        unsafe {
            // set_render_state: the E8 CALL within ~16 bytes AFTER the blend write.
            // Pattern: MOV RCX, RSI (48 8B CE) then E8 <disp32>.
            let search_start = blend_off + 7;
            let mut found_srs = false;
            let mut srs_call_end: usize = search_start + 20; // fallback
            #[allow(clippy::needless_range_loop)]
            for i in search_start..std::cmp::min(search_start + 20, SCAN_LEN - 5) {
                if body[i] == 0xE8 {
                    let call_addr = render_notes.add(i);
                    let target = decode_call_rel32(call_addr);
                    self.resolved.insert("set_render_state".into(), target);
                    let off = target.offset_from(self.base) as usize;
                    log_info!(
                        "  [+] set_render_state (derived from render_notes blend write) @ +0x{:X}",
                        off
                    );
                    found_srs = true;
                    srs_call_end = i + 5;
                    break;
                }
            }
            if !found_srs {
                log_warn!("  [-] set_render_state -- no CALL found after blend-mode write");
            }

            // screen_renderer_state: the first RIP-relative MOV to R10 or R9
            // (4C 8B 15 or 4C 8B 0D) within ~32 bytes after set_render_state.
            for i in srs_call_end..std::cmp::min(srs_call_end + 40, SCAN_LEN - 7) {
                if body[i] == 0x4C
                    && body[i + 1] == 0x8B
                    && (body[i + 2] == 0x15 || body[i + 2] == 0x0D)
                {
                    let global = decode_rip_relative(render_notes.add(i + 3));
                    let off = global.offset_from(self.base) as usize;
                    if off < self.size {
                        self.resolved.insert("screen_renderer_state".into(), global);
                        log_info!(
                            "  [+] screen_renderer_state (derived from render_notes) @ +0x{:X}",
                            off,
                        );
                    }
                    break;
                }
            }

            // default_shader: inside the shock-arrow render helper, which is
            // the E8 CALL just BEFORE the blend-mode write. Scan backwards.
            let mut shock_helper: Option<*const u8> = None;
            for i in (0..blend_off).rev() {
                if body[i] == 0xE8 {
                    shock_helper = Some(decode_call_rel32(render_notes.add(i)));
                    break;
                }
            }
            if let Some(helper) = shock_helper {
                // Scan the helper's first ~128 bytes for MOV RAX, [RIP+disp32]
                // (48 8B 05 <disp32>) that loads the default shader global.
                let helper_body = std::slice::from_raw_parts(helper, 128);
                for i in 0..helper_body.len() - 7 {
                    if helper_body[i] == 0x48
                        && helper_body[i + 1] == 0x8B
                        && helper_body[i + 2] == 0x05
                    {
                        let shader_global = decode_rip_relative(helper.add(i + 3));
                        let off = shader_global.offset_from(self.base) as usize;
                        if off < self.size {
                            self.resolved.insert("default_shader".into(), shader_global);
                            log_info!(
                                "  [+] default_shader (derived from shock-arrow helper) @ +0x{:X}",
                                off,
                            );
                        }
                        break;
                    }
                }
            } else {
                log_warn!(
                    "  [-] default_shader -- shock-arrow helper CALL not found before blend write"
                );
            }
        }
    }

    /// Derive `layer_table` (the render layer-table global) from the
    /// `layer_dispatcher` signature: the dispatcher's first RIP-relative
    /// load (`48 8B 15 <disp32>` at match+10, operand at +13) reads the
    /// table pointer. The overlay-draw animated-background emitter detours
    /// the dispatcher and replicates its per-entry walk conditions to pick
    /// the widget layer's list. RE: docs/overlay_draw_research.md.
    fn derive_layer_table(&mut self) {
        let dispatcher = match self.get_address("layer_dispatcher") {
            Some(a) => a,
            None => return,
        };
        unsafe {
            let body = std::slice::from_raw_parts(dispatcher, 16);
            // Structural check: the load must be exactly where the pattern
            // fixed it (48 8B 15 at +10).
            if body[10] != 0x48 || body[11] != 0x8B || body[12] != 0x15 {
                log_warn!("  [-] layer_table -- dispatcher prologue shape unexpected");
                return;
            }
            let global = decode_rip_relative(dispatcher.add(13));
            let off = global.offset_from(self.base) as usize;
            if off < self.size {
                self.resolved.insert("layer_table".into(), global);
                log_info!(
                    "  [+] layer_table (derived from layer_dispatcher) @ +0x{:X}",
                    off
                );
            } else {
                log_warn!("  [-] layer_table -- derived global outside module");
            }
        }
    }

    /// Derive `player_work_table` from the short accessor function anchored
    /// by `player_work_table_anchor`.
    /// The anchor's first instruction is `MOV RAX, [RIP+disp32]` loading
    /// the table pointer for slot 0, with the RIP-relative operand at
    /// offset +3 into the anchor. Decoding that operand yields the
    /// address of the table global itself (whose entries are 8-byte
    /// wrapper pointers, indexed by playSide).
    fn derive_player_work_table(&mut self) {
        let anchor = match self.get_address("player_work_table_anchor") {
            Some(a) => a,
            None => {
                log_warn!("  [-] player_work_table -- anchor not resolved");
                return;
            }
        };

        unsafe {
            // Sanity-check the three-byte MOV opcode prefix at the anchor.
            if *anchor != 0x48 || *anchor.add(1) != 0x8B || *anchor.add(2) != 0x05 {
                log_warn!(
                    "  [-] player_work_table -- unexpected opcode at anchor ({:02X} {:02X} {:02X})",
                    *anchor,
                    *anchor.add(1),
                    *anchor.add(2)
                );
                return;
            }
            let table = decode_rip_relative(anchor.add(3));
            let off = table.offset_from(self.base) as usize;
            if off >= self.size {
                log_warn!(
                    "  [-] player_work_table -- derived address {:p} lies outside module",
                    table
                );
                return;
            }
            self.resolved.insert("player_work_table".into(), table);
            log_info!("  [+] player_work_table (derived) @ +0x{:X}", off);
        }
    }

    /// Resolve `max_stage_global` — the operator's
    /// `/gameOptions/max_stage/current` cache (`DAT_18047E784` on 20260721),
    /// read once per session start inside `createNextSequence` case 7:
    ///
    ///   LEA RDX,[global]                       ; 48 8D 15 d32 — the out-pointer
    ///   LEA RCX,["/gameOptions/max_stage/current"]  ; 48 8D 0D d32
    ///   CALL [avs property read]
    ///
    /// Anchored on the unique string bytes, then the (single) RIP-relative
    /// `LEA RCX` xref, then the `LEA RDX` immediately before it. Fails closed
    /// on any ambiguity. Consumed by stage_records' session-state decode
    /// (the quick-fail fast path's session-continues predicate).
    fn derive_max_stage_global(&mut self) {
        const STRING_PATTERN: &str =
            "2F 67 61 6D 65 4F 70 74 69 6F 6E 73 2F 6D 61 78 5F 73 74 61 67 65 2F 63 75 72 72 65 6E 74 00";

        let hits = scan_pattern_all(self.base, self.size, STRING_PATTERN);
        if hits.len() != 1 {
            log_warn!(
                "  [-] max_stage_global -- expected 1 string match, found {}",
                hits.len()
            );
            return;
        }
        let string_addr = hits[0].address;

        unsafe {
            // All RIP-relative LEAs targeting the string, filtered to RCX
            // (ModRM 0x0D) — the property-key argument load.
            let leas: Vec<*const u8> =
                crate::core::scanner::scan_lea_xrefs_to(self.base, self.size, string_addr)
                    .into_iter()
                    .filter(|lea| *lea.add(2) == 0x0D)
                    .collect();
            if leas.len() != 1 {
                log_warn!(
                    "  [-] max_stage_global -- expected 1 LEA RCX xref to the key string, found {}",
                    leas.len()
                );
                return;
            }
            let lea_rcx = leas[0];

            // The out-pointer LEA RDX (48 8D 15 d32) sits immediately before.
            let lea_rdx = lea_rcx.sub(7);
            if *lea_rdx != 0x48 || *lea_rdx.add(1) != 0x8D || *lea_rdx.add(2) != 0x15 {
                log_warn!(
                    "  [-] max_stage_global -- expected LEA RDX before the key LEA ({:02X} {:02X} {:02X})",
                    *lea_rdx,
                    *lea_rdx.add(1),
                    *lea_rdx.add(2)
                );
                return;
            }
            let global = decode_rip_relative(lea_rdx.add(3));
            let off = global.offset_from(self.base) as usize;
            if off >= self.size {
                log_warn!(
                    "  [-] max_stage_global -- derived address {:p} lies outside module",
                    global
                );
                return;
            }
            self.resolved.insert("max_stage_global".into(), global);
            log_info!("  [+] max_stage_global (derived) @ +0x{:X}", off);
        }
    }

    /// Derive `shutter_actor_global` — the ShutterActor singleton pointer
    /// global (`DAT_1806f2d40` on 20260721) — from the `shutter_close_request`
    /// wrapper's `MOV RBX,[rip+d32]` at match+9. The opcode bytes are literal
    /// in the pattern, so only the RIP decode + a bounds check remain here.
    /// Consumed by the quick-restart/fail bannerless fast path.
    fn derive_shutter_actor_global(&mut self) {
        let wrapper = match self.get_address("shutter_close_request") {
            Some(w) => w,
            None => {
                log_warn!("  [-] shutter_actor_global -- shutter_close_request not resolved");
                return;
            }
        };
        unsafe {
            // match+9: 48 8B 1D d32 (MOV RBX,[rip+d32]); d32 at match+12.
            let global = decode_rip_relative(wrapper.add(12));
            let off = global.offset_from(self.base) as usize;
            if off >= self.size {
                log_warn!(
                    "  [-] shutter_actor_global -- derived address {:p} lies outside module",
                    global
                );
                return;
            }
            self.resolved.insert("shutter_actor_global".into(), global);
            log_info!("  [+] shutter_actor_global (derived) @ +0x{:X}", off);
        }
    }

    /// Derive `selectmusic_model` from `selectmusic_model_anchor`.
    ///
    /// The anchor is the MusicCard tick's `MOV R11,[rip+d32]` (d32 at
    /// match+3) whose following instructions read the highlighted-song
    /// shared_ptr at `[R11+0x1B8]` / `[R11+0x1B0]` — those offsets are
    /// pinned as immediate bytes in the pattern, so a successful match
    /// certifies both the global and the +0x1B0 object-slot layout.
    /// Consumer: music_wheel_song_length (selection-changed polling).
    fn derive_selectmusic_model(&mut self) {
        let anchor = match self.get_address("selectmusic_model_anchor") {
            Some(a) => a,
            None => {
                log_warn!("  [-] selectmusic_model -- anchor not resolved");
                return;
            }
        };
        unsafe {
            let global = decode_rip_relative(anchor.add(3));
            let off = global.offset_from(self.base) as usize;
            if off >= self.size {
                log_warn!(
                    "  [-] selectmusic_model -- derived address {:p} lies outside module",
                    global
                );
                return;
            }
            self.resolved.insert("selectmusic_model".into(), global);
            log_info!("  [+] selectmusic_model (derived) @ +0x{:X}", off);
        }
    }

    /// Derive `frame_tick_global` from `dps_timing_anchor_site`.
    ///
    /// The site is `MOV RAX,[rip+d32]` (d32 at match+3) followed by the
    /// `[RAX+0x1268]` frame-tick read that DPS state 6 broadcasts as the
    /// msg-0x1044 timing anchor. The pattern matches once on 20260616 /
    /// 20260721 and twice on 20250805 (a second state machine shares the
    /// shape); every match must decode to the SAME global or the
    /// derivation refuses — the in-place reset must never anchor the music
    /// clock to the wrong time source.
    fn derive_frame_tick_global(&mut self) {
        let matches = self.get_all_matches("dps_timing_anchor_site");
        if matches.is_empty() {
            log_warn!("  [-] frame_tick_global -- dps_timing_anchor_site not resolved");
            return;
        }
        unsafe {
            let mut global: Option<*const u8> = None;
            for m in &matches {
                let g = decode_rip_relative(m.add(3));
                match global {
                    None => global = Some(g),
                    Some(prev) if prev != g => {
                        log_warn!(
                            "  [-] frame_tick_global -- anchor sites disagree ({:p} vs {:p}); refusing",
                            prev,
                            g
                        );
                        return;
                    }
                    _ => {}
                }
            }
            let global = global.unwrap();
            let off = global.offset_from(self.base) as usize;
            if off >= self.size {
                log_warn!(
                    "  [-] frame_tick_global -- derived address {:p} lies outside module",
                    global
                );
                return;
            }
            self.resolved.insert("frame_tick_global".into(), global);
            log_info!(
                "  [+] frame_tick_global (derived from {} anchor site(s)) @ +0x{:X}",
                matches.len(),
                off
            );
        }
    }

    /// Find the gauge-actor family + ScoreActor + ControlMessageActor +
    /// NoteResultActor vtables via RTTI. The in-place song reset restores
    /// each side's gauge child by matching its vtable against this set
    /// (value at gauge+0x90, latches +0xB8/+0xD9) and resets the
    /// ScoreActor's displayed-score sentinel (+0x6C = -1 → full digit
    /// repaint) — see
    /// .agents/planning/20260812-inplace-restart/research/run_state_re.md
    /// §5. The ControlMessageActor vtable identifies each side's
    /// end-cascade child for the seek clamp (chart_end_raw at +0x98,
    /// StackStep — training research §4.3). The NoteResultActor vtable
    /// identifies each side's judge-display child, whose
    /// `dance_score_compare` clip (+0xB0) is the pacemaker readout: the
    /// reset rewinds it out of the msg-0x103A "out" outro (a one-way
    /// latch for the actor's lifetime — the msg-0x1036 update refuses
    /// once the clip's frame reaches the "out" label), and the pacemaker
    /// ms-error swap vtable-guards its visibility write against it. All
    /// are resolved fail-open per class; `song_reset` gates itself on the
    /// sets it needs.
    fn find_gauge_vtables(&mut self) {
        for (rtti, name) in [
            (
                ".?AVNormalGaugeActor@dance@sequence@@",
                "normal_gauge_vtable",
            ),
            (".?AVGradeGaugeActor@dance@sequence@@", "grade_gauge_vtable"),
            (".?AVLifeGaugeActor@dance@sequence@@", "life_gauge_vtable"),
            (".?AVFlareGaugeActor@dance@sequence@@", "flare_gauge_vtable"),
            (
                ".?AVImmortalGaugeActor@dance@sequence@@",
                "immortal_gauge_vtable",
            ),
            (".?AVScoreActor@dance@sequence@@", "score_actor_vtable"),
            (
                ".?AVControlMessageActor@dance@sequence@@",
                "control_message_actor_vtable",
            ),
            (
                ".?AVNoteResultActor@dance@sequence@@",
                "note_result_actor_vtable",
            ),
        ] {
            if let Some(vtable) = self.find_vtable_by_rtti(rtti, name) {
                self.resolved.insert(name.into(), vtable);
                let offset = unsafe { vtable.offset_from(self.base) as usize };
                log_info!("  [+] {} (RTTI) @ +0x{:X}", name, offset);
            }
        }
    }

    /// Derive the judge-record rebuild trio (seek-to-T, training design
    /// §4.4) from `judge_rebuild_anchor` — the msg-0x1044 rewind worker's
    /// anchor stores. The FIRST call is pinned by its records-vector
    /// argument setup (`LEA RCX,[R12+0xB0]` immediately before the E8 —
    /// refuses on layout drift); the next two E8s are reserve and rebuild
    /// (the flash-renderer virtual call in between is `FF 50 10`, never
    /// E8; region verified byte-identical on 20260616/20260721). Fail-open:
    /// any check failing leaves the trio unresolved — nonzero-T seeks
    /// refuse, the shipped t=0 reset is unaffected.
    fn derive_judge_rebuild_trio(&mut self) {
        let anchor = match self.get_address("judge_rebuild_anchor") {
            Some(a) => a,
            None => {
                log_warn!("  [-] judge_rebuild_trio -- judge_rebuild_anchor not resolved");
                return;
            }
        };
        // `LEA RCX,[R12+0xB0]` — the records-vector argument load.
        const VECTOR_LEA: [u8; 8] = [0x49, 0x8D, 0x8C, 0x24, 0xB0, 0x00, 0x00, 0x00];
        // The trio calls sit at match+0x37/+0x5F/+0x93 on 20260616/20260721
        // (cabinet-verified 2026-08-13: a 0x60 window truncated the scan
        // after the second call); the NEXT unrelated call is at +0xE0, and
        // the scan stops at three targets anyway.
        const SCAN_LIMIT: usize = 0xC0;
        unsafe {
            // Call 1 (clear): the LEA+E8 pair.
            let mut clear_site: Option<*const u8> = None;
            for index in 0..SCAN_LIMIT {
                let at = anchor.add(index);
                if std::slice::from_raw_parts(at, VECTOR_LEA.len()) == VECTOR_LEA
                    && *at.add(VECTOR_LEA.len()) == 0xE8
                {
                    clear_site = Some(at.add(VECTOR_LEA.len()));
                    break;
                }
            }
            let Some(clear_site) = clear_site else {
                log_warn!(
                    "  [-] judge_rebuild_trio -- records-vector LEA+CALL pair not found (layout drift?)"
                );
                return;
            };
            // Calls 2 (reserve) and 3 (rebuild): the next two E8 sites,
            // skipping each call's own rel32 operand.
            let mut targets = vec![decode_call_rel32(clear_site)];
            let mut cursor = clear_site.add(5);
            let end = anchor.add(SCAN_LIMIT);
            while (cursor as usize) < end as usize && targets.len() < 3 {
                if *cursor == 0xE8 {
                    targets.push(decode_call_rel32(cursor));
                    cursor = cursor.add(5);
                } else {
                    cursor = cursor.add(1);
                }
            }
            if targets.len() < 3 {
                log_warn!("  [-] judge_rebuild_trio -- fewer than three calls after the anchor");
                return;
            }
            for (index, target) in targets.iter().enumerate() {
                let off = target.offset_from(self.base);
                if off < 0 || off as usize >= self.size {
                    log_warn!(
                        "  [-] judge_rebuild_trio -- call {} target {:p} lies outside module",
                        index,
                        target
                    );
                    return;
                }
            }
            if targets[0] == targets[1] || targets[1] == targets[2] || targets[0] == targets[2] {
                log_warn!("  [-] judge_rebuild_trio -- call targets are not distinct");
                return;
            }
            for (name, target) in [
                ("judge_rebuild_clear", targets[0]),
                ("judge_rebuild_reserve", targets[1]),
                ("judge_rebuild_rebuild", targets[2]),
            ] {
                self.resolved.insert(name.into(), target);
                log_info!(
                    "  [+] {} (derived) @ +0x{:X}",
                    name,
                    target.offset_from(self.base) as usize
                );
            }
        }
    }

    /// Resolve `row_builder_fn` — the 21-row OptionForm builder.
    /// Matched directly via `row_builder_fn_prologue` (unique 5-register
    /// save + ~0x1B00 __chkstk frame).
    fn derive_row_builder_fn(&mut self) {
        let entry = match self.get_address("row_builder_fn_prologue") {
            Some(e) => e,
            None => {
                log_warn!("  [-] row_builder_fn -- prologue not resolved");
                return;
            }
        };
        self.resolved.insert("row_builder_fn".into(), entry);
        let offset = unsafe { entry.offset_from(self.base) as usize };
        log_info!(
            "  [+] row_builder_fn (direct prologue match) @ +0x{:X}",
            offset
        );
    }

    /// Find `OptionTab::vftable` via RTTI. Same mechanism as `sprite_vtable`,
    /// `check_step_data_vtable`, `auto_foot_panel_vtable`.
    fn find_option_tab_vtable(&mut self) {
        let vtable = match self
            .find_vtable_by_rtti(".?AVOptionTab@selectmusic@sequence@@", "option_tab_vtable")
        {
            Some(v) => v,
            None => return,
        };
        self.resolved.insert("option_tab_vtable".into(), vtable);
        let offset = unsafe { vtable.offset_from(self.base) as usize };
        log_info!("  [+] option_tab_vtable (RTTI) @ +0x{:X}", offset);
    }

    /// Derive a per-KIND `OptionElement<T>::ctor` + its primary vtable from
    /// the RTTI string naming the specialization.
    ///
    /// The ctor is found by RTTI-walking to one of the specialization's four
    /// vtables, scanning `.text` for `LEA reg, [RIP+disp32]` instructions
    /// whose disp32 resolves to that vtable, then walking each hit back to
    /// the MSVC function prologue (`48 89 4C 24 08` = `MOV [RSP+0x8], RCX`).
    /// Two LEAs reference the vtable — one in the ctor, one in the
    /// destructor — disambiguated by counting `E8` (CALL rel32) bytes
    /// between the prologue and the LEA: the ctor has ≥1 (the parent-class
    /// ctor call); the destructor has 0.
    ///
    /// The primary vtable is then derived from the ctor's canonical 7-byte
    /// LEA at ctor+0x49:
    ///
    /// ```text
    /// ctor+0x49: 48 8D 05 <disp32>   LEA RAX, [RIP + disp]   ; primary vtable
    /// ctor+0x50: 48 89 06            MOV [RSI], RAX          ; install at row+0x00
    /// ```
    ///
    /// The instruction layout is structurally invariant across toolchain
    /// drift for every `OptionElement<T>` specialization the game ships
    /// (verified cross-version on both the ArrowColor and int
    /// specializations); only the disp32 value moves between builds as
    /// `.rdata` shifts.
    fn derive_option_element_ctor(
        &mut self,
        rtti_name: &str,
        ctor_sig_name: &str,
        vtable_sig_name: &str,
    ) {
        let vtable_ref = match self.find_vtable_by_rtti(rtti_name, ctor_sig_name) {
            Some(v) => v,
            None => return,
        };

        let mut lea_hits: Vec<*const u8> = Vec::new();
        unsafe {
            let text = std::slice::from_raw_parts(self.base, self.size);
            for i in 0..text.len().saturating_sub(7) {
                let rex = text[i];
                if rex != 0x48 && rex != 0x4C {
                    continue;
                }
                if text[i + 1] != 0x8D {
                    continue;
                }
                let modrm = text[i + 2];
                if (modrm & 0xC7) != 0x05 {
                    continue;
                }
                let disp = i32::from_le_bytes([text[i + 3], text[i + 4], text[i + 5], text[i + 6]]);
                let instr_addr = self.base.add(i);
                let tgt = instr_addr.add(7).offset(disp as isize);
                if tgt == vtable_ref {
                    lea_hits.push(instr_addr);
                }
            }
        }

        if lea_hits.is_empty() {
            log_warn!("  [-] {ctor_sig_name} -- no LEA to vtable found in .text");
            return;
        }

        let mut ctor_entry: Option<*const u8> = None;
        unsafe {
            for lea in &lea_hits {
                let max_back = 0x400usize;
                let mut prologue: Option<*const u8> = None;
                for back in 5..max_back {
                    let p = lea.sub(back);
                    if p < self.base {
                        break;
                    }
                    if *p == 0x48
                        && *p.add(1) == 0x89
                        && *p.add(2) == 0x4C
                        && *p.add(3) == 0x24
                        && *p.add(4) == 0x08
                    {
                        prologue = Some(p);
                        break;
                    }
                }

                let prologue = match prologue {
                    Some(p) => p,
                    None => continue,
                };

                // Byte-level E8 count in [prologue, lea). Sufficient here
                // because the prologue/setup region is structurally
                // well-formed and E8 bytes don't occur as operands in the
                // specific instruction shapes that populate this region.
                let span = (*lea as usize) - (prologue as usize);
                let region = std::slice::from_raw_parts(prologue, span);
                let e8_count = region.iter().filter(|&&b| b == 0xE8).count();

                if e8_count >= 1 {
                    ctor_entry = Some(prologue);
                    break;
                }
            }
        }

        let ctor = match ctor_entry {
            Some(a) => a,
            None => {
                log_warn!(
                    "  [-] {ctor_sig_name} -- no CALL-bearing prologue found (ctor heuristic failed)"
                );
                return;
            }
        };

        self.resolved.insert(ctor_sig_name.into(), ctor);
        let offset = unsafe { ctor.offset_from(self.base) as usize };
        log_info!(
            "  [+] {ctor_sig_name} (RTTI + ctor/dtor disambig) @ +0x{:X}",
            offset
        );

        // Now derive the primary vtable from the canonical LEA at ctor+0x49.
        unsafe {
            let lea = ctor.add(0x49);
            if *lea != 0x48 || *lea.add(1) != 0x8D || *lea.add(2) != 0x05 {
                log_warn!(
                    "  [-] {vtable_sig_name} -- expected LEA RAX,[RIP+disp32] at ctor+0x49, got {:02X} {:02X} {:02X}",
                    *lea,
                    *lea.add(1),
                    *lea.add(2)
                );
                return;
            }
            let vtable = decode_rip_relative(lea.add(3));
            let voffset = vtable.offset_from(self.base) as usize;
            if voffset >= self.size {
                log_warn!(
                    "  [-] {vtable_sig_name} -- derived address {:p} lies outside module",
                    vtable
                );
                return;
            }
            self.resolved.insert(vtable_sig_name.into(), vtable);
            log_info!(
                "  [+] {vtable_sig_name} (derived from ctor+0x49 LEA) @ +0x{:X}",
                voffset
            );
        }
    }

    /// Derive `string_assign` (MSVC `std::basic_string::assign(const char*,
    /// size_t)`) from xrefs to `metadata_insert`.
    ///
    /// Pair-locality derivation: the row builder's per-tag caller sequence is
    ///
    /// ```text
    /// CALL string_assign    ; RCX=&stack_str, RDX=literal, R8=len
    /// NOP                   ; (optional)
    /// LEA  RDX, [RBP+buf]
    /// MOV  RCX, <row_ptr>
    /// CALL metadata_insert  ; 0x10 bytes after the string_assign CALL
    /// ```
    ///
    /// Every xref to `metadata_insert` has a `CALL string_assign` at
    /// exactly 0x10 bytes prior; decoding that CALL's disp32 yields the
    /// string_assign entry point. Pair-locality is preferred over a direct
    /// AOB scan on string_assign because three overloads of MSVC's
    /// `basic_string::assign` share identical prologue bytes.
    fn derive_string_assign_via_pair(&mut self) {
        let metadata_insert = match self.get_address("metadata_insert") {
            Some(a) => a,
            None => {
                log_warn!("  [-] string_assign -- metadata_insert not resolved");
                return;
            }
        };

        let call_sites = self.xrefs_to(metadata_insert);
        if call_sites.is_empty() {
            log_warn!("  [-] string_assign -- no xrefs to metadata_insert found");
            return;
        }

        // Expected layout at each xref:
        //   site - 0x10  E8 <disp32>         CALL string_assign  (5 bytes)
        //   site - 0x0B  90                  NOP                 (1 byte)
        //   site - 0x0A  48 8D 55 ??          LEA RDX,[RBP+??]   (4 bytes)
        //   site - 0x06  ...                 setup of RCX for metadata_insert
        //   site         E8 <disp32>         CALL metadata_insert
        //
        // Try each xref until one matches the CALL shape exactly; take the
        // first successful decode as the definitive target.
        unsafe {
            for &site in &call_sites {
                let call_site = site.sub(0x10);
                if call_site < self.base {
                    continue;
                }
                if *call_site != 0xE8 {
                    continue;
                }
                let target = decode_call_rel32(call_site);
                let tgt_offset = target.offset_from(self.base) as usize;
                if tgt_offset >= self.size {
                    continue;
                }
                self.resolved.insert("string_assign".into(), target);
                log_info!(
                    "  [+] string_assign (derived from metadata_insert xref pair-locality) @ +0x{:X}",
                    tgt_offset
                );
                return;
            }
        }

        log_warn!("  [-] string_assign -- no metadata_insert xref had a CALL at -0x10 offset");
    }

    /// Derive three MSVC `_Impl_no_alloc0` vtable slots the
    /// `event_register` mechanism expects from mod-authored lambdas.
    ///
    /// These addresses are not AOB-scannable individually (the bodies are
    /// compiler-emitted one-liners that match thousands of sites in the
    /// binary), so the derivation rides on an already-verified RIP chain
    /// whose every link is structurally invariant:
    ///
    /// ```text
    /// option_element_arrowcolor_primary_vtable (LEA-derived from ctor+0x49)
    ///   └── [4]  → FUN_180173c10 (native advanceValue)
    ///        └── +0x2B = E8 <disp32> CALL → FUN_18017dc40 (register left/right)
    ///             └── +0x0D = 48 8D 05 <disp32> LEA → lambda232_vtable
    ///                  ├── [3]  → lambda_destruct_slot3
    ///                  ├── [4]  → lambda_release_slot4
    ///                  └── [5]  → lambda_get_captured_slot5
    /// ```
    ///
    /// Slot 0 (copy constructor) is intentionally NOT derived here. The
    /// native slot-0 body hardcodes the lambda232 vtable as the
    /// destination's initial vtable — fine for native lambdas, fatal for
    /// mod-authored lambdas, because the heap-copied registration would
    /// end up invoking lambda232's native value-list walker instead of
    /// our direction-specific trampoline. Mod code provides its own copy
    /// trampoline that preserves whatever vtable the source lambda holds.
    fn derive_event_lambda_vtable_slots(&mut self) {
        let primary_vtable = match self.get_address("option_element_arrowcolor_primary_vtable") {
            Some(a) => a,
            None => {
                log_warn!("  [-] event_lambda_vtable_slots -- primary vtable not resolved");
                return;
            }
        };

        unsafe {
            // Slot 4 of the primary vtable = FUN_180173c10 (native advanceValue).
            let advance_value = *(primary_vtable.add(4 * 8) as *const *const u8);
            let ofs = advance_value.offset_from(self.base) as usize;
            if ofs >= self.size {
                log_warn!(
                    "  [-] event_lambda_vtable_slots -- primary vtable slot 4 {:p} outside module",
                    advance_value
                );
                return;
            }

            // +0x2B inside FUN_180173c10: E8 <disp32> CALL FUN_18017dc40.
            let call_site = advance_value.add(0x2B);
            if *call_site != 0xE8 {
                log_warn!(
                    "  [-] event_lambda_vtable_slots -- expected E8 CALL at advanceValue+0x2B, got {:02X}",
                    *call_site
                );
                return;
            }
            let register_lambdas = decode_call_rel32(call_site);

            // +0x0D inside FUN_18017dc40: 48 8D 05 <disp32> LEA RAX, [lambda232_vtable].
            let lea = register_lambdas.add(0x0D);
            if *lea != 0x48 || *lea.add(1) != 0x8D || *lea.add(2) != 0x05 {
                log_warn!(
                    "  [-] event_lambda_vtable_slots -- expected 48 8D 05 LEA at register_lambdas+0x0D, got {:02X} {:02X} {:02X}",
                    *lea, *lea.add(1), *lea.add(2)
                );
                return;
            }
            let lambda_vtable = decode_rip_relative(lea.add(3));
            let lv_ofs = lambda_vtable.offset_from(self.base) as usize;
            if lv_ofs >= self.size {
                log_warn!(
                    "  [-] event_lambda_vtable_slots -- lambda vtable {:p} outside module",
                    lambda_vtable
                );
                return;
            }

            let slots = [
                ("lambda_destruct_slot3", 3usize),
                ("lambda_release_slot4", 4usize),
                ("lambda_get_captured_slot5", 5usize),
            ];
            for &(name, idx) in &slots {
                let addr = *(lambda_vtable.add(idx * 8) as *const *const u8);
                let off = addr.offset_from(self.base) as usize;
                if off >= self.size {
                    log_warn!("  [-] {} -- slot {} {:p} outside module", name, idx, addr);
                    continue;
                }
                self.resolved.insert(name.into(), addr);
                log_info!(
                    "  [+] {} (derived via lambda232_vtable[{}]) @ +0x{:X}",
                    name,
                    idx,
                    off
                );
            }
        }
    }

    /// Resolve `textlayer_bind` — binds a TextLayer to a parent MC path.
    ///
    /// Preferred: direct prologue match (`textlayer_bind_direct`, 20260526+).
    /// Legacy fallback: anchor at fn+0x33 (`textlayer_bind_anchor`).
    fn derive_textlayer_bind(&mut self) {
        if let Some(entry) = self.get_address("textlayer_bind_direct") {
            self.resolved.insert("textlayer_bind".into(), entry);
            let offset = unsafe { entry.offset_from(self.base) as usize };
            log_info!(
                "  [+] textlayer_bind (direct prologue match) @ +0x{:X}",
                offset
            );
            return;
        }

        let anchor = match self.get_address("textlayer_bind_anchor") {
            Some(a) => a,
            None => {
                log_warn!("  [-] textlayer_bind -- anchor not resolved");
                return;
            }
        };

        unsafe {
            let fn_entry = anchor.sub(0x33);
            if fn_entry < self.base {
                log_warn!("  [-] textlayer_bind -- derived fn entry lies before module base");
                return;
            }
            self.resolved.insert("textlayer_bind".into(), fn_entry);
            let offset = fn_entry.offset_from(self.base) as usize;
            log_info!(
                "  [+] textlayer_bind (derived from anchor - 0x33) @ +0x{:X}",
                offset
            );
        }
    }

    /// Derive `customize_offset` — the byte offset of `ddr::player::Customize`
    /// within `PlayerWork`. Detected via RTTI walk: find the Customize vtable,
    /// scan for the LEA that loads its address, then read the displacement
    /// from the preceding `LEA RCX, [RDI + disp32]` which addresses the
    /// Customize sub-object within PlayerWork.
    ///
    /// The result is stored as a pointer whose numeric value IS the offset
    /// (e.g., `0x1790 as *const u8`). Consumers cast it to `usize`.
    fn derive_customize_offset(&mut self) {
        let vtable =
            match self.find_vtable_by_rtti(".?AVCustomize@player@ddr@@", "customize_vtable") {
                Some(v) => v,
                None => return,
            };

        let vt_off = unsafe { vtable.offset_from(self.base) as usize };
        log_info!("  [+] customize_vtable (RTTI) @ +0x{:X}", vt_off);

        unsafe {
            let text = std::slice::from_raw_parts(self.base, self.size);

            // Scan for LEA reg, [RIP+disp32] instructions that resolve to the vtable.
            // Encoding: 48/4C 8D (ModRM & 0xC7 == 0x05) <disp32>
            for i in 0..text.len().saturating_sub(7) {
                let rex = text[i];
                if rex != 0x48 && rex != 0x4C {
                    continue;
                }
                if text[i + 1] != 0x8D {
                    continue;
                }
                let modrm = text[i + 2];
                if (modrm & 0xC7) != 0x05 {
                    continue;
                }
                let disp = i32::from_le_bytes([text[i + 3], text[i + 4], text[i + 5], text[i + 6]]);
                let instr_end = self.base.add(i + 7);
                let tgt = instr_end.offset(disp as isize);
                if tgt != vtable {
                    continue;
                }

                // Found LEA that loads the Customize vtable. Two patterns
                // are known for how the compiler stores it into PlayerWork:
                //
                // Pattern A (older builds): LEA RCX,[reg+disp32] immediately
                //   BEFORE the vtable LEA, then MOV [RCX],RAX after.
                //   Encoding: 48 8D 8F/8B/8E/89 <disp32> (7 bytes before)
                //
                // Pattern B (20260526+): MOV [reg+disp32],RAX immediately
                //   AFTER the vtable LEA — compiler folded the LEA+MOV into
                //   a single store.
                //   Encoding: 48 89 87/83/86/85 <disp32> (7 bytes after)

                // Try Pattern A: LEA RCX,[reg+disp32] before vtable LEA
                if i >= 7 {
                    let prev = i - 7;
                    if text[prev] == 0x48 && text[prev + 1] == 0x8D {
                        let prev_modrm = text[prev + 2];
                        // mod=10 (disp32), reg=001 (RCX), rm=any base reg
                        if (prev_modrm & 0xC0) == 0x80 && (prev_modrm & 0x38) == 0x08 {
                            let offset = u32::from_le_bytes([
                                text[prev + 3],
                                text[prev + 4],
                                text[prev + 5],
                                text[prev + 6],
                            ]) as usize;
                            if offset >= 0x1000 && offset <= 0x4000 {
                                self.resolved
                                    .insert("customize_offset".into(), offset as *const u8);
                                log_info!(
                                    "  [+] customize_offset (derived from ctor LEA) = 0x{:X}",
                                    offset
                                );
                                return;
                            }
                        }
                    }
                }

                // Try Pattern B: MOV [reg+disp32],RAX after vtable LEA
                let next = i + 7; // first byte after the LEA instruction
                if next + 7 <= text.len() {
                    if text[next] == 0x48 && text[next + 1] == 0x89 {
                        let next_modrm = text[next + 2];
                        // mod=10 (disp32), reg=000 (RAX), rm=any base reg
                        if (next_modrm & 0xC0) == 0x80 && (next_modrm & 0x38) == 0x00 {
                            let offset = u32::from_le_bytes([
                                text[next + 3],
                                text[next + 4],
                                text[next + 5],
                                text[next + 6],
                            ]) as usize;
                            if offset >= 0x1000 && offset <= 0x4000 {
                                self.resolved
                                    .insert("customize_offset".into(), offset as *const u8);
                                log_info!(
                                    "  [+] customize_offset (derived from vtable store) = 0x{:X}",
                                    offset
                                );
                                return;
                            }
                        }
                    }
                }
            }
        }

        log_warn!("  [-] customize_offset -- could not derive from vtable LEA pattern");
    }

    // ── Generic helpers ─────────────────────────────────────────────

    /// Find a function by scanning for a debug string reference, then walking
    /// backwards to the function prologue (48 8B C4 = mov rax, rsp).
    fn find_function_by_debug_string(&self, needle: &str, label: &str) -> Option<*const u8> {
        // Build AOB pattern for the string bytes + null terminator
        let name_pattern: String = needle
            .bytes()
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join(" ")
            + " 00";

        let str_matches = scan_pattern_all(self.base, self.size, &name_pattern);
        if str_matches.is_empty() {
            log_warn!("  [-] {} -- debug string not found", label);
            return None;
        }
        let str_addr = str_matches[0].address;

        // LEA ModRM bytes for RIP-relative: mod=00, rm=101, reg=any
        let lea_modrm: &[u8] = &[0x05, 0x0D, 0x15, 0x1D, 0x25, 0x2D, 0x35, 0x3D];

        let bytes = unsafe { std::slice::from_raw_parts(self.base, self.size) };

        for i in 0..self.size.saturating_sub(7) {
            let b0 = bytes[i];
            if b0 != 0x48 && b0 != 0x4C {
                continue;
            }
            if bytes[i + 1] != 0x8D {
                continue;
            }
            if !lea_modrm.contains(&bytes[i + 2]) {
                continue;
            }

            let disp = i32::from_le_bytes([bytes[i + 3], bytes[i + 4], bytes[i + 5], bytes[i + 6]]);
            let instr_addr = unsafe { self.base.add(i) };
            let resolved = unsafe { instr_addr.add(7).offset(disp as isize) };
            if resolved != str_addr {
                continue;
            }

            // Found LEA referencing our string. Walk backwards to find prologue.
            for back in 0..0x2000usize {
                if i < back {
                    break;
                }
                let ci = i - back;
                if bytes[ci] == 0x48
                    && ci + 2 < self.size
                    && bytes[ci + 1] == 0x8B
                    && bytes[ci + 2] == 0xC4
                {
                    // Verify next byte is a PUSH (0x50-0x57 or REX 0x41 + 0x50-0x57)
                    if ci + 3 < self.size {
                        let next = bytes[ci + 3];
                        if (0x50..=0x57).contains(&next) {
                            return Some(unsafe { self.base.add(ci) });
                        }
                        if next == 0x41
                            && ci + 4 < self.size
                            && (0x50..=0x57).contains(&bytes[ci + 4])
                        {
                            return Some(unsafe { self.base.add(ci) });
                        }
                    }
                }
            }

            log_warn!(
                "  [-] {} -- found string ref but could not locate function prologue",
                label
            );
            return None;
        }

        log_warn!("  [-] {} -- no LEA referencing debug string found", label);
        None
    }

    /// Generic RTTI vtable finder for MSVC C++ classes.
    pub fn find_vtable_by_rtti(&self, rtti_name: &str, label: &str) -> Option<*const u8> {
        let name_pattern: String = rtti_name
            .bytes()
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join(" ")
            + " 00";

        let name_matches = scan_pattern_all(self.base, self.size, &name_pattern);
        if name_matches.is_empty() {
            log_warn!("  [-] {} -- RTTI string \"{}\" not found", label, rtti_name);
            return None;
        }
        let name_addr = name_matches[0].address;

        unsafe {
            // TypeDescriptor = name_addr - 0x10
            let type_desc = name_addr.sub(0x10);
            let td_rva = type_desc.offset_from(self.base) as u32;
            let td_rva_pattern = format!(
                "{:02X} {:02X} {:02X} {:02X}",
                td_rva & 0xFF,
                (td_rva >> 8) & 0xFF,
                (td_rva >> 16) & 0xFF,
                (td_rva >> 24) & 0xFF
            );

            let rva_matches = scan_pattern_all(self.base, self.size, &td_rva_pattern);
            let mut col_addr: Option<*const u8> = None;

            for m in &rva_matches {
                // COL candidate = match - 0x0C
                let candidate = m.address.sub(0x0C);
                if candidate < self.base {
                    continue;
                }
                // x64 RTTI signature must be 1
                if (candidate as *const u32).read_unaligned() != 1 {
                    continue;
                }
                // pSelf must point back to COL
                let p_self = (candidate.add(0x14) as *const u32).read_unaligned();
                if self.base.add(p_self as usize) == candidate {
                    col_addr = Some(candidate);
                    break;
                }
            }

            let col = match col_addr {
                Some(c) => c,
                None => {
                    log_warn!("  [-] {} -- CompleteObjectLocator not found", label);
                    return None;
                }
            };

            // Find pointer to COL (vtable[-1])
            let col_bytes = (col as u64).to_le_bytes();
            let col_ptr_pattern = col_bytes
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect::<Vec<_>>()
                .join(" ");

            let ptr_matches = scan_pattern_all(self.base, self.size, &col_ptr_pattern);
            if ptr_matches.is_empty() {
                log_warn!("  [-] {} -- vtable meta pointer not found", label);
                return None;
            }

            // vtable = meta_ptr + 8
            Some(ptr_matches[0].address.add(8))
        }
    }

    /// Find a RIP-relative MOV near a SHL reg, 0x6 instruction.
    fn find_rip_load_near_shl(&self, func_addr: *const u8, max_bytes: usize) -> Option<*const u8> {
        let rip_modrm: &[u8] = &[0x05, 0x0D, 0x15, 0x1D, 0x2D, 0x35, 0x3D];

        unsafe {
            for i in 0..max_bytes.saturating_sub(7) {
                let b0 = *func_addr.add(i);
                if (b0 & 0xFE) != 0x48 {
                    continue;
                }
                if *func_addr.add(i + 1) != 0xC1 {
                    continue;
                }
                let b2 = *func_addr.add(i + 2);
                if (b2 & 0xF8) != 0xE0 {
                    continue;
                }
                if *func_addr.add(i + 3) != 0x06 {
                    continue;
                }

                // Found SHL reg, 6. Scan forward for RIP-relative MOV.
                let search_end = std::cmp::min(i + 16, max_bytes.saturating_sub(6));
                for j in (i + 4)..search_end {
                    let m0 = *func_addr.add(j);
                    if (m0 & 0xFE) != 0x48 {
                        continue;
                    }
                    if *func_addr.add(j + 1) != 0x8B {
                        continue;
                    }
                    let m2 = *func_addr.add(j + 2);
                    if !rip_modrm.contains(&m2) {
                        continue;
                    }

                    return Some(decode_rip_relative(func_addr.add(j + 3)));
                }
            }
        }
        None
    }

    /// Find FF 15 [rip+disp] indirect call targets within `max_bytes` of `addr`,
    /// filtered to addresses within the given module.
    fn find_ff15_targets(
        &self,
        addr: *const u8,
        max_bytes: usize,
        mod_base: *const u8,
        mod_size: usize,
    ) -> Vec<*const u8> {
        let mut targets = Vec::new();
        let mod_end = mod_base as usize + mod_size;
        unsafe {
            for i in 0..max_bytes.saturating_sub(6) {
                if *addr.add(i) == 0xFF && *addr.add(i + 1) == 0x15 {
                    let target = decode_rip_relative(addr.add(i + 2));
                    let t = target as usize;
                    if t >= mod_base as usize && t + 8 <= mod_end {
                        targets.push(target);
                    }
                }
            }
        }
        targets
    }

    /// Find all CALL rel32 (E8) targets within `max_bytes` of `addr`.
    fn find_all_calls(&self, addr: *const u8, max_bytes: usize) -> Vec<*const u8> {
        let mut targets = Vec::new();
        unsafe {
            for i in 0..max_bytes {
                if *addr.add(i) == 0xE8 {
                    targets.push(decode_call_rel32(addr.add(i)));
                }
            }
        }
        targets
    }

    /// Find all indirect call targets (FF 15 [disp32] or MOV+CALL reg) within `max_bytes`.
    fn find_all_indirect_call_targets(&self, addr: *const u8, max_bytes: usize) -> Vec<*const u8> {
        let mut targets = Vec::new();
        unsafe {
            for i in 0..max_bytes.saturating_sub(6) {
                let b0 = *addr.add(i);
                let b1 = *addr.add(i + 1);

                // FF 15 [disp32] — CALL [rip+disp]
                if b0 == 0xFF && b1 == 0x15 {
                    targets.push(decode_rip_relative(addr.add(i + 2)));
                    continue;
                }

                // 48 8B 05/0D/15/35/3D [disp32] ... FF D0-D7 — MOV reg,[rip+disp]; CALL reg
                if b0 == 0x48 && b1 == 0x8B {
                    let b2 = *addr.add(i + 2);
                    if matches!(b2, 0x05 | 0x0D | 0x15 | 0x35 | 0x3D) {
                        let data_addr = decode_rip_relative(addr.add(i + 3));
                        // Look for CALL reg (FF D0-D7) within next 32 bytes
                        for j in (i + 7)..std::cmp::min(i + 39, max_bytes.saturating_sub(1)) {
                            if *addr.add(j) == 0xFF
                                && (*addr.add(j + 1) >= 0xD0 && *addr.add(j + 1) <= 0xD7)
                            {
                                targets.push(data_addr);
                                break;
                            }
                        }
                    }
                }
            }
        }
        targets
    }
}

/// Resolve a named export from the already-loaded `libafp-win64.dll`, or
/// `None` if the module or export is missing. Used by the CMovieClip
/// color-twin resolver to compare each twin body's IAT target against the
/// canonical `afp_layer_set_color` / `afp_layer_set_acolor` addresses.
/// libafp is a static import of gamemdx, so it is guaranteed loaded (and its
/// IAT slots patched) by the time `resolve_derived` runs.
fn resolve_libafp_export(name: &str) -> Option<*const u8> {
    use std::ffi::CString;
    use windows::core::PCSTR;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    let module = CString::new("libafp-win64.dll").ok()?;
    let export = CString::new(name).ok()?;
    unsafe {
        let handle = match GetModuleHandleA(PCSTR(module.as_ptr() as *const u8)) {
            Ok(h) if !h.is_invalid() => h,
            _ => return None,
        };
        GetProcAddress(handle, PCSTR(export.as_ptr() as *const u8)).map(|f| f as *const u8)
    }
}
