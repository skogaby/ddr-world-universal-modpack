#!/usr/bin/env python3
"""Dump the game files out of a Wii .wbfs image (e.g. DDR Hottest Party).

Usage: python3 wbfs_extract.py <game.wbfs> [output_dir]
Requires: pip install cryptography
"""
import struct
import sys
from pathlib import Path

from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes

WII_COMMON_KEY = bytes.fromhex("ebe42a225e8593e448d9c5457381aaf7")
CLUSTER_SIZE = 0x8000   # encrypted cluster on disc
HASH_SIZE = 0x400       # hash block at the start of each cluster
DATA_SIZE = 0x7C00      # usable data per cluster


def aes_cbc_decrypt(key, iv, data):
    decryptor = Cipher(algorithms.AES(key), modes.CBC(iv)).decryptor()
    return decryptor.update(data) + decryptor.finalize()


class WbfsDisc:
    """Reads raw (encrypted) Wii disc bytes through the WBFS block map."""

    def __init__(self, path):
        self.file = open(path, "rb")
        magic, _, sector_shift, block_shift = struct.unpack(">4sIBB", self.file.read(10))
        if magic != b"WBFS":
            sys.exit("Not a WBFS file")
        self.block_size = 1 << block_shift
        blocks_per_disc = (143432 * 2) >> (block_shift - 15)
        self.file.seek((1 << sector_shift) + 0x100)
        self.block_map = struct.unpack(f">{blocks_per_disc}H", self.file.read(blocks_per_disc * 2))

    def read(self, offset, size):
        out = bytearray()
        while size:
            block, block_offset = divmod(offset, self.block_size)
            chunk = min(size, self.block_size - block_offset)
            if self.block_map[block]:
                self.file.seek(self.block_map[block] * self.block_size + block_offset)
                out += self.file.read(chunk)
            else:
                out += bytes(chunk)
            offset += chunk
            size -= chunk
        return bytes(out)


class DataPartition:
    """Decrypted view of the game's data partition."""

    def __init__(self, disc):
        self.disc = disc
        count, table = struct.unpack(">II", disc.read(0x40000, 8))
        for i in range(count):
            offset, kind = struct.unpack(">II", disc.read((table << 2) + i * 8, 8))
            if kind == 0:
                break
        else:
            sys.exit("No data partition found")
        start = offset << 2
        ticket = disc.read(start, 0x2A4)
        title_iv = ticket[0x1DC:0x1E4] + bytes(8)
        self.title_key = aes_cbc_decrypt(WII_COMMON_KEY, title_iv, ticket[0x1BF:0x1CF])
        self.data_start = start + (struct.unpack(">I", disc.read(start + 0x2B8, 4))[0] << 2)
        self.cached_index, self.cached_cluster = None, None

    def cluster(self, index):
        if index != self.cached_index:
            raw = self.disc.read(self.data_start + index * CLUSTER_SIZE, CLUSTER_SIZE)
            self.cached_cluster = aes_cbc_decrypt(self.title_key, raw[0x3D0:0x3E0], raw[HASH_SIZE:])
            self.cached_index = index
        return self.cached_cluster

    def chunks(self, offset, size):
        while size:
            index, cluster_offset = divmod(offset, DATA_SIZE)
            chunk = min(size, DATA_SIZE - cluster_offset)
            yield self.cluster(index)[cluster_offset:cluster_offset + chunk]
            offset += chunk
            size -= chunk

    def read(self, offset, size):
        return b"".join(self.chunks(offset, size))


def extract(wbfs_path, out_dir):
    part = DataPartition(WbfsDisc(wbfs_path))
    boot = part.read(0, 0x440)
    if boot[0x18:0x1C] != bytes.fromhex("5d1c9ea3"):
        sys.exit("Decryption failed (bad key?)")
    print(f"Game: {boot[:6].decode()} - {boot[0x20:0x60].rstrip(b'\0').decode()}")

    fst_offset, fst_size = struct.unpack(">II", boot[0x424:0x42C])
    fst = part.read(fst_offset << 2, fst_size << 2)
    entry_count = struct.unpack(">I", fst[8:12])[0]
    names = fst[entry_count * 12:]

    # main.dol lives outside the FST; its size is the end of its furthest section
    dol_offset = struct.unpack(">I", boot[0x420:0x424])[0] << 2
    dol_header = struct.unpack(">64I", part.read(dol_offset, 0x100))
    section_offsets, section_sizes = dol_header[0:18], dol_header[36:54]
    dol_size = max(o + s for o, s in zip(section_offsets, section_sizes))
    (out_dir / "sys").mkdir(parents=True, exist_ok=True)
    (out_dir / "sys" / "main.dol").write_bytes(part.read(dol_offset, dol_size))

    dirs = [(entry_count, out_dir)]  # (index where this dir ends, path)
    for i in range(1, entry_count):
        while i >= dirs[-1][0]:
            dirs.pop()
        type_and_name, a, b = struct.unpack(">III", fst[i * 12:i * 12 + 12])
        name_start = type_and_name & 0xFFFFFF
        name = names[name_start:names.index(b"\0", name_start)].decode("shift_jis", "replace")
        path = dirs[-1][1] / name
        if type_and_name >> 24:  # directory; b = index after its last entry
            path.mkdir(exist_ok=True)
            dirs.append((b, path))
        else:  # file; a = offset >> 2, b = size
            with open(path, "wb") as f:
                for chunk in part.chunks(a << 2, b):
                    f.write(chunk)
        print(f"\r{i}/{entry_count - 1} entries", end="", flush=True)
    print(f"\nDone -> {out_dir}")


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    src = Path(sys.argv[1])
    extract(src, Path(sys.argv[2]) if len(sys.argv) > 2 else src.with_suffix(""))
