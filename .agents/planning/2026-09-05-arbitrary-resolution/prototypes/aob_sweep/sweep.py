#!/usr/bin/env python3
"""Raw-file AOB sweep of docs/arbitrary_resolution_research.md §9 patterns.

stdlib only. Scans each gamemdx build's raw bytes, maps file offsets to
image addresses (0x180000000 + RVA) via the PE section table, and reports
match counts + the bytes a patcher would read/write at fixed offsets.

Usage: python3 sweep.py [--out REPORT.md]
"""
import hashlib
import os
import re
import struct
import sys
import time

IMAGE_BASE = 0x180000000
MODULES_DIR = os.path.expanduser("~/Desktop/ddr_modules")
BUILDS = ["20250805", "20260224", "20260721", "20260825"]
INSTALL = os.environ.get(
    "DDR_WORLD_INSTALL",
    "/Users/holmej/Library/Application Support/CrossOver/Bottles/bemani/drive_c/ddr_world/contents",
)
LIVE = os.path.join(INSTALL, "modules", "gamemdx.dll")

# §9 rows, copied verbatim.
DOC_PATTERNS = [
    ("back-buffer dims select (FUN_1801ef6d0)",
     "80 79 12 00 48 8B F1 74 16 C7 05 ?? ?? ?? ?? 00 05 00 00 C7 05 ?? ?? ?? ?? D0 02 00 00 EB 14 C7 05 ?? ?? ?? ?? 80 02 00 00 C7 05 ?? ?? ?? ?? E0 01 00 00"),
    ("tag-0x07 rt-dims read",
     "0F B7 81 46 01 00 00 66 0F 6E C0 0F B7 81 44 01 00 00 66 0F 6E C8"),
    ("tag-0x0C raw gd write",
     "C7 00 18 00 0C 00 66 89 48 04 66 89 50 06 66 44 89 40 08 66 44 89 50 0A 48 83 C0 0C"),
    ("letterbox rect (FUN_1801f3f60)",
     "BA 00 05 00 00 8B F8 8B F0 44 8B C8 41 B8 01 00 00 00 44 3B D2 74 ?? 45 3B D8 75 ?? B8 A0 00 00 00 BA 60 04 00 00"),
    ("FUN_1801f01a0 hoisted 1280",
     "45 33 C9 45 8D 41 15 41 BF 00 05 00 00 41 8B D7 41 8B CF E8"),
    ("list viewport dims (FUN_1801f5d10)",
     "C7 85 ?? ?? 00 00 00 05 00 00 C7 85 ?? ?? 00 00 D0 02 00 00 48 8D 05 ?? ?? ?? ?? 48 89 85 ?? ?? 00 00 C7 85 ?? ?? 00 00 00 05 00 00 C7 85 ?? ?? 00 00 D0 02 00 00"),
    ("layer set-size loop (FUN_18002b080)",
     "48 83 FB 01 74 ?? 48 83 FB 03 74 ?? 48 83 FB 04 76 ?? 48 83 FB 07 76 ?? 48 83 FB 0A 75"),
]

# Rows documented as legitimately multi-hit.
EXPECTED_COUNT = {"list viewport dims (FUN_1801f5d10)": 2}

CANDIDATE_PATTERNS = [
    ("cmp eax,1 ; seta  (83 F8 01 0F 97)", "83 F8 01 0F 97"),
    ("cmp eax,1 ; setbe (83 F8 01 0F 96)", "83 F8 01 0F 96"),
    ("cmp eax,1 ; ja    (83 F8 01 77)", "83 F8 01 77"),
    # Observed shape in onBoot (from the 7b dump): mov edx,[rsp+28]; test edx,edx; js;
    # cmp edx,1; jg; xor al,al; jmp; mov al,1; mov [rsp+62],al
    ("OBSERVED HD-flag: test edx,edx; js ??; cmp edx,1; jg 4; xor al,al; jmp 2; mov al,1; mov [rsp+??],al",
     "85 D2 78 ?? 83 FA 01 7F 04 32 C0 EB 02 B0 01 88 44 24 ??"),
    ("OBSERVED HD-flag core (cmp edx,1; jg 4; xor al,al; jmp 2; mov al,1; mov [rsp+??],al)",
     "83 FA 01 7F 04 32 C0 EB 02 B0 01 88 44 24 ??"),
]

FPS_PATTERN = "C7 44 24 ?? 3C 00 00 00 75 08 C7 44 24 ?? 4B 00 00 00"
AA_PATTERN = "C7 44 24 ?? 03 00 00 00"


