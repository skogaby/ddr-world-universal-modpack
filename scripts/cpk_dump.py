#!/usr/bin/env python3
"""CRI Middleware CPK archive reader (pure Python).

A CPK is a sequence of chunks, each `magic[4] flags:u32 size:u64` followed by an @UTF table:

    @UTF table_size:u32be | rows_off:u32 strings_off:u32 data_off:u32 name:u32
         ncol:u16 row_len:u16 nrows:u32 | columns | rows | strings | data

Offsets after the first 8 bytes are relative to byte 8. A column is `flags:u8 name:u32`; the
high nibble of `flags` says where the value lives (0x10 zero, 0x30 constant after the column
header, 0x50 per row) and the low nibble the type (0 u8, 1 s8, 2 u16, 3 s16, 4 u32, 5 s32,
6 u64, 7 s64, 8 f32, 9 f64, 0xA string offset, 0xB data offset + size). Tables may be XOR
obfuscated (`@UTF` unreadable at the start): byte i ^= m, m = m * 0x15 & 0xFF, m0 = 0x5F.

The `CPK ` header table has one row: TocOffset / TocSize, ItocOffset, EtocOffset,
ContentOffset, Files, Align ... The `TOC ` table lists DirName, FileName, FileSize,
ExtractSize, FileOffset, ID; FileOffset is relative to min(TocOffset, ContentOffset).
A member whose data starts with `CRILAYLA` is compressed: u32 uncompressed size, u32 header
offset; the last 0x100 bytes of the stream (after the 16-byte header) are the raw prefix of the
output and the rest is an LZ stream read backwards bit by bit.
"""

import io
import os
import struct
import sys

UTF_TYPES = {
    0: ('B', 1), 1: ('b', 1), 2: ('H', 2), 3: ('h', 2), 4: ('I', 4), 5: ('i', 4),
    6: ('Q', 8), 7: ('q', 8), 8: ('f', 4), 9: ('d', 8),
}


def _unmask(buf):
    out = bytearray(len(buf))
    m, t = 0x5F, 0x15
    for i, b in enumerate(buf):
        out[i] = b ^ m
        m = (m * t) & 0xFF
    return bytes(out)


def parse_utf(buf):
    """Parse an @UTF table. Returns (table_name, [row dict, ...])."""
    if buf[:4] != b'@UTF':
        buf = _unmask(buf)
        if buf[:4] != b'@UTF':
            raise ValueError('not an @UTF table')
    table_size = struct.unpack('>I', buf[4:8])[0]
    body = buf[8:8 + table_size]
    rows_off, strings_off, data_off, name_off, ncol, row_len, nrows = struct.unpack('>IIIIHHI', body[:0x18])

    def cstr(off):
        end = body.index(b'\0', strings_off + off)
        return body[strings_off + off:end].decode('utf-8', 'replace')

    cols = []
    p = 0x18
    for _ in range(ncol):
        flags = body[p]
        name = cstr(struct.unpack('>I', body[p + 1:p + 5])[0])
        p += 5
        storage, typ = flags & 0xF0, flags & 0x0F
        const = None
        if storage == 0x30:
            const, n = _read_value(body, p, typ, cstr, data_off)
            p += n
        cols.append((name, storage, typ, const))

    rows = []
    for r in range(nrows):
        p = rows_off + r * row_len
        row = {}
        for name, storage, typ, const in cols:
            if storage == 0x10:
                row[name] = 0
            elif storage == 0x30:
                row[name] = const
            elif storage == 0x50:
                row[name], n = _read_value(body, p, typ, cstr, data_off)
                p += n
            else:
                row[name] = None
        rows.append(row)
    return cstr(name_off), rows


def _read_value(body, p, typ, cstr, data_off):
    if typ == 0xA:
        return cstr(struct.unpack('>I', body[p:p + 4])[0]), 4
    if typ == 0xB:
        off, size = struct.unpack('>II', body[p:p + 8])
        return body[data_off + off:data_off + off + size], 8
    fmt, n = UTF_TYPES[typ]
    return struct.unpack('>' + fmt, body[p:p + n])[0], n


def read_chunk_table(f, offset):
    f.seek(offset)
    magic, _flags, size = struct.unpack('<4sIQ', f.read(16))
    return magic, parse_utf(f.read(size))


