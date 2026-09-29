#!/usr/bin/env python3
"""Decode Konami System 573 background movies (`.SBS`, MAX / MAX 2 / EXTREME, plus the
`data/movie/*/*.sbs` clips inside the flash images) to MP4, and contact-sheet them.

An .SBS is a headerless run of PlayStation MDEC "BS" v2 frames, one per fixed slot
(8 KiB in every shipped clip; detected per file from the frame-header spacing):
    frame  = { u16 mdec_words, u16 0x3800, u16 qscale, u16 version=2 } + bitstream
    stream = 16-bit little-endian words read MSB-first; per macroblock six 8x8 blocks
             Cr, Cb, Y0, Y1, Y2, Y3; macroblocks stored column-major (top to bottom, then
             left to right). Block: 10-bit signed DC (x the intra quant matrix's 2), then
             MPEG-1 table B.14 AC run/level codes (EOB '10', escape '000001' + 6-bit run +
             10-bit signed level), dequantised as (level * q[i] * qscale + 4) >> 3.
Every clip decodes to 209 macroblocks = 304x176 (19 x 11). ffmpeg has no demuxer for
headerless BS, hence this decoder (numpy IDCT, not bit-exact to the MDEC's fixed point).
Clips are 80 frames; the playback rate is not stored — 30 fps (the default here) looked
right for looped dance clips but is unverified against the game.

Usage:
    sys573_video.py mp4   <out dir> <file.sbs>... [--fps 30] [--scale 2]
    sys573_video.py sheet <out.png> <file.sbs>... [--frame 40]      # one labelled frame each
    sys573_video.py frame <file.sbs> <index> <out.png>

Needs numpy; Pillow for PNG output; ffmpeg on PATH for mp4.
"""
import argparse
import os
import struct
import subprocess
import sys

import numpy as np

# MPEG-1 table B.14 (the sign bit follows each code).
_VLC_SRC = """
11 0 1|011 1 1|0100 0 2|0101 2 1|00101 0 3|00111 3 1|00110 4 1|000110 1 2|000111 5 1
000101 6 1|000100 7 1|0000110 0 4|0000100 2 2|0000111 8 1|0000101 9 1|00100110 0 5
00100001 0 6|00100101 1 3|00100100 3 2|00100111 10 1|00100011 11 1|00100010 12 1
00100000 13 1|0000001010 0 7|0000001100 1 4|0000001011 2 3|0000001111 4 2
0000001001 5 2|0000001110 14 1|0000001101 15 1|0000001000 16 1|000000011101 0 8
000000011000 0 9|000000010011 0 10|000000010000 0 11|000000011011 1 5|000000010100 2 4
000000011100 3 3|000000010010 4 3|000000011110 6 2|000000010101 7 2|000000010001 8 2
000000011111 17 1|000000011010 18 1|000000011001 19 1|000000010111 20 1|000000010110 21 1
0000000011010 0 12|0000000011001 0 13|0000000011000 0 14|0000000010111 0 15
0000000010110 1 6|0000000010101 1 7|0000000010100 2 5|0000000010011 3 4|0000000010010 5 3
0000000010001 9 2|0000000010000 10 2|0000000011111 22 1|0000000011110 23 1
0000000011101 24 1|0000000011100 25 1|0000000011011 26 1|00000000011111 0 16
00000000011110 0 17|00000000011101 0 18|00000000011100 0 19|00000000011011 0 20
00000000011010 0 21|00000000011001 0 22|00000000011000 0 23|00000000010111 0 24
00000000010110 0 25|00000000010101 0 26|00000000010100 0 27|00000000010011 0 28
00000000010010 0 29|00000000010001 0 30|00000000010000 0 31|000000000011000 0 32
000000000010111 0 33|000000000010110 0 34|000000000010101 0 35|000000000010100 0 36
000000000010011 0 37|000000000010010 0 38|000000000010001 0 39|000000000010000 0 40
000000000011111 1 8|000000000011110 1 9|000000000011101 1 10|000000000011100 1 11
000000000011011 1 12|000000000011010 1 13|000000000011001 1 14|0000000000010011 1 15
0000000000010010 1 16|0000000000010001 1 17|0000000000010000 1 18|0000000000010100 6 3
0000000000011010 11 2|0000000000011001 12 2|0000000000011000 13 2|0000000000010111 14 2
0000000000010110 15 2|0000000000010101 16 2|0000000000011111 27 1|0000000000011110 28 1
0000000000011101 29 1|0000000000011100 30 1|0000000000011011 31 1
"""
VLC = {}
for _ent in _VLC_SRC.replace('\n', '|').split('|'):
    if _ent.strip():
        _code, _run, _lvl = _ent.split()
        VLC[_code] = (int(_run), int(_lvl))