# ----------------------------------------------------------------------------
# PE helpers
# ----------------------------------------------------------------------------
def parse_pe(data):
    """Return (timestamp, sections) where sections = [(name, va, vsize, rawptr, rawsize)]."""
    if data[:2] != b"MZ":
        raise ValueError("not MZ")
    e_lfanew = struct.unpack_from("<I", data, 0x3C)[0]
    if data[e_lfanew:e_lfanew + 4] != b"PE\0\0":
        raise ValueError("not PE")
    coff = e_lfanew + 4
    machine, nsec, tstamp, _, _, opt_size, _ = struct.unpack_from("<HHIIIHH", data, coff)
    opt = coff + 20
    magic = struct.unpack_from("<H", data, opt)[0]
    if magic == 0x20B:
        image_base = struct.unpack_from("<Q", data, opt + 24)[0]
    else:
        image_base = struct.unpack_from("<I", data, opt + 28)[0]
    sec_off = opt + opt_size
    sections = []
    for i in range(nsec):
        s = sec_off + i * 40
        name = data[s:s + 8].rstrip(b"\0").decode("ascii", "replace")
        vsize, va, rawsize, rawptr = struct.unpack_from("<IIII", data, s + 8)
        sections.append((name, va, vsize, rawptr, rawsize))
    return tstamp, image_base, sections


def file_off_to_va(off, sections, image_base):
    for name, va, vsize, rawptr, rawsize in sections:
        if rawptr <= off < rawptr + rawsize:
            return image_base + va + (off - rawptr), name
    # outside every section's raw data (headers/overlay): report file offset
    return off, "<no-section>"


# ----------------------------------------------------------------------------
# AOB matching
# ----------------------------------------------------------------------------
def compile_aob(text):
    toks = text.split()
    parts = []
    for t in toks:
        if t == "??" or t == "?":
            parts.append(b".")
        else:
            parts.append(re.escape(bytes([int(t, 16)])))
    return re.compile(b"".join(parts), re.DOTALL), len(toks), toks


def find_all(rx, data):
    # overlapping search
    out = []
    pos = 0
    while True:
        m = rx.search(data, pos)
        if not m:
            break
        out.append(m.start())
        pos = m.start() + 1
    return out


def hexb(b):
    return " ".join("%02X" % x for x in b)


def u32(data, off):
    return struct.unpack_from("<I", data, off)[0]


def hexdump(data, start, length, base_va):
    lines = []
    for i in range(0, length, 16):
        chunk = data[start + i:start + i + 16]
        lines.append("%016X  %s" % (base_va + i, hexb(chunk)))
    return "\n".join(lines)


def token_offset(toks, seq):
    """Locate a literal byte sequence within the pattern's tokens; return offset or None."""
    seq = seq.split()
    for i in range(len(toks) - len(seq) + 1):
        if [t.upper() for t in toks[i:i + len(seq)]] == [s.upper() for s in seq]:
            return i
    raise ValueError("sequence %r not in pattern" % (" ".join(seq),))


def find_seq_forward(data, start, limit, seq_text):
    rx, n, _ = compile_aob(seq_text)
    m = rx.search(data, start, start + limit + n)
    return (m.start() - start) if m else None


# ----------------------------------------------------------------------------
# main
# ----------------------------------------------------------------------------
def load(path):
    with open(path, "rb") as f:
        return f.read()


