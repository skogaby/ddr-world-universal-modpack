#!/usr/bin/env python3
"""Extract assets from Dancing Stage / DanceDanceRevolution HOTTEST PARTY (Wii, 2007).

Hottest Party runs on Hudson Soft's Mario Party engine, so its data uses Hudson's formats. The
Mario Party 4 decompilation (mariopartyrd/marioparty4) documents them, and each one is checked
against this game's own files. See docs/wii_ddr_hottest_party_research.md for the RE.

Input: the game's file tree, either the unpacked disc (`DATA/files` of a Dolphin / wit dump:
data/, dll/, mess/, movie/, sound/, sys/main.dol...) or a .wbfs / .iso image read directly
(`disc` subcommand; the partition is decrypted with the Wii common key, so this needs the
`cryptography` package).

Data packs (data/*.bin; HuData in the decomp). The file id is `dir << 16 | index`, and `dir`
indexes a table of path strings in main.dol. Each pack is:

    u32 count, u32 offset[count]          (an entry ends where the next one starts)
    entry: u32 raw_size, u32 codec, codec data

The codecs (decode.c HuDecodeData):
  - 0 none;
  - 1 LZ: an 8-flag LZSS, 1 = literal, with a 1 KiB ring that starts at 958;
  - 2 slide, 3 / 4 fslide: a u32 size, then 32-bit flag words; a pair is u16 {len:4, dist:12},
    and len 0 means an extra byte + 18;
  - 5 RLE;
  - 7 zlib: { u32 raw_size, u32 zlib_size }, then a zlib stream (the Wii Mario Party engine).
Every entry is a self-contained file. These kinds are recognized by structure:
  - .hsf: an HSFV037 model / motion / camera (decoder: scripts/hsf_dump.py);
  - .spr: a Hudson sprite animation (ANIMDATA): s16 bank, pattern and bitmap counts, then
    offsets to the bank / pattern / bitmap tables. The bitmaps are GX textures with an RGB5A3
    palette;
  - .ssq: a DDR step chart (little-endian SSQ chunks: tempo, then steps), entry 0 of every
    per-song bundle data/c_000_NN.bin;
  - .bin: anything else.

Other disc files:
  - .tpl: GX texture container → .png;
  - .arc: U8 archive, unpacked recursively;
  - opening.bnr: IMET + U8; its IMD5 / LZ77 members are unwrapped;
  - .brstm: streamed audio, copied; with --wav, decoded;
  - sound/ddr.brsar: the sound archive (RSAR: SYMB names, INFO tables, FILE groups). It is split
    into its RWSD / RBNK / RSEQ / RWAR files, one folder per group (named from SYMB), each file
    beside its group's raw wave block (`.waves`), plus sounds.csv (sound -> file, type, player).
    With --wav, every wave of every RWSD (data in the wave block) and RWAR is decoded to
    `<file>.<n>.wav` -- each song is one stereo wave;
  - .thp: movies, copied; with --mp4, converted by ffmpeg;
  - mess/*.bin: message banks, copied, plus a .txt with the decoded strings;
  - dll/*.rel, sys/main.dol: copied.

Pictures. HSF bitmaps, sprite bitmaps and TPL images are GX-tiled textures in these formats:
I4, I8, IA4, IA8, RGB565, RGB5A3, RGBA8, C4, C8, C14X2 (with an IA8 / RGB565 / RGB5A3 palette)
and CMPR (DXT1-like, big-endian, 2x2 sub-blocks). With --png they are decoded by decode_gx
(numpy).

Audio: BRSTM / RWSD / RWAR waves are DSP-ADPCM (8-byte frames, 14 samples each, two-tap
predictor per channel); --wav decodes them in pure Python (about 1.5 s per stereo song).

Usage:
    extract_wii_ddr_data.py extract <game_dir> <out_dir> [--png] [--wav] [--mp4]
                            [--only data,sound,...] [--files 'stg*.bin,c_0*.bin']
    extract_wii_ddr_data.py disc <game.wbfs|game.iso> <out_dir>   # dump the file tree first
    extract_wii_ddr_data.py pack <file.bin> <out_dir> [--png]      # one data pack
    extract_wii_ddr_data.py png <file.tpl|.spr|.hsf> <out_dir>     # pictures of one file
    extract_wii_ddr_data.py wav <file.brstm> <out.wav>

Outputs:
  - data/<pack>/<index>.<ext>: the pack members (+ .spr.json), with their .png;
  - sound/ddr_brsar/<group>/file<NNN>.<ext> (+ .waves, .<n>.wav) and sound/ddr_brsar/sounds.csv;
  - copies of every other disc file under their own path, with their decoded sidecars;
  - manifest.csv: one row per pack member (pack, index, file id, codec, raw size, kind, path)
    and per sound file;
  - dol/: data_dirs.csv and songs.csv (main.dol), dance_clip_bars.csv (dll/danceviewDll.rel).

Import-safe: `from extract_wii_ddr_data import read_pack, decode_entry, decode_gx, ...`.
Needs numpy for --png and the pictures; everything else is the standard library.
Tests: scripts/validate_wii_ddr_tools.sh.
"""
import argparse
import csv
import fnmatch
import json
import os
import shutil
import struct
import subprocess
import sys
import zlib

try:
    import numpy as np
except ImportError:  # pragma: no cover - pictures need numpy
    np = None

# ---------------------------------------------------------------------------
# GX textures
# ---------------------------------------------------------------------------
GX_I4, GX_I8, GX_IA4, GX_IA8, GX_RGB565, GX_RGB5A3, GX_RGBA8 = 0, 1, 2, 3, 4, 5, 6
GX_C4, GX_C8, GX_C14X2, GX_CMPR = 8, 9, 10, 14
GX_A8 = 0x27  # GX_CTF_A8 (sprites only)
TL_IA8, TL_RGB565, TL_RGB5A3 = 0, 1, 2
GX_NAMES = {GX_I4: 'I4', GX_I8: 'I8', GX_IA4: 'IA4', GX_IA8: 'IA8', GX_RGB565: 'RGB565', GX_RGB5A3: 'RGB5A3',
            GX_RGBA8: 'RGBA8', GX_C4: 'C4', GX_C8: 'C8', GX_C14X2: 'C14X2', GX_CMPR: 'CMPR', GX_A8: 'A8'}
# format -> (block width, block height, bits per pixel)
GX_BLOCKS = {GX_I4: (8, 8, 4), GX_I8: (8, 4, 8), GX_IA4: (8, 4, 8), GX_IA8: (4, 4, 16), GX_RGB565: (4, 4, 16),
             GX_RGB5A3: (4, 4, 16), GX_RGBA8: (4, 4, 32), GX_C4: (8, 8, 4), GX_C8: (8, 4, 8), GX_C14X2: (4, 4, 16),
             GX_CMPR: (8, 8, 4), GX_A8: (8, 4, 8)}