ZIGZAG = [0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48,
          41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22,
          15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55,
          62, 63]
# PSX default intra quantisation matrix (natural order) = MPEG-1's default.
QUANT = np.array([
    2, 16, 19, 22, 26, 27, 29, 34, 16, 16, 22, 24, 27, 29, 34, 37,
    19, 22, 26, 27, 29, 34, 34, 38, 22, 22, 26, 27, 29, 34, 37, 40,
    22, 26, 27, 29, 32, 35, 40, 48, 26, 27, 29, 32, 35, 40, 48, 58,
    26, 27, 29, 34, 38, 46, 56, 69, 27, 29, 35, 38, 46, 56, 69, 83])
_K = np.arange(8)
_IDCT = np.cos((2 * _K[None, :] + 1) * _K[:, None] * np.pi / 16) * np.where(_K[:, None] == 0, np.sqrt(1 / 8), np.sqrt(2 / 8))
# frame sizes by macroblock count (w, h); 209 is the only one seen in shipped 573 clips
SIZES = {209: (304, 176), 300: (320, 240), 240: (320, 192), 280: (320, 224), 220: (320, 176)}
FRAME_MAGIC = 0x3800


class _Bits:
    def __init__(self, data):
        n = len(data) // 2
        self.s = ''.join(format(w, '016b') for w in struct.unpack('<%dH' % n, data[:2 * n]))
        self.p = 0

    def read(self, n):
        v = self.s[self.p:self.p + n]
        if len(v) < n:
            raise EOFError
        self.p += n
        return v


def _signed(bits):
    v = int(bits, 2)
    return v - (1 << len(bits)) if bits[0] == '1' else v


def _block(b, qscale):
    coef = np.zeros(64)
    coef[0] = _signed(b.read(10)) * QUANT[0]
    i = 0
    code = ''
    while True:
        code += b.read(1)
        if code == '10':
            break
        if code == '000001':
            run, lvl = int(b.read(6), 2), _signed(b.read(10))
        elif code in VLC:
            run, lvl = VLC[code]
            if b.read(1) == '1':
                lvl = -lvl
        elif len(code) > 16:
            raise ValueError('bad VLC')
        else:
            continue
        code = ''
        i += run + 1
        if i > 63:
            raise ValueError('coefficient overflow')
        k = ZIGZAG[i]
        mag = (abs(lvl) * QUANT[k] * qscale + 4) >> 3
        coef[k] = mag if lvl >= 0 else -mag
    return _IDCT.T @ coef.reshape(8, 8) @ _IDCT


def decode_frame(frame):
    """One BS v2 frame -> list of macroblocks (cr 8x8, cb 8x8, y 16x16), stream order."""
    _words, magic, qscale, ver = struct.unpack_from('<HHHH', frame, 0)
    if magic != FRAME_MAGIC or ver not in (2,):
        raise ValueError('not a BS v2 frame (magic %04x version %d)' % (magic, ver))
    b = _Bits(frame[8:])
    mbs = []
    while True:
        start = b.p
        if b.s[start:start + 32] == '0' * 32:  # slot padding
            break
        try:
            cr, cb, y0, y1, y2, y3 = [_block(b, qscale) for _ in range(6)]
        except (EOFError, ValueError):
            break
        mbs.append((cr, cb, np.block([[y0, y1], [y2, y3]])))
    return mbs


