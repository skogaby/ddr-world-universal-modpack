#!/usr/bin/env python3
"""
ARC File Unpacker for DDR A Archives

This script can list and extract files from DDR A ARC archives with automatic
Konami Lz77 decompression support.

ARC File Format:
- Magic number: 0x19751120
- Header: version (4 bytes), file_count (4 bytes), compression (4 bytes)
- Cue entries: array of file entries (16 bytes each)
  - path_offset: offset to null-terminated filename
  - data_offset: offset to file data
  - decompressed_size: size after decompression
  - compressed_size: size of compressed data
- File data: compressed (Konami Lz77) or uncompressed file contents

Features:
- Automatic detection and decompression of Konami Lz77 compressed files
- Fallback to raw compressed data if decompression fails
- Size verification for decompressed content

Usage:
    python3 unpack_arc.py file.arc -l                    # List files
    python3 unpack_arc.py file.arc                       # Extract all files
    python3 unpack_arc.py file.arc -f specific_file.txt  # Extract single file
    python3 unpack_arc.py file.arc -o output_dir         # Extract to directory
"""

import struct
import sys
import os
import argparse
from pathlib import Path
from typing import Dict, List, Optional, Tuple


class ARCError(Exception):
    """Base exception for ARC file operations"""
    pass


class KonamiLz77:
    """
    Konami's specific implementation of Lz77 compression/decompression.
    
    This is the compression scheme used for network communications and file formats
    in Konami games. Ported from Kotlin implementation.
    """
    
    LZ_WINDOW_SIZE = 0x1000
    LZ_WINDOW_MASK = LZ_WINDOW_SIZE - 1
    LZ_THRESHOLD = 3
    
    @classmethod
    def decompress(cls, input_data: bytes) -> bytes:
        """
        Decompresses the given input data using the Lz77 decompression algorithm.
        
        Args:
            input_data: The compressed data to decompress
            
        Returns:
            The decompressed data
            
        Raises:
            ValueError: If input data is invalid or corrupted
        """
        if not input_data:
            return b''
            
        curr_byte = 0
        window_cursor = 0
        data_size = len(input_data)
        window = bytearray(cls.LZ_WINDOW_SIZE)
        output = bytearray()
        
        while curr_byte < data_size:
            flag = input_data[curr_byte]
            curr_byte += 1
            
            for i in range(8):
                if curr_byte >= data_size:
                    break
                    
                if (flag >> i) & 1 == 1:
                    # Uncompressed byte
                    if curr_byte >= data_size:
                        raise ValueError("Unexpected end of data while reading uncompressed byte")
                    
                    output.append(input_data[curr_byte])
                    window[window_cursor] = input_data[curr_byte]
                    window_cursor = (window_cursor + 1) & cls.LZ_WINDOW_MASK
                    curr_byte += 1
                else:
                    # Compressed sequence
                    if curr_byte + 1 >= data_size:
                        break
                        
                    w = (input_data[curr_byte] << 8) | (input_data[curr_byte + 1] & 0xFF)
                    if w == 0:
                        return bytes(output)
                    
                    curr_byte += 2
                    position = (window_cursor - (w >> 4)) & cls.LZ_WINDOW_MASK
                    length = (w & 0x0F) + cls.LZ_THRESHOLD
                    
                    # Validate length to prevent excessive memory usage
                    if length > 0x10000:  # Reasonable upper bound
                        raise ValueError(f"Invalid compression length: {length}")
                    
                    for j in range(length):
                        b = window[position & cls.LZ_WINDOW_MASK]
                        output.append(b)
                        window[window_cursor] = b
                        window_cursor = (window_cursor + 1) & cls.LZ_WINDOW_MASK
                        position += 1
        
        return bytes(output)


