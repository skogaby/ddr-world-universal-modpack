#!/usr/bin/env python3
"""arkmdxbio2 export-wrapper vtable-slot derivation, cross-build.

Mirrors `input_manager::derive_vtable_slot_from_export` +
`derive_ark_vtable_slots` (src/services/input_manager.rs) byte-for-byte in
Python and runs them over every `arkmdxbio2*.dll` in a directory. The Rust
side has no host harness (input_manager pulls in the Windows detour
machinery), so this port is the offline check — KEEP THE TWO IN STEP.

The 2026-09-03 regression this guards against: the Rust helper accepted
only a `41` REX byte, but `arkMDXGet10Key` tail-jumps with `49 FF A2 d32`
(REX.WB), so the 10-key slot never derived on ANY ark and every injection
detour (pinpad 0-0-0, menu bytes, card-in, stage panels) was silently
withheld cabinet-wide.

Usage: ark_check.py --dir ~/Desktop/ddr_modules
Exit 0 when every ark derives a plausible, mutually consistent slot set.
"""
from __future__ import annotations

import argparse
import sys
from pathlib import Path

import pefile

EXPORTS = [
    "arkMDXGet10Key",
    "arkMDXGetPanelUp",
    "arkMDXGetPanelDown",
    "arkMDXGetPanelLeft",
    "arkMDXGetPanelRight",
]
# The MdxHWIO field map input_manager's dispatcher detour was RE'd against.
VERIFIED = [0x308, 0x310, 0x318, 0x320, 0x328]
# Every ark export name the DLL resolves (grep'd from src/); each must exist.
USED_EXPORTS = [
    "arkMDXGetStart", "arkMDXGetUp", "arkMDXGetDown", "arkMDXGetLeft",
    "arkMDXGetRight", "arkMDXGet10Key", "arkMDXGetPanelUp", "arkMDXGetPanelDown",
    "arkMDXGetPanelLeft", "arkMDXGetPanelRight",
]


def derive_slot(body: bytes) -> int | None:
    """Port of `derive_vtable_slot_from_export`."""
    for i in range(len(body) - 7):
        if (body[i] & 0xF1) != 0x41 or body[i + 1] != 0xFF:
            continue
        modrm = body[i + 2]
        if (modrm & 0xC0) == 0x80 and (modrm & 0x38) in (0x10, 0x20) and (modrm & 7) != 4:
            return int.from_bytes(body[i + 3 : i + 7], "little")
    return None


def derive_slots(image: bytes, exports: dict[str, int]) -> list[int] | None:
    """Port of `derive_ark_vtable_slots` (returns [tenkey, up, down, left, right])."""
    slots = []
    for name in EXPORTS:
        rva = exports.get(name)
        if rva is None:
            return None
        s = derive_slot(image[rva : rva + 0x80])
        if s is None:
            return None
        slots.append(s)
    plausible = lambda s: 0x40 <= s <= 0x1000 and s % 8 == 0
    if not all(plausible(s) for s in slots):
        return None
    panels = slots[1:]
    if any(panels[i + 1] != panels[i] + 8 for i in range(3)) or slots[0] in panels:
        return None
    return slots


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", required=True, type=Path)
    args = ap.parse_args()
    d = Path(str(args.dir).replace("~", str(Path.home())))
    arks = sorted(list(d.glob("arkmdxbio2*.dll")) + list(d.glob("*/arkmdxbio2*.dll")))
    if not arks:
        print("  (no arkmdxbio2*.dll found — ark leg skipped)")
        return 0
    print("-" * 78)
    print("ARKMDXBIO2 EXPORT-WRAPPER VTABLE SLOTS (input_manager derivation, Python mirror)")
    print("-" * 78)
    ok = True
    for p in arks:
        pe = pefile.PE(str(p))
        image = pe.get_memory_mapped_image()
        exports = {e.name.decode(): e.address for e in pe.DIRECTORY_ENTRY_EXPORT.symbols if e.name}
        missing = [n for n in USED_EXPORTS if n not in exports]
        slots = derive_slots(image, exports)
        name = p.stem.replace("arkmdxbio2", "").lstrip("_") or p.parent.name
        if missing or slots is None:
            ok = False
            print(f"  {name:<10} FAIL  missing exports={missing}  slots={slots}")
            continue
        verified = slots == VERIFIED
        print(f"  {name:<10} OK    10-key +{slots[0]:#x}  panels +{slots[1]:#x}..+{slots[4]:#x}  "
              f"field-map verified={verified}" + ("" if verified else "  (panels+pinpad only, no menu/card injection)"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