def main():
    out_path = "REPORT.md"
    if "--out" in sys.argv:
        out_path = sys.argv[sys.argv.index("--out") + 1]

    files = []
    for b in BUILDS:
        p = os.path.join(MODULES_DIR, "gamemdx_%s.dll" % b)
        files.append((b, p))
    live_present = os.path.isfile(LIVE)
    if live_present:
        files.append(("live", LIVE))

    R = []  # report lines
    def w(s=""):
        R.append(s)

    w("# AOB sweep — arbitrary_resolution_research.md §9")
    w()
    w("Generated by `sweep.py` (raw file scan, stdlib only). Addresses are image addresses")
    w("`0x180000000 + RVA`, RVA mapped through each file's PE section table.")
    w()

    # ---- load + identify -------------------------------------------------
    blobs = {}
    meta = {}
    w("## Builds")
    w()
    w("| label | path | size | MD5 | PE TimeDateStamp | .text VA/rawptr |")
    w("|---|---|---|---|---|---|")
    md5s = {}
    for label, path in files:
        data = load(path)
        blobs[label] = data
        ts, ib, secs = parse_pe(data)
        meta[label] = (ts, ib, secs)
        md5 = hashlib.md5(data).hexdigest()
        md5s[label] = md5
        text = [s for s in secs if s[0] == ".text"]
        tsec = "0x%X / 0x%X" % (text[0][1], text[0][3]) if text else "?"
        w("| %s | `%s` | %d | `%s` | %d (%s UTC) | %s |" % (
            label, path, len(data), md5, ts,
            time.strftime("%Y-%m-%d %H:%M:%S", time.gmtime(ts)), tsec))
    w()
    if live_present:
        same = [b for b in BUILDS if md5s[b] == md5s["live"]]
        if same:
            w("Live install binary MD5 matches build **%s** (byte-identical)." % same[0])
        else:
            w("Live install binary MD5 matches NONE of the four (PE stamp %d)." % meta["live"][0])
    else:
        w("Live install binary not present at `%s`." % LIVE)
    w()
    for label in blobs:
        ts, ib, secs = meta[label]
        if ib != IMAGE_BASE:
            w("NOTE: %s ImageBase = 0x%X (not 0x180000000); addresses below use the file's own base." % (label, ib))

    labels = [l for l, _ in files]

    # ---- §9 patterns -----------------------------------------------------
    w("## §9 patterns — match counts")
    w()
    w("| site | " + " | ".join(labels) + " |")
    w("|---|" + "---|" * len(labels))
    matches = {}  # (site,label) -> [file_off,...]
    flags = []
    for site, pat in DOC_PATTERNS:
        rx, n, toks = compile_aob(pat)
        cells = []
        for label in labels:
            data = blobs[label]
            ts, ib, secs = meta[label]
            hits = find_all(rx, data)
            matches[(site, label)] = hits
            vas = []
            for h in hits:
                va, sec = file_off_to_va(h, secs, ib)
                vas.append("`%X`" % va if sec != "<no-section>" else "`fileoff:%X`" % h)
            exp = EXPECTED_COUNT.get(site, 1)
            mark = "" if len(hits) == exp else " **!!**"
            cells.append("%d%s %s" % (len(hits), mark, ", ".join(vas)))
            if len(hits) != exp:
                flags.append("%s / %s: %d hit(s), expected %d" % (site, label, len(hits), exp))
        w("| %s | %s |" % (site, " | ".join(cells)))
    w()
    w("`!!` = count differs from the expected (1, or 2 for the list-viewport row, documented as")
    w("two overlapping hits of one table — the second hit starts at the row's second")
    w("`C7 85 .. 00 05 00 00` pair, 0x22 bytes in).")
    w()
    if flags:
        w("### Flags")
        w()
        for f in flags:
            w("- " + f)
        w()
    else:
        w("No flags: every pattern hit its expected count on every build.")
        w()

    # ---- per-site detail ---------------------------------------------------
    w("## Per-site byte checks")
    w()

    # (4) back-buffer dims select
    site, pat = DOC_PATTERNS[0]
    rx, n, toks = compile_aob(pat)
    o_500 = token_offset(toks, "00 05 00 00")
    o_2d0 = token_offset(toks, "D0 02 00 00")
    o_280 = token_offset(toks, "80 02 00 00")
    o_1e0 = token_offset(toks, "E0 01 00 00")
    w("### back-buffer dims select")
    w()
    w("imm32 offsets computed from the pattern tokens: 0x500 @ +%d, 0x2d0 @ +%d, 0x280 @ +%d, 0x1e0 @ +%d"
      % (o_500, o_2d0, o_280, o_1e0))
    w("(doc expectation: +15 / +25 / +37 / +47).")
    w()
    w("| build | match | +%d | +%d | +%d | +%d | disp32 targets (RIP-rel of the 4 `C7 05` stores) |" % (o_500, o_2d0, o_280, o_1e0))
    w("|---|---|---|---|---|---|---|")
    for label in labels:
        data = blobs[label]
        ts, ib, secs = meta[label]
        for h in matches[(site, label)]:
            va, _ = file_off_to_va(h, secs, ib)
            vals = []
            for o in (o_500, o_2d0, o_280, o_1e0):
                vals.append("`%s` (0x%X)" % (hexb(data[h + o:h + o + 4]), u32(data, h + o)))
            # each C7 05 disp32 imm32 store: opcode at token index of 'C7' before the imm
            tgts = []
            for o in (o_500, o_2d0, o_280, o_1e0):
                store = o - 6  # C7 05 disp32
                disp = struct.unpack_from("<i", data, h + store + 2)[0]
                next_ins = va + store + 10
                tgts.append("`%X`" % (next_ins + disp))
            w("| %s | `%X` | %s | %s |" % (label, va, " | ".join(vals), " / ".join(tgts)))
    w()

    # (5) letterbox rect
    site, pat = DOC_PATTERNS[3]
    rx, n, toks = compile_aob(pat)
    w("### letterbox rect")
    w()
    w("| build | match | imm32 @ +1 | `C7 83 98 02 00 00 D0 02 00 00` offset (≤ +0xC0) |")
    w("|---|---|---|---|")
    for label in labels:
        data = blobs[label]
        ts, ib, secs = meta[label]
        for h in matches[(site, label)]:
            va, _ = file_off_to_va(h, secs, ib)
            imm = data[h + 1:h + 5]
            off = find_seq_forward(data, h, 0xC0, "C7 83 98 02 00 00 D0 02 00 00")
            offs = ("+0x%X (`%X`)" % (off, va + off)) if off is not None else "NOT FOUND within +0xC0"
            w("| %s | `%X` | `%s` (0x%X) | %s |" % (label, va, hexb(imm), u32(data, h + 1), offs))
    w()

    # (6) hoisted 1280
    site, pat = DOC_PATTERNS[4]
    rx, n, toks = compile_aob(pat)
    o_r15 = token_offset(toks, "41 BF 00 05 00 00")
    w("### FUN_1801f01a0 hoisted 1280")
    w()
    w("`41 BF 00 05 00 00` (MOV R15D,0x500) sits at pattern offset +%d." % o_r15)
    w()
    w("| build | match | bytes @ +%d | `BE D0 02 00 00` (MOV ESI,0x2d0) forward ≤ 0x40 |" % o_r15)
    w("|---|---|---|---|")
    for label in labels:
        data = blobs[label]
        ts, ib, secs = meta[label]
        for h in matches[(site, label)]:
            va, _ = file_off_to_va(h, secs, ib)
            off = find_seq_forward(data, h, 0x40, "BE D0 02 00 00")
            offs = ("+0x%X (`%X`)" % (off, va + off)) if off is not None else "NOT FOUND within +0x40"
            w("| %s | `%X` | `%s` | %s |" % (label, va, hexb(data[h + o_r15:h + o_r15 + 6]), offs))
            # also dump 0x60 bytes from match for eyeballing
    w()
    w("Hex at match (0x60 bytes) for the hoisted-1280 site, per build:")
    w()
    for label in labels:
        data = blobs[label]
        ts, ib, secs = meta[label]
        for h in matches[(site, label)]:
            va, _ = file_off_to_va(h, secs, ib)
            w("```")
            w("[%s]" % label)
            w(hexdump(data, h, 0x60, va))
            w("```")
    w()

    # tag-0x07 / tag-0x0C / list viewport / layer loop: bytes at match
    w("### remaining rows — bytes at match")
    w()
    for idx in (1, 2, 5, 6):
        site, pat = DOC_PATTERNS[idx]
        rx, n, toks = compile_aob(pat)
        w("**%s** (pattern length %d)" % (site, n))
        w()
        for label in labels:
            data = blobs[label]
            ts, ib, secs = meta[label]
            for h in matches[(site, label)]:
                va, _ = file_off_to_va(h, secs, ib)
                w("- %s `%X`: `%s`" % (label, va, hexb(data[h:h + n])))
        w()

    # list viewport: decode the RBP displacements + LEA target
    site, pat = DOC_PATTERNS[5]
    rx, n, toks = compile_aob(pat)
    w("**list viewport dims — decoded fields (first hit only)**")
    w()
    w("| build | match | [RBP+d1]=0x500 | [RBP+d2]=0x2d0 | LEA RAX target | [RBP+d3]=RAX | [RBP+d4]=0x500 | [RBP+d5]=0x2d0 |")
    w("|---|---|---|---|---|---|---|---|")
    for label in labels:
        data = blobs[label]
        ts, ib, secs = meta[label]
        hits = matches[(site, label)]
        if not hits:
            continue
        h = hits[0]
        va, _ = file_off_to_va(h, secs, ib)
        d1 = struct.unpack_from("<i", data, h + 2)[0]
        d2 = struct.unpack_from("<i", data, h + 12)[0]
        lea_disp = struct.unpack_from("<i", data, h + 23)[0]
        lea_tgt = va + 27 + lea_disp
        d3 = struct.unpack_from("<i", data, h + 30)[0]
        d4 = struct.unpack_from("<i", data, h + 36)[0]
        d5 = struct.unpack_from("<i", data, h + 46)[0]
        w("| %s | `%X` | 0x%X | 0x%X | `%X` | 0x%X | 0x%X | 0x%X |" % (label, va, d1, d2, lea_tgt, d3, d4, d5))
    w()

    # ---- (7a) candidate patterns -----------------------------------------
    w("## 7a. Candidate HD-flag test patterns (gauge only)")
    w()
    w("| pattern | " + " | ".join(labels) + " |")
    w("|---|" + "---|" * len(labels))
    cand_hits = {}
    for name, pat in CANDIDATE_PATTERNS:
        rx, n, toks = compile_aob(pat)
        cells = []
        for label in labels:
            data = blobs[label]
            ts, ib, secs = meta[label]
            hits = find_all(rx, data)
            cand_hits[(name, label)] = hits
            vas = []
            for h in hits:
                va, _ = file_off_to_va(h, secs, ib)
                vas.append("`%X`" % va)
            shown = vas if len(vas) <= 12 else vas[:12] + ["… (+%d more)" % (len(vas) - 12)]
            cells.append("%d: %s" % (len(hits), ", ".join(shown)))
        w("| `%s` | %s |" % (name, " | ".join(cells)))
    w()
    # For seta/setbe hits, show following bytes to catch a byte store to [RSP/RBP+disp8]
    w("Bytes at each candidate hit (0x18 bytes from match), to spot a `88 44 24 xx` /")
    w("`88 45 xx` byte store (SETcc rows had zero hits; the `ja` rows are listed in full):")
    w()
    for name, pat in CANDIDATE_PATTERNS:
        for label in labels:
            data = blobs[label]
            ts, ib, secs = meta[label]
            if label == "live":
                continue  # byte-identical to 20260825
            if len(cand_hits[(name, label)]) > 40:
                w("- %s: %d hits, not listed" % (label, len(cand_hits[(name, label)])))
                continue
            for h in cand_hits[(name, label)]:
                va, _ = file_off_to_va(h, secs, ib)
                w("- %s `%X`: `%s`" % (label, va, hexb(data[h:h + 0x18])))
    w()

    # ---- (7b) fps imm pattern + AA store ----------------------------------
    w("## 7b. onBoot fps imm pattern + AA-config store")
    w()
    rx_fps, n_fps, _ = compile_aob(FPS_PATTERN)
    rx_aa, n_aa, _ = compile_aob(AA_PATTERN)
    w("fps pattern `%s`" % FPS_PATTERN)
    w()
    w("| build | fps hits | AA-store `C7 44 24 ?? 03 00 00 00` hits within ±0x100 of the fps hit |")
    w("|---|---|---|")
    fps_hits = {}
    for label in labels:
        data = blobs[label]
        ts, ib, secs = meta[label]
        hits = find_all(rx_fps, data)
        fps_hits[label] = hits
        vas = []
        aa_cells = []
        for h in hits:
            va, _ = file_off_to_va(h, secs, ib)
            vas.append("`%X`" % va)
            lo = max(0, h - 0x100)
            for a in find_all(rx_aa, data[lo:h + 0x100 + n_aa]):
                aoff = lo + a
                ava, _ = file_off_to_va(aoff, secs, ib)
                aa_cells.append("`%X` (fps%+d) `%s`" % (ava, aoff - h, hexb(data[aoff:aoff + n_aa])))
        w("| %s | %d: %s | %s |" % (label, len(hits), ", ".join(vas), "; ".join(aa_cells) or "none"))
    w()
    w("Whole-file counts of the AA-store shape `C7 44 24 ?? 03 00 00 00`:")
    w()
    for label in labels:
        w("- %s: %d" % (label, len(find_all(rx_aa, blobs[label]))))
    w()
    w("### 0x80 bytes BEFORE each fps hit, the 0x12-byte pattern, then 0xC0 bytes AFTER, per build")
    w()
    w("(The AA store at fps+105 and the other display-struct stores follow the fps hit, so the")
    w("after-window is included too. Row 0x80 into the dump = the fps match.)")
    w()
    for label in labels:
        data = blobs[label]
        ts, ib, secs = meta[label]
        for h in fps_hits[label]:
            va, _ = file_off_to_va(h, secs, ib)
            w("```")
            w("[%s] fps match at %X; dump starts at %X" % (label, va, va - 0x80))
            w(hexdump(data, h - 0x80, 0x80 + n_fps + 0xC0, va - 0x80))
            w("```")
    w()

    w("### Decode of the onBoot window (byte-identical across the four builds apart from rel32/disp32)")
    w()
    w("Struct base: the call after the AA store is `LEA RCX,[RSP+0x50]; CALL` → display struct = `RSP+0x50`.")
    w("Offsets below are `[RSP+x]` → `struct+(x-0x50)`.")
    w()
    w("| fps-relative | bytes | instruction | struct field |")
    w("|---|---|---|---|")
    w("| -0x2E | `48 89 44 24 50` | `MOV [RSP+0x50],RAX` (vfunc +0x08 result) | +0x00 |")
    w("| -0x20 | `48 89 44 24 58` | `MOV [RSP+0x58],RAX` (vfunc +0x10 result) | +0x08 |")
    w("| -0x1B | `C6 44 24 60 01` | `MOV byte [RSP+0x60],1` | +0x10 = 1 |")
    w("| -0x16 | `44 89 64 24 68` | `MOV [RSP+0x68],R12D` | +0x18 = R12D (pre-AA default) |")
    w("| -0x11..-0x01 | `48 8D 4C 24 40 FF 15 .. 8B 54 24 40 FF CA` | `LEA RCX,[RSP+0x40]; CALL [ord]; MOV EDX,[RSP+0x40]; DEC EDX` | fps query |")
    w("| +0x00 | `C7 44 24 6C 3C 00 00 00` | `MOV [RSP+0x6C],60` | +0x1C = 60 |")
    w("| +0x08 | `75 08` | `JNZ +8` | |")
    w("| +0x0A | `C7 44 24 6C 4B 00 00 00` | `MOV [RSP+0x6C],75` | +0x1C = 75 |")
    w("| +0x12 | `48 8D 4C 24 28 FF 15 ..` | `LEA RCX,[RSP+0x28]; CALL [ord]` | machine-type query → [RSP+0x28] |")
    w("| +0x1D | `8B 54 24 28 85 D2 78 09` | `MOV EDX,[RSP+0x28]; TEST EDX,EDX; JS +9` | negative ⇒ skip (flag left 0) |")
    w("| +0x25 | `83 FA 01 7F 04 32 C0 EB 02 B0 01` | `CMP EDX,1; JG +4; XOR AL,AL; JMP +2; MOV AL,1` | AL = (type > 1) |")
    w("| +0x30 | `88 44 24 62` | `MOV [RSP+0x62],AL` | **+0x12 = HD flag** |")
    w("| +0x34 | `48 8D 4C 24 3C FF 15 ..` | `LEA RCX,[RSP+0x3C]; CALL [ord]` | second query → [RSP+0x3C] |")
    w("| +0x3F | `8B 54 24 3C 85 D2 74 2A 83 FA 01 7E 25 83 FA 04 7F 20` | `TEST; JZ; CMP 1; JLE; CMP 4; JG` (range 2..4 gate) | |")
    w("| +0x51 | `48 8D 4C 24 38 FF 15 ..` | `LEA RCX,[RSP+0x38]; CALL [ord]` | third query → [RSP+0x38] |")
    w("| +0x5C | `8B 54 24 38 85 D2 78 05 83 FA 01 7E 08` | `TEST; JS; CMP 1; JLE` | |")
    w("| +0x69 | `C7 44 24 68 03 00 00 00` | `MOV [RSP+0x68],3` | +0x18 = 3 (AA) |")
    w("| +0x71 | `48 8D 4C 24 50 E8 ..` | `LEA RCX,[RSP+0x50]; CALL` | consume display struct |")
    w()
    w("Note: the HD-flag store is `TEST/JS` + `CMP EDX,1; JG` on the value in `[RSP+0x28]` (EDX), not")
    w("`CMP EAX,1; SETA` — hence the zero hits for the 7a SETcc guesses. The two OBSERVED rows in 7a")
    w("(`85 D2 78 ?? 83 FA 01 7F 04 32 C0 EB 02 B0 01 88 44 24 ??` and its 15-byte core) hit exactly")
    w("once per build, at fps+0x21 / fps+0x25 respectively.")
    w()

    report = "\n".join(R) + "\n"
    with open(out_path, "w") as f:
        f.write(report)
    sys.stdout.write(report)


if __name__ == "__main__":
    main()