class CueEntry:
    """Represents a file entry in the ARC archive"""
    
    def __init__(self, path_offset: int, data_offset: int, 
                 decompressed_size: int, compressed_size: int):
        self.path_offset = path_offset
        self.data_offset = data_offset
        self.decompressed_size = decompressed_size
        self.compressed_size = compressed_size
    
    @classmethod
    def parse(cls, data: bytes) -> 'CueEntry':
        """Parse a cue entry from 16 bytes of data"""
        if len(data) < 16:
            raise ARCError(f"Insufficient data for cue entry: {len(data)} bytes")
        
        path_offset, data_offset, decompressed_size, compressed_size = struct.unpack('<IIII', data[:16])
        return cls(path_offset, data_offset, decompressed_size, compressed_size)
    
    def parse_path(self, arc_data: bytes) -> str:
        """Extract the null-terminated path string from the archive data"""
        if self.path_offset >= len(arc_data):
            raise ARCError(f"Path offset {self.path_offset} exceeds data length {len(arc_data)}")
        
        # Find the null terminator
        path_data = arc_data[self.path_offset:]
        null_pos = path_data.find(b'\x00')
        if null_pos == -1:
            # No null terminator found, use rest of data
            path_bytes = path_data
        else:
            path_bytes = path_data[:null_pos]
        
        return path_bytes.decode('utf-8', errors='replace')


class ARC:
    """DDR A ARC archive parser and extractor"""
    
    MAGIC = 0x19751120
    
    def __init__(self, data: bytes, decompress: bool = True):
        self.data = data
        self.file_count = 0
        self.version = 0
        self.cue: Dict[str, CueEntry] = {}
        self.decompress = decompress
        self._parse()
    
    def _parse(self):
        """Parse the ARC file header and cue entries"""
        if len(self.data) < 16:
            raise ARCError("File too small to be a valid ARC archive")
        
        # Parse header
        magic, self.version, self.file_count, compression = struct.unpack('<IIII', self.data[:16])
        
        if magic != self.MAGIC:
            raise ARCError(f"Invalid magic number: expected {self.MAGIC:#x}, got {magic:#x}")
        
        print(f"ARC archive version {self.version} with {self.file_count} files")
        if self.version != 1:
            print(f"Warning: Unknown version {self.version}, continuing anyway")
        
        # Parse cue entries
        cue_size = 16 * self.file_count  # Each entry is 16 bytes
        cue_start = 16
        cue_end = cue_start + cue_size
        
        if len(self.data) < cue_end:
            raise ARCError(f"File too small for {self.file_count} cue entries")
        
        cue_data = self.data[cue_start:cue_end]
        
        for i in range(self.file_count):
            entry_start = i * 16
            entry_data = cue_data[entry_start:entry_start + 16]
            entry = CueEntry.parse(entry_data)
            
            try:
                path = entry.parse_path(self.data)
                self.cue[path] = entry
                print(f"Found file: {path} (offset: {entry.data_offset}, size: {entry.decompressed_size})")
            except Exception as e:
                print(f"Warning: Could not parse path for entry {i}: {e}")
    
    def list_files(self) -> List[str]:
        """Return a list of all file paths in the archive"""
        return list(self.cue.keys())
    
    def get_file_info(self, path: str) -> Optional[Tuple[int, int, bool]]:
        """Get file information: (compressed_size, decompressed_size, is_compressed)"""
        if path not in self.cue:
            return None
        entry = self.cue[path]
        is_compressed = entry.compressed_size != entry.decompressed_size
        return (entry.compressed_size, entry.decompressed_size, is_compressed)
    
    def has_file(self, path: str) -> bool:
        """Check if a file exists in the archive"""
        return path in self.cue
    
    def get_file(self, path: str) -> Optional[bytes]:
        """Extract a single file from the archive"""
        if path not in self.cue:
            return None
        
        entry = self.cue[path]
        
        # Extract compressed data
        data_start = entry.data_offset
        data_end = data_start + entry.compressed_size
        
        if data_end > len(self.data):
            raise ARCError(f"File data extends beyond archive: {data_end} > {len(self.data)}")
        
        compressed_data = self.data[data_start:data_end]
        
        # Check if file is compressed
        if entry.compressed_size != entry.decompressed_size:
            if self.decompress:
                print(f"Decompressing {path}")
                print(f"Compressed size: {entry.compressed_size}, decompressed size: {entry.decompressed_size}")
                
                try:
                    decompressed_data = KonamiLz77.decompress(compressed_data)
                    
                    # Verify decompressed size matches expected size
                    if len(decompressed_data) != entry.decompressed_size:
                        print(f"Warning: Decompressed size mismatch for {path}")
                        print(f"Expected: {entry.decompressed_size}, got: {len(decompressed_data)}")
                    
                    return decompressed_data
                    
                except Exception as e:
                    print(f"Error decompressing {path}: {e}")
                    print("Falling back to compressed data")
                    return compressed_data
            else:
                print(f"Skipping decompression for {path} (--no-decompress flag)")
                return compressed_data
        else:
            return compressed_data