def gx_size(fmt, width, height):
    """Bytes of the base level of a GX texture (dimensions padded to whole blocks)."""
    bw, bh, bpp = GX_BLOCKS[fmt]
    return ((width + bw - 1) // bw) * ((height + bh - 1) // bh) * bw * bh * bpp // 8


def _need_numpy():
    if np is None:
        raise RuntimeError('numpy is needed to decode pictures')


def _untile(values, width, height, bw, bh):
    """Per-pixel values in GX tile order (block rows, blocks, rows in block, pixels in row)
    -> an (height, width, ...) array cropped to the texture size."""
    bx, by = (width + bw - 1) // bw, (height + bh - 1) // bh
    rest = values.shape[1:]
    v = values[:bx * by * bw * bh].reshape((by, bx, bh, bw) + rest)
    v = v.transpose((0, 2, 1, 3) + tuple(range(4, 4 + len(rest)))).reshape((by * bh, bx * bw) + rest)
    return v[:height, :width]


def _rgb5a3(v):
    v = v.astype(np.uint32)
    opaque = (v & 0x8000) != 0
    r5, g5, b5 = (v >> 10) & 31, (v >> 5) & 31, v & 31
    r4, g4, b4, a3 = (v >> 8) & 15, (v >> 4) & 15, v & 15, (v >> 12) & 7
    out = np.empty(v.shape + (4,), np.uint8)
    out[..., 0] = np.where(opaque, (r5 << 3) | (r5 >> 2), r4 * 17)
    out[..., 1] = np.where(opaque, (g5 << 3) | (g5 >> 2), g4 * 17)
    out[..., 2] = np.where(opaque, (b5 << 3) | (b5 >> 2), b4 * 17)
    out[..., 3] = np.where(opaque, 255, (a3 << 5) | (a3 << 2) | (a3 >> 1))
    return out


def _rgb565(v):
    v = v.astype(np.uint32)
    r, g, b = (v >> 11) & 31, (v >> 5) & 63, v & 31
    out = np.empty(v.shape + (4,), np.uint8)
    out[..., 0] = (r << 3) | (r >> 2)
    out[..., 1] = (g << 2) | (g >> 4)
    out[..., 2] = (b << 3) | (b >> 2)
    out[..., 3] = 255
    return out


def _ia8(v):
    v = v.astype(np.uint32)
    out = np.empty(v.shape + (4,), np.uint8)
    i = v & 0xFF
    out[..., 0] = out[..., 1] = out[..., 2] = i
    out[..., 3] = v >> 8
    return out


def decode_palette(data, tl_format):
    """(n, 4) RGBA of a GX palette (TLUT) of big-endian 16-bit entries."""
    _need_numpy()
    v = np.frombuffer(data[:len(data) // 2 * 2], '>u2')
    return {TL_IA8: _ia8, TL_RGB565: _rgb565, TL_RGB5A3: _rgb5a3}[tl_format](v)


def _nibbles(raw):
    b = np.frombuffer(raw, np.uint8)
    out = np.empty(len(b) * 2, np.uint8)
    out[0::2] = b >> 4
    out[1::2] = b & 15
    return out


def _cmpr(raw, width, height):
    bx, by = (width + 7) // 8, (height + 7) // 8
    n = bx * by * 4
    blk = np.frombuffer(raw[:n * 8], np.uint8).reshape(n, 8)
    c0 = (blk[:, 0].astype(np.uint32) << 8) | blk[:, 1]
    c1 = (blk[:, 2].astype(np.uint32) << 8) | blk[:, 3]
    p0, p1 = _rgb565(c0).astype(np.int32), _rgb565(c1).astype(np.int32)
    four = c0 > c1
    pal = np.empty((n, 4, 4), np.int32)
    pal[:, 0], pal[:, 1] = p0, p1
    pal[:, 2] = np.where(four[:, None], (2 * p0 + p1) // 3, (p0 + p1) // 2)
    pal[:, 3] = np.where(four[:, None], (p0 + 2 * p1) // 3, 0)
    pal[:, 2:, 3] = np.where(four[:, None], 255, pal[:, 2:, 3])
    pal[:, 2, 3] = 255
    idx = np.empty((n, 16), np.uint8)
    for row in range(4):
        b = blk[:, 4 + row]
        for col in range(4):
            idx[:, 4 * row + col] = (b >> (6 - 2 * col)) & 3
    px = pal[np.arange(n)[:, None], idx].astype(np.uint8)          # (n, 16, 4)
    # sub-blocks: (block row, block, sub row, sub col, 4 x 4 pixels)
    px = px.reshape(by, bx, 2, 2, 4, 4, 4).transpose(0, 2, 4, 1, 3, 5, 6).reshape(by * 8, bx * 8, 4)
    return px[:height, :width]


def decode_gx(raw, width, height, fmt, palette=None):
    """RGBA uint8 (height, width, 4) of the base level of a GX texture. `palette` is an (n, 4)
    RGBA array (decode_palette) for C4 / C8 / C14X2."""
    _need_numpy()
    bw, bh, bpp = GX_BLOCKS[fmt]
    size = gx_size(fmt, width, height)
    raw = bytes(raw[:size]).ljust(size, b'\0')
    if fmt == GX_CMPR:
        return _cmpr(raw, width, height)
    if fmt == GX_RGBA8:
        n = len(raw) // 64
        b = np.frombuffer(raw, np.uint8).reshape(n, 2, 16, 2)       # (block, AR|GB, pixel, pair)
        px = np.empty((n, 16, 4), np.uint8)
        px[..., 3], px[..., 0] = b[:, 0, :, 0], b[:, 0, :, 1]
        px[..., 1], px[..., 2] = b[:, 1, :, 0], b[:, 1, :, 1]
        return _untile(px.reshape(n * 16, 4), width, height, bw, bh)
    if bpp == 4:
        v = _nibbles(raw)
    elif bpp == 8:
        v = np.frombuffer(raw, np.uint8)
    else:
        v = np.frombuffer(raw, '>u2')
    v = _untile(v, width, height, bw, bh)
    if fmt in (GX_I4, GX_I8):
        i = (v * 17).astype(np.uint8) if fmt == GX_I4 else v.astype(np.uint8)
        return np.stack([i, i, i, i], -1)
    if fmt == GX_A8:
        a = v.astype(np.uint8)
        return np.stack([np.full_like(a, 255)] * 3 + [a], -1)
    if fmt == GX_IA4:
        i, a = (v & 15) * 17, (v >> 4) * 17
        i, a = i.astype(np.uint8), a.astype(np.uint8)
        return np.stack([i, i, i, a], -1)
    if fmt == GX_IA8:
        return _ia8(v)
    if fmt == GX_RGB565:
        return _rgb565(v)
    if fmt == GX_RGB5A3:
        return _rgb5a3(v)
    if fmt in (GX_C4, GX_C8, GX_C14X2):
        if palette is None or not len(palette):
            g = (v.astype(np.uint32) * 255 // max(1, (1 << min(bpp, 14)) - 1)).astype(np.uint8)
            return np.stack([g, g, g, np.full_like(g, 255)], -1)
        idx = (v & 0x3FFF) if fmt == GX_C14X2 else v
        return palette[np.minimum(idx, len(palette) - 1)]
    raise ValueError('unsupported GX format %r' % fmt)


def write_png(path, width, height, rgba):
    """Write 8-bit RGBA pixels (bytes, rows top-down) as a PNG."""
    def chunk(tag, body):
        return struct.pack('>I', len(body)) + tag + body + struct.pack('>I', zlib.crc32(tag + body) & 0xFFFFFFFF)

    stride = width * 4
    rows = b''.join(b'\0' + rgba[y * stride:(y + 1) * stride] for y in range(height))
    with open(path, 'wb') as f:
        f.write(b'\x89PNG\r\n\x1a\n')
        f.write(chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 6, 0, 0, 0)))
        f.write(chunk(b'IDAT', zlib.compress(rows, 6)))
        f.write(chunk(b'IEND', b''))


def safe_name(name):
    """A file-system-safe stem for an asset name (`tex*emi*01.tga` -> `tex_emi_01.tga`)."""
    out = ''.join(c if (c.isalnum() or c in '._-') else '_' for c in name)
    return out.strip('._') or 'unnamed'


# ---------------------------------------------------------------------------
# Hudson data packs (HuData) and codecs
# ---------------------------------------------------------------------------
CODEC_NONE, CODEC_LZ, CODEC_SLIDE, CODEC_FSLIDE_ALT, CODEC_FSLIDE, CODEC_RLE, CODEC_ZLIB = 0, 1, 2, 3, 4, 5, 7
CODEC_NAMES = {0: 'none', 1: 'lz', 2: 'slide', 3: 'fslide', 4: 'fslide', 5: 'rle', 7: 'zlib'}


class PackError(ValueError):
    pass


def decode_lz(src, size, pos=0):
    """HuDecodeLz: 8 flags per control byte (1 = literal), pairs {lo, hi:2 | len:6} address a
    1 KiB ring that starts at 958, length + 3. Returns (data, bytes consumed)."""
    ring = bytearray(1024)
    rp = 958
    out = bytearray()
    flag = 0
    s = pos
    while len(out) < size:
        flag >>= 1
        if not flag & 0x100:
            flag = src[s] | 0xFF00
            s += 1
        if flag & 1:
            b = src[s]
            s += 1
            out.append(b)
            ring[rp] = b
            rp = (rp + 1) & 0x3FF
        else:
            i = src[s] | ((src[s + 1] & 0xC0) << 2)
            n = (src[s + 1] & 0x3F) + 3
            s += 2
            for j in range(n):
                b = ring[(i + j) & 0x3FF]
                out.append(b)
                ring[rp] = b
                rp = (rp + 1) & 0x3FF
    return bytes(out[:size]), s - pos


def decode_slide(src, size, pos=0, zero_fill=True):
    """HuDecodeSlide / HuDecodeFslide: a u32 size, then 32-flag words (1 = literal); a pair
    u16 {len:4, dist:12} copies len + 2 bytes (len 0: a byte + 18) from dist + 1 back."""
    s = pos + 4
    out = bytearray()
    flag, left = 0, 0
    while len(out) < size:
        if left == 0:
            flag = struct.unpack_from('>I', src, s)[0]
            s += 4
            left = 32
        if flag & 0x80000000:
            out.append(src[s])
            s += 1
        else:
            d = (src[s] << 8) | src[s + 1]
            s += 2
            n = (d >> 12) & 15
            d &= 0xFFF
            if n == 0:
                n = src[s] + 18
                s += 1
            else:
                n += 2
            start = len(out) - d - 1
            for k in range(n):
                j = start + k
                out.append(out[j] if j >= 0 else 0)
        flag = (flag << 1) & 0xFFFFFFFF
        left -= 1
    return bytes(out[:size]), s - pos


def decode_rle(src, size, pos=0):
    s = pos
    out = bytearray()
    while len(out) < size:
        n = src[s]
        s += 1
        if n < 128:
            out += bytes([src[s]]) * n
            s += 1
        else:
            out += src[s:s + n - 128]
            s += n - 128
    return bytes(out[:size]), s - pos


def decode_entry(blob, offset=0, end=None):
    """One HuData entry at `offset`: {u32 raw_size, u32 codec, data} -> (raw bytes, codec)."""
    end = len(blob) if end is None else end
    if end - offset < 8:
        raise PackError('entry too short')
    size, codec = struct.unpack_from('>II', blob, offset)
    body = offset + 8
    if codec == CODEC_ZLIB:
        size2, zsize = struct.unpack_from('>II', blob, body)
        raw = zlib.decompress(bytes(blob[body + 8:body + 8 + zsize]))
    elif codec == CODEC_NONE:
        raw = bytes(blob[body:body + size])
    elif codec == CODEC_LZ:
        raw, _ = decode_lz(blob, size, body)
    elif codec in (CODEC_SLIDE, CODEC_FSLIDE, CODEC_FSLIDE_ALT):
        raw, _ = decode_slide(blob, size, body)
    elif codec == CODEC_RLE:
        raw, _ = decode_rle(blob, size, body)
    else:
        raise PackError('unknown codec %d' % codec)
    if len(raw) != size:
        raise PackError('size mismatch: %d != %d' % (len(raw), size))
    return raw, codec


def read_pack(blob):
    """The (offset, end) span of every entry of a HuData pack."""
    if len(blob) < 4:
        raise PackError('too short')
    n = struct.unpack_from('>I', blob, 0)[0]
    if not 0 < n < 0x10000 or 4 + 4 * n > len(blob):
        raise PackError('bad entry count %d' % n)
    offs = list(struct.unpack_from('>%dI' % n, blob, 4))
    if offs[0] < 4 + 4 * n or any(b < a for a, b in zip(offs, offs[1:])) or offs[-1] > len(blob):
        raise PackError('entry offsets are not ascending inside the file')
    return list(zip(offs, offs[1:] + [len(blob)]))


def is_pack(blob):
    try:
        spans = read_pack(blob)
        size, codec = struct.unpack_from('>II', blob, spans[0][0])
        return codec in CODEC_NAMES and size > 0
    except (PackError, struct.error):
        return False


# ---------------------------------------------------------------------------
# member kinds
# ---------------------------------------------------------------------------
def is_ssq(blob):
    """A DDR step chart: little-endian chunks {u32 length, u16 type, ...}, a tempo chunk (type 1)
    first, and a zero-length terminator."""
    if len(blob) < 16:
        return False
    pos = 0
    first = True
    while pos + 4 <= len(blob):
        n = struct.unpack_from('<I', blob, pos)[0]
        if n == 0:
            return not first
        if n < 8 or pos + n > len(blob):
            return False
        if first and struct.unpack_from('<H', blob, pos + 4)[0] != 1:
            return False
        first = False
        pos += n
    return False


def sprite_header(blob):
    if len(blob) < 0x14:
        return None
    banks, pats, bmps, _use = struct.unpack_from('>hhhh', blob, 0)
    ob, op, om = struct.unpack_from('>III', blob, 8)
    if not (0 < banks < 0x400 and 0 < pats < 0x2000 and 0 < (bmps & 0x7FFF) < 0x400):
        return None
    if not all(0x14 <= o < len(blob) for o in (ob, op, om)):
        return None
    return banks, pats, bmps & 0x7FFF, ob, op, om


# ANIMDATA bitmap formats (animdata.h ANIM_BMP_*) -> GX format
SPRITE_FORMATS = {0: GX_RGBA8, 1: GX_RGB5A3, 2: GX_RGB5A3, 3: GX_C8, 4: GX_C4, 5: GX_IA8, 6: GX_IA4, 7: GX_I8,
                  8: GX_I4, 9: GX_A8, 10: GX_CMPR}


def parse_sprite(blob):
    """A Hudson sprite animation (sprman.c HuSprAnimRead): banks of frames, patterns of layers,
    bitmaps. Returns a JSON-able dict with the bitmaps' raw fields (`data`, `palette` offsets)."""
    h = sprite_header(blob)
    if h is None:
        raise ValueError('not a sprite')
    nbank, npat, nbmp, ob, op, om = h
    banks = []
    for i in range(nbank):
        nframes, _u, fo = struct.unpack_from('>hhI', blob, ob + 8 * i)
        frames = [dict(zip(('pat', 'time', 'shift_x', 'shift_y', 'flip'),
                           struct.unpack_from('>5h', blob, fo + 12 * k))) for k in range(nframes)]
        banks.append(frames)
    pats = []
    for i in range(npat):
        nlay, cx, cy, sx, sy = struct.unpack_from('>5h', blob, op + 16 * i)
        lo = struct.unpack_from('>I', blob, op + 16 * i + 12)[0]
        layers = []
        for k in range(nlay):
            o = lo + 32 * k
            alpha, flip = blob[o], blob[o + 1]
            bmp, x, y, w, hh, shx, shy = struct.unpack_from('>7h', blob, o + 2)
            vtx = struct.unpack_from('>8h', blob, o + 16)
            layers.append(dict(alpha=alpha, flip=flip, bmp=bmp, start=(x, y), size=(w, hh), shift=(shx, shy),
                               vtx=list(vtx)))
        pats.append(dict(center=(cx, cy), size=(sx, sy), layers=layers))
    bmps = []
    for i in range(nbmp):
        o = om + 20 * i
        pix, fmt = blob[o], blob[o + 1]
        pal_num, w, hh = struct.unpack_from('>hhh', blob, o + 2)
        size, pal, data = struct.unpack_from('>III', blob, o + 8)
        bmps.append(dict(pix_size=pix, format=fmt & 0xF, pal_num=pal_num, width=w, height=hh, size=size,
                         palette=pal, data=data))
    return dict(banks=banks, patterns=pats, bitmaps=bmps)


def sprite_bitmap_rgba(blob, bmp):
    gx = SPRITE_FORMATS[bmp['format']]
    pal = None
    if gx in (GX_C4, GX_C8):
        pal = decode_palette(blob[bmp['palette']:bmp['palette'] + 2 * bmp['pal_num']], TL_RGB5A3)
    return decode_gx(blob[bmp['data']:], bmp['width'], bmp['height'], gx, pal)


def kind_of(blob):
    """The extension a pack member is written with."""
    if blob[:8] == b'HSFV037\0':
        return 'hsf'
    if is_ssq(blob):
        return 'ssq'
    if blob[:4] == b'\x00\x20\xaf\x30':
        return 'tpl'
    if blob[:4] == b'\x55\xaa\x38\x2d':
        return 'arc'
    if sprite_header(blob) is not None:
        try:
            parse_sprite(blob)
            return 'spr'
        except (struct.error, ValueError, IndexError):
            pass
    return 'bin'


# ---------------------------------------------------------------------------
# TPL, U8, Nintendo LZ77, IMD5 / IMET
# ---------------------------------------------------------------------------
TPL_MAGIC = b'\x00\x20\xaf\x30'


def parse_tpl(blob):
    """[(width, height, gx format, data offset, palette or None)] of a TPL."""
    if blob[:4] != TPL_MAGIC:
        raise ValueError('not a TPL')
    n, table = struct.unpack_from('>II', blob, 4)
    out = []
    for i in range(n):
        img, pal = struct.unpack_from('>II', blob, table + 8 * i)
        h, w, fmt, data = struct.unpack_from('>HHII', blob, img)
        palette = None
        if pal:
            count, _unpacked, _pad, pfmt, pdata = struct.unpack_from('>HBBII', blob, pal)
            palette = (pfmt, blob[pdata:pdata + 2 * count])
        out.append((w, h, fmt, data, palette))
    return out


def tpl_images(blob):
    """RGBA arrays of every TPL image."""
    out = []
    for w, h, fmt, data, palette in parse_tpl(blob):
        pal = decode_palette(palette[1], palette[0]) if palette else None
        out.append(decode_gx(blob[data:], w, h, fmt, pal))
    return out


U8_MAGIC = b'\x55\xaa\x38\x2d'


def parse_u8(blob):
    """[(path, data)] of a U8 archive (directories implied by the paths)."""
    if blob[:4] != U8_MAGIC:
        raise ValueError('not a U8 archive')
    root = struct.unpack_from('>I', blob, 4)[0]
    total = struct.unpack_from('>I', blob, root + 8)[0]
    names = root + 12 * total
    out = []
    stack = [(total, '')]

    def name(off):
        e = blob.index(b'\0', names + off)
        return blob[names + off:e].decode('latin1')

    for i in range(1, total):
        while i >= stack[-1][0] and len(stack) > 1:
            stack.pop()
        word, a, b = struct.unpack_from('>III', blob, root + 12 * i)
        nm = name(word & 0xFFFFFF)
        path = stack[-1][1] + nm
        if word >> 24:
            stack.append((b, path + '/'))
        else:
            out.append((path, blob[a:a + b]))
    return out


def decode_nlz(blob, pos=0):
    """Nintendo LZ77 (type 0x10 LZ10 / 0x11 LZ11), header u32le {type:8, size:24}."""
    kind = blob[pos]
    size = struct.unpack_from('<I', blob, pos)[0] >> 8
    s = pos + 4
    if size == 0:
        size = struct.unpack_from('<I', blob, s)[0]
        s += 4
    out = bytearray()
    while len(out) < size:
        flags = blob[s]
        s += 1
        for bit in range(8):
            if len(out) >= size:
                break
            if not flags & (0x80 >> bit):
                out.append(blob[s])
                s += 1
                continue
            if kind == 0x10:
                n = (blob[s] >> 4) + 3
                d = (((blob[s] & 15) << 8) | blob[s + 1]) + 1
                s += 2
            else:
                ind = blob[s] >> 4
                if ind == 0:
                    n = (((blob[s] & 15) << 4) | (blob[s + 1] >> 4)) + 0x11
                    d = (((blob[s + 1] & 15) << 8) | blob[s + 2]) + 1
                    s += 3
                elif ind == 1:
                    n = (((blob[s] & 15) << 12) | (blob[s + 1] << 4) | (blob[s + 2] >> 4)) + 0x111
                    d = (((blob[s + 2] & 15) << 8) | blob[s + 3]) + 1
                    s += 4
                else:
                    n = ind + 1
                    d = (((blob[s] & 15) << 8) | blob[s + 1]) + 1
                    s += 2
            for _ in range(n):
                out.append(out[-d])
    return bytes(out[:size])


def unwrap(blob):
    """Strip the Wii wrappers that hide an archive: IMD5 (0x20), "LZ77" + LZ10/11, bare LZ10/11
    in front of a U8, the IMET banner header. Returns (inner bytes, [wrapper names])."""
    wrappers = []
    for _ in range(4):
        if blob[:4] == b'IMD5':
            blob, w = blob[0x20:], 'imd5'
        elif blob[:4] == b'LZ77' and blob[4] in (0x10, 0x11):
            blob, w = decode_nlz(blob, 4), 'lz77'
        elif len(blob) > 0x44 and blob[0x40:0x44] == b'IMET':
            blob, w = blob[0x600:], 'imet'
        elif blob[:1] in (b'\x10', b'\x11') and len(blob) > 8:
            try:
                inner = decode_nlz(blob)
            except (IndexError, struct.error):
                break
            if inner[:4] != U8_MAGIC:
                break
            blob, w = inner, 'lz'
        else:
            break
        wrappers.append(w)
    return blob, wrappers


# ---------------------------------------------------------------------------
# audio: DSP-ADPCM, BRSTM, RSAR / RWSD / RWAR / RWAV
# ---------------------------------------------------------------------------
def _clamp16(v):
    return -32768 if v < -32768 else 32767 if v > 32767 else v


def dsp_decode(data, samples, coefs, hist1=0, hist2=0):
    """Nintendo DSP-ADPCM: 8-byte frames {u8 ps (predictor << 4 | scale), 7 bytes = 14
    signed nibbles}; s = ((n << scale) << 11) + 1024 + c1 * h1 + c2 * h2 >> 11, clamped to
    int16 (the same output as ffmpeg's adpcm_thp, within the last bits of rounding). Returns a
    list of int16."""
    out = []
    pos = 0
    while len(out) < samples and pos + 8 <= len(data):
        ps = data[pos]
        scale = 1 << (ps & 15)
        c1, c2 = coefs[2 * ((ps >> 4) & 7)], coefs[2 * ((ps >> 4) & 7) + 1]
        for b in data[pos + 1:pos + 8]:
            for nib in (b >> 4, b & 15):
                if nib >= 8:
                    nib -= 16
                s = _clamp16(((nib * scale) << 11) + 1024 + c1 * hist1 + c2 * hist2 >> 11)
                out.append(s)
                hist2, hist1 = hist1, s
        pos += 8
    return out[:samples]


def nibbles_to_samples(nibbles):
    return (nibbles // 16) * 14 + max(0, nibbles % 16 - 2)


def write_wav(path, rate, channels, pcm_channels):
    """PCM16 WAV from per-channel int16 sample lists (equal lengths)."""
    n = min(len(c) for c in pcm_channels) if pcm_channels else 0
    if np is not None:
        inter = np.stack([np.asarray(c[:n], np.int16) for c in pcm_channels], 1).astype('<i2').tobytes()
    else:
        inter = b''.join(struct.pack('<%dh' % channels, *(c[i] for c in pcm_channels)) for i in range(n))
    with open(path, 'wb') as f:
        f.write(b'RIFF' + struct.pack('<I', 36 + len(inter)) + b'WAVEfmt ')
        f.write(struct.pack('<IHHIIHH', 16, 1, channels, rate, rate * channels * 2, channels * 2, 16))
        f.write(b'data' + struct.pack('<I', len(inter)) + inter)


def parse_adpcm_info(blob, off):
    coefs = list(struct.unpack_from('>16h', blob, off))
    gain, ps, h1, h2, lps, lh1, lh2 = struct.unpack_from('>Hhhhhhh', blob, off + 32)
    return dict(coefs=coefs, ps=ps, hist1=h1, hist2=h2, loop_ps=lps, loop_hist1=lh1, loop_hist2=lh2)


def parse_brstm(blob):
    """Stream info + per-channel (adpcm info, data bytes) of an RSTM, de-interleaved."""
    if blob[:4] != b'RSTM':
        raise ValueError('not an RSTM')
    head, _hs, _ao, _as, data, _ds = struct.unpack_from('>6I', blob, 0x10)
    base = head + 8
    p1, p3 = struct.unpack_from('>I', blob, base + 4)[0], struct.unpack_from('>I', blob, base + 20)[0]
    o = base + p1
    codec, loop, channels = blob[o], blob[o + 1], blob[o + 2]
    rate = struct.unpack_from('>H', blob, o + 4)[0]
    loop_start, total, data_off, nblocks, bsize, bsamples, lsize, lsamples, lpadded = \
        struct.unpack_from('>9I', blob, o + 8)
    chan = []
    o3 = base + p3
    nch = blob[o3]
    for c in range(nch):
        ci = base + struct.unpack_from('>I', blob, o3 + 4 + 8 * c + 4)[0]
        ai = struct.unpack_from('>I', blob, ci + 4)[0]
        chan.append(parse_adpcm_info(blob, base + ai) if codec == 2 else None)
    start = data + 8 + struct.unpack_from('>I', blob, data + 8)[0]   # DATA {magic, size, u32 0x18} -> +0x20
    streams = [bytearray() for _ in range(channels)]
    pos = start
    for b in range(nblocks):
        last = b == nblocks - 1
        size, pad = (lsize, lpadded) if last else (bsize, bsize)
        for c in range(channels):
            streams[c] += blob[pos:pos + size]
            pos += pad
    return dict(codec=codec, loop=loop, channels=channels, rate=rate, loop_start=loop_start, samples=total,
                adpcm=chan, data=[bytes(s) for s in streams])


def wave_to_wav(out_path, wave):
    """Write one decoded wave {format (0 PCM8, 1 PCM16, 2 DSP-ADPCM), channels, rate, samples,
    adpcm, data} as a PCM16 WAV."""
    fmt, rate, n = wave['format'], wave['rate'], wave['samples']
    if fmt == 2:
        chans = [dsp_decode(d, n, a['coefs'], a['hist1'], a['hist2']) for d, a in zip(wave['data'], wave['adpcm'])]
    elif fmt == 1:
        chans = [list(struct.unpack('>%dh' % min(n, len(d) // 2), d[:2 * min(n, len(d) // 2)])) for d in wave['data']]
    else:
        chans = [[(b - 256 if b > 127 else b) << 8 for b in d[:n]] for d in wave['data']]
    write_wav(out_path, rate, len(chans), chans)


def parse_wave_info(blob, info, wave_base):
    """One RWSD WAVE entry / RWAV INFO body at `info`: {u8 format, loop, channels, rate_hi,
    u16 rate, u16, u32 loop_start, loop_end (nibbles for ADPCM), channel table offset,
    data location, ...}; channel info {u32 data offset, u32 adpcm offset, ...}. Offsets are
    relative to `info`; the data to `wave_base + data location`."""
    fmt, loop, nch, rate_hi = blob[info], blob[info + 1], blob[info + 2], blob[info + 3]
    rate = (rate_hi << 16) | struct.unpack_from('>H', blob, info + 4)[0]
    loop_start, loop_end, ctab, data_loc = struct.unpack_from('>4I', blob, info + 8)
    samples = nibbles_to_samples(loop_end) if fmt == 2 else loop_end
    frame_bytes = (loop_end + 15) // 16 * 8 if fmt == 2 else loop_end * (2 if fmt == 1 else 1)
    adpcm, data = [], []
    for c in range(nch):
        ci = info + struct.unpack_from('>I', blob, info + ctab + 4 * c)[0]
        doff, aoff = struct.unpack_from('>II', blob, ci)
        adpcm.append(parse_adpcm_info(blob, info + aoff) if fmt == 2 else None)
        s = wave_base + data_loc + doff
        data.append(bytes(blob[s:s + frame_bytes]))
    return dict(format=fmt, loop=loop, channels=nch, rate=rate, loop_start=loop_start, samples=samples,
                adpcm=adpcm, data=data)


def rwsd_waves(rwsd, wave_area):
    """Waves of an RWSD whose wave data sits in `wave_area` (the group's wave block)."""
    if rwsd[:4] != b'RWSD':
        raise ValueError('not an RWSD')
    wave = struct.unpack_from('>I', rwsd, 0x18)[0]
    n = struct.unpack_from('>I', rwsd, wave + 8)[0]
    blob = rwsd + wave_area
    out = []
    for i in range(n):
        info = wave + struct.unpack_from('>I', rwsd, wave + 12 + 4 * i)[0]
        out.append(parse_wave_info(blob, info, len(rwsd)))
    return out


def rwar_waves(rwar):
    """Waves of an RWAR (its TABL lists RWAV files in DATA)."""
    if rwar[:4] != b'RWAR':
        raise ValueError('not an RWAR')
    tabl, _ts, data, _ds = struct.unpack_from('>4I', rwar, 0x10)
    n = struct.unpack_from('>I', rwar, tabl + 8)[0]
    out = []
    for i in range(n):
        _ref, off, size = struct.unpack_from('>III', rwar, tabl + 12 + 12 * i)
        rwav = rwar[data + off:data + off + size]
        info, _is, wdata, _ws = struct.unpack_from('>4I', rwav, 0x10)
        out.append(parse_wave_info(rwav, info + 8, wdata + 8))
    return out


def parse_rsar(blob):
    """The RSAR's string table, sounds, files and groups (INFO references resolved)."""
    if blob[:4] != b'RSAR':
        raise ValueError('not an RSAR')
    symb, _ss, info, _is, _fo, _fs = struct.unpack_from('>6I', blob, 0x10)
    S = symb + 8
    st = S + struct.unpack_from('>I', blob, S)[0]
    names = []
    for i in range(struct.unpack_from('>I', blob, st)[0]):
        o = S + struct.unpack_from('>I', blob, st + 4 + 4 * i)[0]
        names.append(blob[o:blob.index(b'\0', o)].decode('latin1'))
    I = info + 8

    def ref(o):
        return struct.unpack_from('>I', blob, o + 4)[0]

    def table(o):
        c = struct.unpack_from('>I', blob, I + o)[0]
        return [ref(I + o + 4 + 8 * k) for k in range(c)]

    def nm(i):
        return names[i] if 0 <= i < len(names) else None

    tabs = [ref(I + 8 * k) for k in range(6)]
    sounds = []
    for o in table(tabs[0]):
        sid, fid, pid = struct.unpack_from('>III', blob, I + o)
        sounds.append(dict(name=nm(sid), file=fid, player=nm(pid), type=blob[I + o + 22]))
    files = []
    for o in table(tabs[3]):
        hsize, wsize, entry = struct.unpack_from('>IIi', blob, I + o)
        ext = ref(I + o + 12)
        ext_name = blob[I + ext:blob.index(b'\0', I + ext)].decode('latin1') if ext else None
        files.append(dict(size=hsize, wave_size=wsize, external=ext_name))
    groups = []
    for o in table(tabs[4]):
        sid, entry = struct.unpack_from('>Ii', blob, I + o)
        off, size, woff, wsize = struct.unpack_from('>4I', blob, I + o + 16)
        items = []
        for io in table(ref(I + o + 32)):
            fid, ioff, isize, iwoff, iwsize = struct.unpack_from('>5I', blob, I + io)
            items.append(dict(file=fid, offset=off + ioff, size=isize, wave_offset=woff + iwoff, wave_size=iwsize))
        groups.append(dict(name=nm(sid) if sid != 0xFFFFFFFF else None, items=items))
    return dict(names=names, sounds=sounds, files=files, groups=groups)


# ---------------------------------------------------------------------------
# messages
# ---------------------------------------------------------------------------
def decode_message_text(raw):
    """A message string: 0x10 = space, 0x0A = newline, other control bytes as {xx}."""
    out = []
    for b in raw:
        if b == 0x10:
            out.append(' ')
        elif b == 0x0A:
            out.append('\n')
        elif 0x20 <= b < 0x7F:
            out.append(chr(b))
        else:
            out.append('{%02x}' % b)
    return ''.join(out)


def parse_messages(blob):
    """[[(u16 a, u16 b, text bytes)]] of a mess/*.bin bank: u32 group count, u32 group offset[]
    (relative to offset 4); a group is u32 count, u32 message offset[] (relative to the group +
    4); a message is {u16, u16} then its text, up to the next message (NUL-padded)."""
    ng = struct.unpack_from('>I', blob, 0)[0]
    goffs = [4 + x for x in struct.unpack_from('>%dI' % ng, blob, 4)] + [len(blob)]
    out = []
    for g in range(ng):
        base, end = goffs[g], goffs[g + 1]
        n = struct.unpack_from('>I', blob, base)[0]
        moffs = [base + 4 + x for x in struct.unpack_from('>%dI' % n, blob, base + 4)] + [end]
        msgs = []
        for k in range(n):
            a, b = struct.unpack_from('>HH', blob, moffs[k])
            msgs.append((a, b, blob[moffs[k] + 4:moffs[k + 1]].rstrip(b'\0')))
        out.append(msgs)
    return out


# ---------------------------------------------------------------------------
# main.dol tables
# ---------------------------------------------------------------------------
class Dol:
    def __init__(self, blob):
        h = struct.unpack_from('>64I', blob, 0)
        self.blob = blob
        self.sections = [(h[18 + i], h[i], h[36 + i]) for i in range(18) if h[36 + i]]

    def offset(self, va, size=1):
        for addr, off, sz in self.sections:
            if addr <= va and va + size <= addr + sz:
                return off + va - addr
        return None

    def u32(self, va):
        o = self.offset(va, 4)
        return None if o is None else struct.unpack_from('>I', self.blob, o)[0]

    def cstr(self, va):
        o = self.offset(va)
        if o is None:
            return None
        e = self.blob.find(b'\0', o, o + 512)
        return self.blob[o:e].decode('latin1') if e >= 0 else None

    def find_ptr(self, va):
        """Data addresses holding the pointer `va`."""
        pat = struct.pack('>I', va)
        out = []
        for addr, off, sz in self.sections:
            i = self.blob.find(pat, off, off + sz)
            while i >= 0:
                out.append(addr + i - off)
                i = self.blob.find(pat, i + 1, off + sz)
        return out

    def find_str(self, text):
        b = text.encode() + b'\0'
        for addr, off, sz in self.sections:
            i = self.blob.find(b, off, off + sz)
            if i >= 0:
                return addr + i - off
        return None


def dol_data_dirs(dol):
    """The HuData directory table: [(dir index, path)] (pointer pairs {char *path, handle}
    terminated by a NULL path; the first entry is the string `data/arrow.bin`)."""
    s = dol.find_str('data/arrow.bin')
    if s is None:
        return []
    for t in dol.find_ptr(s):
        out = []
        while True:
            p = dol.u32(t + 8 * len(out))
            if not p:
                break
            name = dol.cstr(p)
            if name is None:
                break
            out.append(name)
        if len(out) > 8:
            return list(enumerate(out))
    return []


def dol_songs(dol):
    """The song list: 16-byte records {u16 category, u16 song (= data/c_000_<song>.bin), u16 a,
    u16 b, u16 bpm_lo, u16 bpm_hi, char *title}, found by the record of `1,2 Step`."""
    s = dol.find_str('1,2 Step')
    if s is None:
        return []
    for p in dol.find_ptr(s):
        start = p - 12
        out = []
        va = start
        while True:
            o = dol.offset(va, 16)
            if o is None:
                break
            cat, song, a, b, lo, hi, title = struct.unpack_from('>6HI', dol.blob, o)
            name = dol.cstr(title) if 0x80000000 <= title < 0x81800000 else None
            if name is None or song == 0:
                break
            out.append(dict(category=cat, song=song, a=a, b=b, bpm_lo=lo, bpm_hi=hi, title=name))
            va += 16
        if out:
            return out
    return []


# ---------------------------------------------------------------------------
# disc images: .wbfs / .iso -> the game's file tree
# ---------------------------------------------------------------------------
WII_COMMON_KEY = bytes.fromhex('ebe42a225e8593e448d9c5457381aaf7')
WII_MAGIC = bytes.fromhex('5d1c9ea3')
CLUSTER = 0x8000
CLUSTER_HASH = 0x400
CLUSTER_DATA = 0x7C00


def _aes_cbc_decrypt(key, iv, data):
    try:
        from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
    except ImportError as e:  # pragma: no cover
        raise RuntimeError('reading a disc image needs the `cryptography` package (pip install cryptography)') from e
    d = Cipher(algorithms.AES(key), modes.CBC(iv)).decryptor()
    return d.update(data) + d.finalize()


class IsoDisc:
    """Raw (encrypted) disc bytes of a plain .iso."""

    def __init__(self, path):
        self.file = open(path, 'rb')

    def read(self, offset, size):
        self.file.seek(offset)
        return self.file.read(size).ljust(size, b'\0')


class WbfsDisc:
    """Raw (encrypted) disc bytes through a .wbfs block map (the first disc slot)."""

    def __init__(self, path):
        self.file = open(path, 'rb')
        magic, _n, sector_shift, block_shift = struct.unpack('>4sIBB', self.file.read(10))
        if magic != b'WBFS':
            raise ValueError('not a WBFS file')
        self.block = 1 << block_shift
        blocks = (143432 * 2) >> (block_shift - 15)
        self.file.seek((1 << sector_shift) + 0x100)
        self.map = struct.unpack('>%dH' % blocks, self.file.read(2 * blocks))

    def read(self, offset, size):
        out = bytearray()
        while size:
            blk, inner = divmod(offset, self.block)
            n = min(size, self.block - inner)
            if blk < len(self.map) and self.map[blk]:
                self.file.seek(self.map[blk] * self.block + inner)
                out += self.file.read(n).ljust(n, b'\0')
            else:
                out += bytes(n)
            offset += n
            size -= n
        return bytes(out)


class DataPartition:
    """The decrypted data partition of a Wii disc (title key from the ticket, AES-CBC per
    0x8000 cluster: IV at cluster + 0x3D0, data from + 0x400)."""

    def __init__(self, disc):
        self.disc = disc
        count, table = struct.unpack('>II', disc.read(0x40000, 8))
        for i in range(count):
            offset, kind = struct.unpack('>II', disc.read((table << 2) + 8 * i, 8))
            if kind == 0:
                break
        else:
            raise ValueError('no data partition')
        start = offset << 2
        ticket = disc.read(start, 0x2A4)
        self.key = _aes_cbc_decrypt(WII_COMMON_KEY, ticket[0x1DC:0x1E4] + bytes(8), ticket[0x1BF:0x1CF])
        self.data = start + (struct.unpack('>I', disc.read(start + 0x2B8, 4))[0] << 2)
        self.cached = (None, None)

    def cluster(self, i):
        if self.cached[0] != i:
            raw = self.disc.read(self.data + i * CLUSTER, CLUSTER)
            self.cached = (i, _aes_cbc_decrypt(self.key, raw[0x3D0:0x3E0], raw[CLUSTER_HASH:]))
        return self.cached[1]

    def chunks(self, offset, size):
        while size:
            i, inner = divmod(offset, CLUSTER_DATA)
            n = min(size, CLUSTER_DATA - inner)
            yield self.cluster(i)[inner:inner + n]
            offset += n
            size -= n

    def read(self, offset, size):
        return b''.join(self.chunks(offset, size))


def dump_disc(image, out_dir, log=print):
    """Write the data partition's file tree (and sys/main.dol) of a .wbfs / .iso to out_dir."""
    with open(image, 'rb') as f:
        head = f.read(4)
    disc = WbfsDisc(image) if head == b'WBFS' else IsoDisc(image)
    part = DataPartition(disc)
    boot = part.read(0, 0x440)
    if boot[0x18:0x1C] != WII_MAGIC:
        raise ValueError('decryption failed (not a Wii disc?)')
    log('Game: %s - %s' % (boot[:6].decode('latin1'), boot[0x20:0x60].rstrip(b'\0').decode('latin1')))
    fst_off, fst_size = struct.unpack('>II', boot[0x424:0x42C])
    fst = part.read(fst_off << 2, fst_size << 2)
    n = struct.unpack('>I', fst[8:12])[0]
    names = fst[12 * n:]
    dol_off = struct.unpack('>I', boot[0x420:0x424])[0] << 2
    dh = struct.unpack('>64I', part.read(dol_off, 0x100))
    dol_size = max(o + s for o, s in zip(dh[0:18], dh[36:54]))
    os.makedirs(os.path.join(out_dir, 'sys'), exist_ok=True)
    with open(os.path.join(out_dir, 'sys', 'main.dol'), 'wb') as f:
        f.write(part.read(dol_off, dol_size))
    dirs = [(n, out_dir)]
    for i in range(1, n):
        while i >= dirs[-1][0]:
            dirs.pop()
        word, a, b = struct.unpack('>III', fst[12 * i:12 * i + 12])
        s = word & 0xFFFFFF
        name = names[s:names.index(b'\0', s)].decode('shift_jis', 'replace')
        path = os.path.join(dirs[-1][1], name)
        if word >> 24:
            os.makedirs(path, exist_ok=True)
            dirs.append((b, path))
        else:
            with open(path, 'wb') as f:
                for c in part.chunks(a << 2, b):
                    f.write(c)
    log('%d entries -> %s' % (n - 1, _tilde(out_dir)))


# ---------------------------------------------------------------------------
# extraction
# ---------------------------------------------------------------------------
def _tilde(path):
    home = os.path.expanduser('~')
    path = os.path.abspath(path)
    return '~' + path[len(home):] if path.startswith(home) else path


def game_root(game_dir):
    """The directory holding data/ and sys/ (a Dolphin `DATA/files` dump nests it)."""
    for cand in (game_dir, os.path.join(game_dir, 'files'), os.path.join(game_dir, 'DATA', 'files')):
        if os.path.isdir(os.path.join(cand, 'data')):
            return cand
    raise SystemExit('no data/ directory under %s' % _tilde(game_dir))


def main_dol(root):
    sysdir = os.path.join(root, 'sys')
    for cand in [os.path.join(sysdir, 'main.dol'), os.path.join(os.path.dirname(root), 'sys', 'main.dol')] + \
            ([os.path.join(sysdir, f) for f in sorted(os.listdir(sysdir)) if f.endswith('.dol')]
             if os.path.isdir(sysdir) else []):
        if os.path.exists(cand):
            return cand
    return None


def write_pictures(kind, raw, stem, stats):
    """The .png sidecars of one member: HSF bitmaps, sprite bitmaps, TPL images."""
    written = []
    if kind == 'hsf':
        import hsf_dump as H   # noqa: E402  (needs numpy)
        model = H.parse_hsf(raw)
        for i, b in enumerate(model['bitmaps']):
            path = '%s.%02d_%s.png' % (stem, i, safe_name(b['name'] or 'bitmap'))
            write_png(path, b['width'], b['height'], H.decode_bitmap(b).tobytes())
            written.append(path)
    elif kind == 'spr':
        spr = parse_sprite(raw)
        for i, b in enumerate(spr['bitmaps']):
            path = '%s.%02d.png' % (stem, i)
            write_png(path, b['width'], b['height'], sprite_bitmap_rgba(raw, b).tobytes())
            written.append(path)
    elif kind == 'tpl':
        for i, img in enumerate(tpl_images(raw)):
            path = '%s.%02d.png' % (stem, i)
            write_png(path, img.shape[1], img.shape[0], img.tobytes())
            written.append(path)
    stats['png'] = stats.get('png', 0) + len(written)
    return written


def extract_pack(path, out_dir, opts, rows, stats, dir_index=None):
    blob = open(path, 'rb').read()
    pack = os.path.splitext(os.path.basename(path))[0]
    dest = os.path.join(out_dir, pack)
    os.makedirs(dest, exist_ok=True)
    for i, (a, b) in enumerate(read_pack(blob)):
        raw, codec = decode_entry(blob, a, b)
        kind = kind_of(raw)
        stem = os.path.join(dest, '%03d' % i)
        member = stem + '.' + kind
        with open(member, 'wb') as f:
            f.write(raw)
        if kind == 'spr':
            with open(stem + '.spr.json', 'w') as f:
                json.dump(parse_sprite(raw), f, indent=1)
        if opts.png and kind in ('hsf', 'spr', 'tpl'):
            try:
                write_pictures(kind, raw, stem, stats)
            except (ValueError, struct.error, IndexError, KeyError) as e:
                stats.setdefault('problems', []).append((member, 'png: %r' % e))
        fid = '' if dir_index is None else '0x%08x' % ((dir_index << 16) | i)
        rows.append(['data/' + os.path.basename(path), i, fid, CODEC_NAMES.get(codec, codec), len(raw), kind,
                     os.path.relpath(member, os.path.dirname(out_dir))])
        stats[kind] = stats.get(kind, 0) + 1


def extract_rsar(path, out_dir, opts, rows, stats, log):
    """Split ddr.brsar into its files (named by group) and, with --wav, decode every wave."""
    blob = open(path, 'rb').read()
    rsar = parse_rsar(blob)
    dest = os.path.join(out_dir, os.path.splitext(os.path.basename(path))[0] + '_brsar')
    os.makedirs(dest, exist_ok=True)
    with open(os.path.join(dest, 'sounds.csv'), 'w', newline='') as f:
        w = csv.writer(f)
        w.writerow(['index', 'name', 'type', 'file', 'player', 'external'])
        for i, s in enumerate(rsar['sounds']):
            ext = rsar['files'][s['file']]['external'] if 0 <= s['file'] < len(rsar['files']) else None
            w.writerow([i, s['name'], {1: 'seq', 2: 'strm', 3: 'wave'}.get(s['type'], s['type']), s['file'],
                        s['player'], ext or ''])
    done = set()
    for g in rsar['groups']:
        gname = safe_name(g['name'] or 'group')
        for it in g['items']:
            if it['file'] in done:
                continue
            done.add(it['file'])
            data = blob[it['offset']:it['offset'] + it['size']]
            waves_area = blob[it['wave_offset']:it['wave_offset'] + it['wave_size']]
            magic = data[:4].decode('latin1', 'replace')
            ext = magic.lower() if magic in ('RWSD', 'RBNK', 'RSEQ', 'RWAR', 'RWAV') else 'bin'
            gdir = os.path.join(dest, gname)
            os.makedirs(gdir, exist_ok=True)
            stem = os.path.join(gdir, 'file%03d' % it['file'])
            with open(stem + '.' + ext, 'wb') as f:
                f.write(data)
            if waves_area:
                with open(stem + '.waves', 'wb') as f:
                    f.write(waves_area)
            waves = []
            try:
                if ext == 'rwsd':
                    waves = rwsd_waves(data, waves_area)
                elif ext == 'rbnk' and waves_area[:4] == b'RWAR':
                    waves = rwar_waves(waves_area)
                elif ext == 'rwar':
                    waves = rwar_waves(data)
            except (ValueError, struct.error, IndexError) as e:
                stats.setdefault('problems', []).append((stem, 'waves: %r' % e))
            for k, wv in enumerate(waves):
                if opts.wav:
                    wave_to_wav('%s.%03d.wav' % (stem, k), wv)
                stats['waves'] = stats.get('waves', 0) + 1
            rows.append(['sound/' + os.path.basename(path), it['file'], g['name'] or '', '', len(data), ext,
                         os.path.relpath(stem + '.' + ext, os.path.dirname(out_dir))])
            stats[ext] = stats.get(ext, 0) + 1
    log('  %s: %d sounds, %d files in %d groups, %d waves' % (
        os.path.basename(path), len(rsar['sounds']), len(done), len(rsar['groups']), stats.get('waves', 0)))


def extract_other(path, rel, out_dir, opts, rows, stats):
    """A non-pack disc file: copied, plus its decoded form."""
    dest = os.path.join(out_dir, rel)
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    shutil.copyfile(path, dest)
    ext = os.path.splitext(path)[1].lower()
    blob = None
    if ext == '.brstm' and opts.wav:
        s = parse_brstm(open(path, 'rb').read())
        wave_to_wav(os.path.splitext(dest)[0] + '.wav',
                    dict(format=s['codec'], rate=s['rate'], samples=s['samples'], adpcm=s['adpcm'], data=s['data']))
        stats['wav'] = stats.get('wav', 0) + 1
    elif ext == '.thp' and opts.mp4 and shutil.which('ffmpeg'):
        subprocess.run(['ffmpeg', '-v', 'error', '-y', '-i', path, '-c:v', 'libx264', '-pix_fmt', 'yuv420p',
                        '-c:a', 'aac', os.path.splitext(dest)[0] + '.mp4'], check=False)
        stats['mp4'] = stats.get('mp4', 0) + 1
    elif ext == '.tpl' and opts.png:
        write_pictures('tpl', open(path, 'rb').read(), os.path.splitext(dest)[0], stats)
    elif ext in ('.arc', '.bnr'):
        blob, wrappers = unwrap(open(path, 'rb').read())
        if blob[:4] == U8_MAGIC:
            unpack_u8(blob, dest + '_unpacked', opts, stats)
    elif rel.replace(os.sep, '/').startswith('mess/') and ext == '.bin':
        try:
            groups = parse_messages(open(path, 'rb').read())
            with open(os.path.splitext(dest)[0] + '.txt', 'w', encoding='utf-8') as f:
                for g, msgs in enumerate(groups):
                    for k, (a, b, m) in enumerate(msgs):
                        f.write('[%d.%d] (%d, %d) %s\n' % (g, k, a, b, decode_message_text(m)))
        except (struct.error, IndexError) as e:
            stats.setdefault('problems', []).append((dest, 'messages: %r' % e))
    rows.append([rel.replace(os.sep, '/'), '', '', '', os.path.getsize(path), ext.lstrip('.'), rel.replace(os.sep, '/')])
    stats['copied'] = stats.get('copied', 0) + 1


def unpack_u8(blob, dest, opts, stats, depth=0):
    for name, data in parse_u8(blob):
        out = os.path.join(dest, name)
        os.makedirs(os.path.dirname(out), exist_ok=True)
        inner, _w = unwrap(data)
        if inner[:4] == U8_MAGIC and depth < 4:
            unpack_u8(inner, out + '_unpacked', opts, stats, depth + 1)
        with open(out, 'wb') as f:
            f.write(data)
        if opts.png and inner[:4] == TPL_MAGIC:
            try:
                write_pictures('tpl', inner, out, stats)
            except (ValueError, struct.error, IndexError, KeyError) as e:
                stats.setdefault('problems', []).append((out, 'png: %r' % e))
        stats['u8_member'] = stats.get('u8_member', 0) + 1


def danceview_clip_bars(rel):
    """The dance viewer's clip table in dll/danceviewDll.rel: the file ids 0x00060001.. of
    data/c_000.bin's dance clips (one u32 each, consecutive), directly followed by one u32 per
    clip giving its length in SSQ measure units (0x1000 = one 4/4 bar). Returns {c_000 index:
    bars} -- the tempo the clip was authored at is frames / bars (120 frames = 120 BPM)."""
    start = rel.find(struct.pack('>II', 0x00060001, 0x00060002))
    if start < 0:
        return {}
    n = 0
    while start + 4 * (n + 1) <= len(rel) and struct.unpack_from('>I', rel, start + 4 * n)[0] == 0x00060001 + n:
        n += 1
    vals = struct.unpack_from('>%dI' % n, rel, start + 4 * n)
    return {k + 1: vals[k] / 4096.0 for k in range(n)}


def write_dol_tables(dol_path, out_dir):
    dol = Dol(open(dol_path, 'rb').read())
    os.makedirs(os.path.join(out_dir, 'dol'), exist_ok=True)
    dirs = dol_data_dirs(dol)
    with open(os.path.join(out_dir, 'dol', 'data_dirs.csv'), 'w', newline='') as f:
        w = csv.writer(f)
        w.writerow(['dir', 'file_id', 'path'])
        for i, p in dirs:
            w.writerow([i, '0x%08x' % (i << 16), p])
    songs = dol_songs(dol)
    with open(os.path.join(out_dir, 'dol', 'songs.csv'), 'w', newline='') as f:
        w = csv.writer(f)
        w.writerow(['song', 'bundle', 'category', 'a', 'b', 'bpm_lo', 'bpm_hi', 'title'])
        for s in songs:
            w.writerow([s['song'], 'data/c_000_%02d.bin' % s['song'], s['category'], s['a'], s['b'], s['bpm_lo'],
                        s['bpm_hi'], s['title']])
    return dirs, songs


def write_rel_tables(root, out_dir):
    rel = os.path.join(root, 'dll', 'danceviewDll.rel')
    if not os.path.exists(rel):
        return {}
    bars = danceview_clip_bars(open(rel, 'rb').read())
    os.makedirs(os.path.join(out_dir, 'dol'), exist_ok=True)
    with open(os.path.join(out_dir, 'dol', 'dance_clip_bars.csv'), 'w', newline='') as f:
        w = csv.writer(f)
        w.writerow(['c_000_index', 'file_id', 'bars'])
        for k, b in sorted(bars.items()):
            w.writerow([k, '0x%08x' % (0x00060000 | k), b])
    return bars


def cmd_extract(args):
    root = game_root(args.game_dir)
    out = args.out_dir
    os.makedirs(out, exist_ok=True)
    only = set(args.only.split(',')) if args.only else None
    globs = [g.strip() for g in args.files.split(',')] if args.files else None
    rows, stats = [], {}
    dol_path = main_dol(root)
    dir_of = {}
    if dol_path and (only is None or 'dol' in only):
        dirs, songs = write_dol_tables(dol_path, out)
        dir_of = {os.path.basename(p): i for i, p in dirs}
        print('main.dol: %d data directories, %d songs; danceviewDll.rel: %d clip lengths' % (
            len(dirs), len(songs), len(write_rel_tables(root, out))))
    for dirpath, dnames, fnames in os.walk(root):
        dnames.sort()
        for fn in sorted(fnames):
            if fn.startswith('.'):
                continue
            path = os.path.join(dirpath, fn)
            rel = os.path.relpath(path, root)
            top = rel.split(os.sep)[0] if os.sep in rel else ''
            if only is not None and (top or 'root') not in only:
                continue
            if globs and not any(fnmatch.fnmatch(fn, g) for g in globs):
                continue
            try:
                if top == 'data' and fn.endswith('.bin') and is_pack(open(path, 'rb').read()):
                    extract_pack(path, os.path.join(out, 'data'), args, rows, stats, dir_of.get('data/' + fn))
                    print('  %s' % rel)
                elif fn.endswith('.brsar'):
                    extract_rsar(path, os.path.join(out, 'sound'), args, rows, stats, print)
                else:
                    extract_other(path, rel, out, args, rows, stats)
            except (PackError, ValueError, struct.error, IndexError, zlib.error) as e:
                stats.setdefault('problems', []).append((rel, repr(e)))
    with open(os.path.join(out, 'manifest.csv'), 'w', newline='') as f:
        w = csv.writer(f)
        w.writerow(['source', 'index', 'file_id', 'codec', 'size', 'kind', 'path'])
        w.writerows(rows)
    problems = stats.pop('problems', [])
    for p, e in problems:
        print('PROBLEM', p, e)
    print('done: %s -> %s' % (', '.join('%s %d' % kv for kv in sorted(stats.items())), _tilde(out)))
    return 1 if problems else 0


def cmd_disc(args):
    dump_disc(args.image, args.out_dir)


def cmd_pack(args):
    rows, stats = [], {}
    extract_pack(args.file, args.out_dir, args, rows, stats)
    print(', '.join('%s %d' % kv for kv in sorted(stats.items()) if kv[0] != 'problems'))


def cmd_png(args):
    raw = open(args.file, 'rb').read()
    kind = kind_of(raw)
    os.makedirs(args.out_dir, exist_ok=True)
    stats = {}
    for p in write_pictures(kind, raw, os.path.join(args.out_dir, os.path.splitext(os.path.basename(args.file))[0]),
                            stats):
        print(_tilde(p))


def cmd_wav(args):
    s = parse_brstm(open(args.file, 'rb').read())
    wave_to_wav(args.out, dict(format=s['codec'], rate=s['rate'], samples=s['samples'], adpcm=s['adpcm'],
                               data=s['data']))


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0],
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest='cmd', required=True)
    p = sub.add_parser('extract', help='extract the whole game')
    p.add_argument('game_dir')
    p.add_argument('out_dir')
    p.add_argument('--png', action='store_true', help='decode every picture to PNG (needs numpy)')
    p.add_argument('--wav', action='store_true', help='decode every BRSTM / RSAR wave to WAV')
    p.add_argument('--mp4', action='store_true', help='convert the THP movies with ffmpeg')
    p.add_argument('--only', help='top-level directories to process (data,sound,movie,mess,dll,home,sys,root,dol)')
    p.add_argument('--files', help='file name globs to process (e.g. "stg*.bin,c_0*.bin")')
    p.set_defaults(fn=cmd_extract)
    p = sub.add_parser('disc', help='dump the file tree of a .wbfs / .iso')
    p.add_argument('image')
    p.add_argument('out_dir')
    p.set_defaults(fn=cmd_disc)
    p = sub.add_parser('pack', help='split one data pack')
    p.add_argument('file')
    p.add_argument('out_dir')
    p.add_argument('--png', action='store_true')
    p.set_defaults(fn=cmd_pack)
    p = sub.add_parser('png', help='pictures of one .hsf / .spr / .tpl')
    p.add_argument('file')
    p.add_argument('out_dir')
    p.set_defaults(fn=cmd_png)
    p = sub.add_parser('wav', help='decode one .brstm')
    p.add_argument('file')
    p.add_argument('out')
    p.set_defaults(fn=cmd_wav)
    a = ap.parse_args(argv)
    return a.fn(a) or 0


if __name__ == '__main__':
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    sys.exit(main())