def crilayla_decompress(data):
    if data[:8] != b'CRILAYLA':
        return data
    out_size, header_off = struct.unpack('<II', data[8:16])
    prefix = data[16 + header_off:16 + header_off + 0x100]
    comp = data[16:16 + header_off]
    out = bytearray(out_size)
    # the stream is read from its end, backwards; output is written from its end backwards
    pos = len(comp) - 1
    bit_pool, bits_left = 0, 0

    def get_bits(n):
        nonlocal pos, bit_pool, bits_left
        v = 0
        need = n
        while need > 0:
            if bits_left == 0:
                bit_pool = comp[pos]
                pos -= 1
                bits_left = 8
            take = min(need, bits_left)
            v = (v << take) | ((bit_pool >> (bits_left - take)) & ((1 << take) - 1))
            bits_left -= take
            need -= take
        return v

    o = out_size - 1
    written = 0
    while written < out_size:
        if get_bits(1):
            back = get_bits(13) + 3
            length = 3
            for lv_bits in (2, 3, 5):
                lv = get_bits(lv_bits)
                length += lv
                if lv != (1 << lv_bits) - 1:
                    break
            else:
                while True:
                    lv = get_bits(8)
                    length += lv
                    if lv != 255:
                        break
            for _ in range(length):
                out[o] = out[o + back]
                o -= 1
                written += 1
        else:
            out[o] = get_bits(8)
            o -= 1
            written += 1
    return prefix + bytes(out)


class Cpk:
    def __init__(self, path):
        self.path = path
        self.f = open(path, 'rb')
        magic, (_, rows) = read_chunk_table(self.f, 0)
        if magic != b'CPK ':
            raise ValueError('not a CPK')
        self.header = rows[0]
        h = self.header
        toc_off, content_off = h.get('TocOffset', 0), h.get('ContentOffset', 0)
        self.base = min(toc_off, content_off) if toc_off and content_off else (toc_off or content_off)
        self.files = []
        if toc_off:
            magic, (_, trows) = read_chunk_table(self.f, toc_off)
            assert magic == b'TOC ', magic
            for r in trows:
                d = r.get('DirName') or ''
                name = r['FileName'] if not d else d + '/' + r['FileName']
                self.files.append({
                    'name': name, 'offset': self.base + r['FileOffset'], 'size': r['FileSize'],
                    'extract_size': r.get('ExtractSize', r['FileSize']), 'id': r.get('ID'),
                })
        elif h.get('ItocOffset'):
            # id-only archives: members are stored in id order at Align granularity
            magic, (_, irows) = read_chunk_table(self.f, h['ItocOffset'])
            align = h.get('Align', 2048)
            entries = []
            for key in ('DataL', 'DataH'):
                blob = irows[0].get(key)
                if blob:
                    _, drows = parse_utf(blob)
                    entries += [(r['ID'], r['FileSize'], r.get('ExtractSize', r['FileSize'])) for r in drows]
            entries.sort()
            off = content_off
            for fid, size, xsize in entries:
                self.files.append({'name': f'{fid:05d}', 'offset': off, 'size': size, 'extract_size': xsize, 'id': fid})
                off += (size + align - 1) // align * align

    def read(self, entry):
        self.f.seek(entry['offset'])
        data = self.f.read(entry['size'])
        if data[:8] == b'CRILAYLA':
            data = crilayla_decompress(data)
        return data

    def extract_all(self, out_dir, verbose=False, filt=None):
        n = 0
        for e in self.files:
            if filt and not filt(e['name']):
                continue
            dest = os.path.join(out_dir, e['name'])
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            with open(dest, 'wb') as o:
                o.write(self.read(e))
            n += 1
            if verbose:
                print(e['name'], e['size'])
        return n


def main(argv):
    if len(argv) < 3 or argv[1] not in ('list', 'extract', 'header'):
        print('usage: cpk_dump.py list|header <archive.cpk>\n       cpk_dump.py extract <archive.cpk> <out dir> [substring filter]')
        return 2
    cpk = Cpk(argv[2])
    if argv[1] == 'header':
        for k, v in cpk.header.items():
            print(f'{k} = {v!r}')
    elif argv[1] == 'list':
        for e in cpk.files:
            print(f"{e['offset']:#012x} {e['size']:>10} {e['extract_size']:>10} {e['name']}")
        print(f'{len(cpk.files)} files')
    else:
        filt = (lambda n, s=argv[4]: s in n) if len(argv) > 4 else None
        print(cpk.extract_all(argv[3], verbose=False, filt=filt), 'files extracted')
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))