def main():
    parser = argparse.ArgumentParser(
        description='Extract files from DDR A ARC archives',
        epilog="""
Examples:
  %(prog)s file.arc -l              List all files in archive
  %(prog)s file.arc -lv             List files with detailed info
  %(prog)s file.arc                 Extract all files to current directory
  %(prog)s file.arc -o output/      Extract all files to output directory
  %(prog)s file.arc -f song.ssq     Extract only song.ssq file
  %(prog)s file.arc --no-decompress Extract compressed files without decompression
        """,
        formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument('file', help='ARC file to process')
    parser.add_argument('-l', '--list', action='store_true', 
                       help='List files in archive and exit')
    parser.add_argument('-v', '--verbose', action='store_true',
                       help='Show detailed file information (use with --list)')
    parser.add_argument('-f', '--file', dest='single_file', 
                       help='Extract only this specific file')
    parser.add_argument('-o', '--output', default='.', 
                       help='Output directory (default: current directory)')
    parser.add_argument('--no-decompress', action='store_true',
                       help='Skip decompression and extract raw compressed data')
    
    args = parser.parse_args()
    
    # Validate arguments
    if args.verbose and not args.list:
        print("Warning: --verbose flag only affects --list output")
    
    if not os.path.exists(args.file):
        print(f"Error: File '{args.file}' does not exist")
        return 1
    
    # Read the ARC file
    try:
        with open(args.file, 'rb') as f:
            arc_data = f.read()
    except IOError as e:
        print(f"Error reading file {args.file}: {e}")
        return 1
    
    # Parse the archive
    try:
        arc = ARC(arc_data, decompress=not args.no_decompress)
    except ARCError as e:
        print(f"Error parsing ARC file: {e}")
        return 1
    
    # List files if requested
    if args.list:
        print(f"\nFiles in {args.file}:")
        if args.verbose:
            print(f"{'Path':<50} {'Compressed':<12} {'Decompressed':<12} {'Ratio':<8} {'Status'}")
            print("-" * 90)
            for path in sorted(arc.list_files()):
                info = arc.get_file_info(path)
                if info:
                    comp_size, decomp_size, is_compressed = info
                    ratio = f"{comp_size/decomp_size:.2%}" if decomp_size > 0 else "N/A"
                    status = "Compressed" if is_compressed else "Uncompressed"
                    print(f"{path:<50} {comp_size:<12} {decomp_size:<12} {ratio:<8} {status}")
        else:
            for path in sorted(arc.list_files()):
                print(path)
        return 0
    
    # Determine which files to extract
    if args.single_file:
        if not arc.has_file(args.single_file):
            print(f"File '{args.single_file}' not found in archive")
            return 1
        files_to_extract = [args.single_file]
    else:
        files_to_extract = arc.list_files()
    
    # Extract files
    output_dir = Path(args.output)
    output_dir.mkdir(parents=True, exist_ok=True)
    
    for file_path in files_to_extract:
        print(f"Extracting {file_path}...")
        
        try:
            file_data = arc.get_file(file_path)
            if file_data is None:
                print(f"Warning: Could not extract {file_path}")
                continue
            
            # Create output path
            output_path = output_dir / file_path
            output_path.parent.mkdir(parents=True, exist_ok=True)
            
            # Write file
            with open(output_path, 'wb') as f:
                f.write(file_data)
            
            print(f"Wrote {len(file_data)} bytes to {output_path}")
            
        except Exception as e:
            print(f"Error extracting {file_path}: {e}")
    
    print(f"\nExtraction complete. Files written to {output_dir}")
    return 0


if __name__ == '__main__':
    sys.exit(main())