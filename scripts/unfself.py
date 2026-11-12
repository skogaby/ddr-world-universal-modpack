#!/usr/bin/env python3
"""Unwrap a fake-signed PS4 SELF (eboot.bin / .prx / .sprx) into a plain ELF.

A SELF is `header | entries[n] | ELF header + program headers | ... segment data`. Entry i
(`props:u64 offset:u64 filesz:u64 memsz:u64`) with props bit 11 set ("blocked") carries the
data of ELF program header `props >> 20 & 0xFFF`; bit 1 says the segment is encrypted, which
a fake-signed SELF never is. The output file is the ELF header + phdrs followed by each
segment at its original `p_offset`, which is what Ghidra's ELF loader (or GhidraOrbis) wants.
"""

import struct
import sys


def unfself(src, dst):
    d = open(src, 'rb').read()
    magic = struct.unpack('<I', d[:4])[0]
    if magic != 0x1D3D154F:
        if d[:4] == b'\x7fELF':
            open(dst, 'wb').write(d)
            return 'already an ELF'
        raise SystemExit(f'not a SELF (magic {magic:#x})')
    nent = struct.unpack('<H', d[0x18:0x1A])[0]
    ents = [struct.unpack('<QQQQ', d[0x20 + i * 0x20:0x40 + i * 0x20]) for i in range(nent)]
    elf_off = 0x20 + nent * 0x20
    e = d[elf_off:]
    assert e[:4] == b'\x7fELF'
    phoff, = struct.unpack('<Q', e[0x20:0x28])
    phentsize, phnum = struct.unpack('<HH', e[0x36:0x3A])
    phdrs = []
    for i in range(phnum):
        p = phoff + i * phentsize
        p_type, p_flags, p_offset, p_vaddr, p_paddr, p_filesz, p_memsz, p_align = struct.unpack('<IIQQQQQQ', e[p:p + 56])
        phdrs.append((p_type, p_offset, p_filesz, p_memsz))
    out = bytearray(e[:phoff + phnum * phentsize])
    for props, off, filesz, memsz in ents:
        if not (props >> 11) & 1:
            continue
        if (props >> 1) & 1:
            raise SystemExit('segment is encrypted; this is not a fake-signed SELF')
        if (props >> 3) & 1:
            raise SystemExit('compressed segments are not handled')
        idx = (props >> 20) & 0xFFF
        p_type, p_offset, p_filesz, p_memsz = phdrs[idx]
        if p_offset + filesz > len(out):
            out += b'\0' * (p_offset + filesz - len(out))
        out[p_offset:p_offset + filesz] = d[off:off + filesz]
        print(f'phdr {idx}: type {p_type:#x} -> file {p_offset:#x}..{p_offset + filesz:#x}')
    open(dst, 'wb').write(out)
    return f'{len(out)} bytes'


if __name__ == '__main__':
    if len(sys.argv) != 3:
        raise SystemExit('usage: unfself.py <eboot.bin> <out.elf>')
    print(unfself(sys.argv[1], sys.argv[2]))
