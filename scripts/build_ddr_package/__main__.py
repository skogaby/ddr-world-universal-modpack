#!/usr/bin/env python3
"""
build_ddr_package — Build DDR World texture packs from PNG images.

Takes a directory of PNG images and produces an ARC file containing an IFS
texture pack that can be loaded by the game's BM2D texture system.

Usage:
    python -m build_ddr_package path/to/pngs -o output.arc
    python -m build_ddr_package path/to/pngs -o output.arc --name my_mod

Requires: Pillow, kbinxml, lxml, ifstools
"""

import argparse
import hashlib
import math
import os
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image


def next_power_of_2(n):
    """Round up to next power of 2 (minimum 256 for game compatibility)."""
    n = max(n, 256)
    return 1 << (n - 1).bit_length()


def collect_images(input_dir):
    """Collect all PNG images from a directory recursively."""
    images = []
    for png_file in sorted(Path(input_dir).rglob('*.png')):
        name = png_file.stem
        img = Image.open(png_file)
        if img.mode != 'RGBA':
            img = img.convert('RGBA')
        images.append((name, img))
        print(f'  {name}: {img.size[0]}x{img.size[1]}')
    return images


def build_ifs_dir(images, output_dir):
    """Create an ifstools-compatible directory structure."""
    tex_dir = os.path.join(output_dir, 'tex')
    os.makedirs(tex_dir, exist_ok=True)

    # Save images as PNGs
    for name, img in images:
        img.save(os.path.join(tex_dir, f'{name}.png'))

    # Build texturelist.xml — one texture atlas per image
    lines = ['<?xml version=\'1.0\' encoding=\'UTF-8\'?>']
    lines.append('<texturelist compress="avslz">')
    for i, (name, img) in enumerate(images):
        w, h = img.size
        atlas_w = next_power_of_2(w)
        atlas_h = next_power_of_2(h)
        lines.append(f'  <texture format="argb8888rev" mag_filter="linear" min_filter="linear" name="tex{i:03d}" wrap_s="clamp" wrap_t="clamp">')
        lines.append(f'    <size __type="2u16">{atlas_w} {atlas_h}</size>')
        lines.append(f'    <image name="{name}">')
        lines.append(f'      <uvrect __type="4u16">{0} {w*2} {0} {h*2}</uvrect>')
        lines.append(f'      <imgrect __type="4u16">{0} {w*2} {0} {h*2}</imgrect>')
        lines.append(f'    </image>')
        lines.append(f'  </texture>')
    lines.append('</texturelist>')

    with open(os.path.join(tex_dir, 'texturelist.xml'), 'w') as f:
        f.write('\n'.join(lines) + '\n')

    # magic
    with open(os.path.join(output_dir, 'magic'), 'wb') as f:
        f.write(b'NGPF')

    # version.xml
    with open(os.path.join(output_dir, 'version.xml'), 'w') as f:
        f.write('<?xml version=\'1.0\' encoding=\'UTF-8\'?>\n')
        f.write('<version>\n')
        f.write('  <converter __type="str">1.3.80</converter>\n')
        f.write('  <package __type="str">1.0.1</package>\n')
        f.write('  <afp __type="str">1.0.0</afp>\n')
        f.write('  <afpstr __type="str">1.1.0</afpstr>\n')
        f.write('  <cplatform __type="str">linux</cplatform>\n')
        f.write('</version>\n')


def build_arc(ifs_data, ifs_path='data/bm2d/custom_mod.ifs'):
    """Wrap IFS data in a minimal ARC file."""
    path_bytes = ifs_path.encode('utf-8') + b'\x00'
    header_size = 16
    cue_size = 16
    path_start = header_size + cue_size
    data_start = path_start + len(path_bytes)
    if data_start % 0x20 != 0:
        data_start += 0x20 - (data_start % 0x20)

    header = struct.pack('<IIII', 0x19751120, 1, 1, 2)
    cue = struct.pack('<IIII', path_start, data_start, len(ifs_data), len(ifs_data))
    padding = b'\x00' * (data_start - header_size - cue_size - len(path_bytes))

    return header + cue + path_bytes + padding + ifs_data


def main():
    parser = argparse.ArgumentParser(
        description='Build DDR World texture packs from PNG images',
    )
    parser.add_argument('input_dir', help='Directory containing PNG images')
    parser.add_argument('-o', '--output', required=True, help='Output ARC file path')
    parser.add_argument('--name', default='custom_mod',
                        help='Package name (default: custom_mod)')
    args = parser.parse_args()

    if not os.path.isdir(args.input_dir):
        print(f'Error: {args.input_dir} is not a directory')
        return 1

    print(f'Collecting images from {args.input_dir}...')
    images = collect_images(args.input_dir)
    if not images:
        print('Error: No PNG images found')
        return 1

    with tempfile.TemporaryDirectory() as tmpdir:
        # Create ifstools-compatible directory
        ifs_dir = os.path.join(tmpdir, f'{args.name}_ifs')
        print(f'\nBuilding IFS structure...')
        build_ifs_dir(images, ifs_dir)

        # Pack with ifstools
        print('Packing IFS with ifstools...')
        result = subprocess.run(
            ['ifstools', ifs_dir, '-y'],
            cwd=tmpdir, capture_output=True, text=True,
        )
        if result.returncode != 0:
            print(f'ifstools failed:\n{result.stderr}')
            return 1

        ifs_path = os.path.join(tmpdir, f'{args.name}.ifs')
        if not os.path.exists(ifs_path):
            print(f'Error: ifstools did not produce {ifs_path}')
            return 1

        with open(ifs_path, 'rb') as f:
            ifs_data = f.read()
        print(f'IFS: {len(ifs_data)} bytes')

        # Wrap in ARC
        arc_ifs_path = f'data/bm2d/{args.name}.ifs'
        arc_data = build_arc(ifs_data, arc_ifs_path)

        output_path = Path(args.output)
        output_path.parent.mkdir(parents=True, exist_ok=True)
        with open(output_path, 'wb') as f:
            f.write(arc_data)
        print(f'ARC: {output_path} ({len(arc_data)} bytes)')

    print('Done!')
    return 0


if __name__ == '__main__':
    sys.exit(main())