def to_rgb(mbs, size=None):
    w, h = size or SIZES.get(len(mbs), (16 * max(1, len(mbs) // 11), 176))
    rows = h // 16
    img = np.zeros((h, w, 3))
    up = np.ones((2, 2))
    for i, (cr, cb, y) in enumerate(mbs[:(w // 16) * rows]):
        mx, my = divmod(i, rows)
        crf, cbf = np.kron(cr, up), np.kron(cb, up)
        img[my * 16:my * 16 + 16, mx * 16:mx * 16 + 16] = np.stack(
            [y + 1.402 * crf, y - 0.3437 * cbf - 0.7143 * crf, y + 1.772 * cbf], -1)
    return np.clip(img + 128, 0, 255).astype(np.uint8)


def frame_slots(data):
    """Offsets of every frame; the slot size is the header spacing (a multiple of 0x800)."""
    heads = [o for o in range(0, len(data) - 8, 0x800)
             if struct.unpack_from('<H', data, o + 2)[0] == FRAME_MAGIC and data[o + 6] == 2 and data[o + 7] == 0]
    return heads


def decode_file(path):
    data = open(path, 'rb').read()
    for o in frame_slots(data):
        yield to_rgb(decode_frame(data[o:o + 0x10000]))


def _tilde(p):
    home = os.path.expanduser('~')
    return '~' + p[len(home):] if p.startswith(home) else p


def cmd_mp4(a):
    os.makedirs(a.out_dir, exist_ok=True)
    for src in a.files:
        frames = list(decode_file(src))
        if not frames:
            print('skip %s: no BS frames' % _tilde(src))
            continue
        h, w = frames[0].shape[:2]
        dst = os.path.join(a.out_dir, os.path.splitext(os.path.basename(src))[0] + '.mp4')
        proc = subprocess.Popen(['ffmpeg', '-y', '-loglevel', 'error', '-f', 'rawvideo', '-pix_fmt', 'rgb24',
                                 '-s', '%dx%d' % (w, h), '-r', str(a.fps), '-i', '-', '-vf',
                                 'scale=iw*%d:ih*%d:flags=lanczos' % (a.scale, a.scale), '-c:v', 'libx264',
                                 '-pix_fmt', 'yuv420p', '-crf', '16', dst], stdin=subprocess.PIPE)
        assert proc.stdin is not None
        for f in frames:
            proc.stdin.write(f.tobytes())
        proc.stdin.close()
        proc.wait()
        print('%s: %d frames -> %s' % (_tilde(src), len(frames), _tilde(dst)))


def cmd_sheet(a):
    from PIL import Image, ImageDraw
    tiles = []
    for src in a.files:
        data = open(src, 'rb').read()
        slots = frame_slots(data)
        if not slots:
            continue
        o = slots[min(a.frame, len(slots) - 1)]
        tiles.append((os.path.basename(src), to_rgb(decode_frame(data[o:o + 0x10000]))[::2, ::2]))
    cols, tw, th = 16, 152, 88
    sheet = Image.new('RGB', (cols * tw, ((len(tiles) + cols - 1) // cols) * (th + 12)))
    draw = ImageDraw.Draw(sheet)
    for i, (name, img) in enumerate(tiles):
        r, c = divmod(i, cols)
        sheet.paste(Image.fromarray(np.ascontiguousarray(img[:th, :tw])), (c * tw, r * (th + 12)))
        draw.text((c * tw + 2, r * (th + 12) + th), name, fill=(255, 255, 0))
    sheet.save(a.out)
    print('%d clips -> %s' % (len(tiles), _tilde(a.out)))


def cmd_frame(a):
    from PIL import Image
    data = open(a.file, 'rb').read()
    o = frame_slots(data)[a.index]
    Image.fromarray(to_rgb(decode_frame(data[o:o + 0x10000]))).save(a.out)


def main(argv):
    ap = argparse.ArgumentParser(description='System 573 .SBS (PSX MDEC BS v2) movie decoder.')
    sub = ap.add_subparsers(dest='cmd', required=True)
    p = sub.add_parser('mp4')
    p.add_argument('out_dir')
    p.add_argument('files', nargs='+')
    p.add_argument('--fps', type=float, default=30.0)
    p.add_argument('--scale', type=int, default=2)
    p = sub.add_parser('sheet')
    p.add_argument('out')
    p.add_argument('files', nargs='+')
    p.add_argument('--frame', type=int, default=40)
    p = sub.add_parser('frame')
    p.add_argument('file')
    p.add_argument('index', type=int)
    p.add_argument('out')
    a = ap.parse_args(argv)
    dict(mp4=cmd_mp4, sheet=cmd_sheet, frame=cmd_frame)[a.cmd](a)


if __name__ == '__main__':
    main(sys.argv[1:])
