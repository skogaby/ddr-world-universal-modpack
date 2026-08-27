//! Konami's LZ77 compression variant (AVSLZ).
//! 4096-byte sliding window, 3-byte match threshold, big-endian back-references.

const WINDOW_SIZE: usize = 0x1000;
const WINDOW_MASK: usize = WINDOW_SIZE - 1;
const THRESHOLD: usize = 3;
const MAX_MATCH: usize = THRESHOLD + 0x0F; // 18 bytes max

/// Compress data using Konami's LZ77 variant with sliding window search.
///
/// Match search is accelerated with a hash chain over 2-byte prefixes
/// (`head`/`prev`): we probe only positions that actually share the next two
/// bytes, newest-first (= smallest-offset-first), instead of brute-forcing all
/// 4096 window offsets for every input byte. Output is **byte-identical** to
/// the naive search — same candidate order yields the same "longest match,
/// smallest offset on ties" result and the same early-exit — so it stays
/// wire-compatible with the game's decompressor. Verified by differential
/// tests against the naive matcher (see the `tests` module). This took a
/// 32 MB option-preview atlas from ~24 s to compress down to ~100 ms.
pub fn compress(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let mut window = [0u8; WINDOW_SIZE];
    let mut w_cursor: usize = 0;
    let mut pos: usize = 0;

    // Hash chain over input positions, keyed on the 2-byte prefix at each
    // position. `head[key]` = most recent input position with that prefix
    // (-1 if none); `prev[p]` = the next-older position sharing p's prefix.
    // Built incrementally as the cursor advances, so at any `pos` a key's
    // chain holds every earlier position with that prefix, newest first.
    let mut head = vec![-1i32; 1 << 16];
    let mut prev = vec![-1i32; input.len().max(1)];

    while pos < input.len() {
        let flag_pos = output.len();
        output.push(0u8); // placeholder for flag byte
        let mut flag: u8 = 0;

        for bit in 0..8u8 {
            if pos >= input.len() {
                // Remaining bits are 0 → back-ref → write terminator
                output.extend_from_slice(&[0x00, 0x00]);
                for _ in (bit + 1)..8 {
                    output.extend_from_slice(&[0x00, 0x00]);
                }
                output[flag_pos] = flag;
                return output;
            }

            // Search for the best match. For `pos >= WINDOW_SIZE` the window is
            // exactly `input[pos-4096 .. pos]`, so the hash chain over input
            // positions reproduces the brute-force search precisely. Before
            // that the window includes the leading zero-fill (which the chain
            // can't represent), and at the final byte there's no 2-byte prefix
            // to key on, so fall back to the exact naive search (both cheap).
            let (match_offset, match_len) = if pos >= WINDOW_SIZE && pos + 1 < input.len() {
                let key = ((input[pos] as usize) << 8) | (input[pos + 1] as usize);
                find_match_hashed(input, pos, &head, &prev, key)
            } else {
                find_match(input, pos, &window, w_cursor)
            };

            let advance = if match_len >= THRESHOLD {
                let w = ((match_offset as u16) << 4) | ((match_len - THRESHOLD) as u16);
                output.push((w >> 8) as u8);
                output.push((w & 0xFF) as u8);
                match_len
            } else {
                flag |= 1 << bit;
                output.push(input[pos]);
                1
            };

            // Advance the cursor, inserting each consumed position into the
            // hash chain so future searches can find it.
            for _ in 0..advance {
                if pos + 1 < input.len() {
                    let k = ((input[pos] as usize) << 8) | (input[pos + 1] as usize);
                    prev[pos] = head[k];
                    head[k] = pos as i32;
                }
                window[w_cursor] = input[pos];
                w_cursor = (w_cursor + 1) & WINDOW_MASK;
                pos += 1;
            }
        }

        output[flag_pos] = flag;
    }

    output.extend_from_slice(&[0x00, 0x00, 0x00]);
    output
}

/// Hash-chain match search. Walks candidate input positions sharing the
/// 2-byte prefix at `pos`, newest-first (= smallest offset first), running the
/// identical extension + overlap logic as [`find_match`]. Only valid when
/// `pos >= WINDOW_SIZE` (so every `offset in 1..WINDOW_SIZE` maps to a real
/// input byte `input[pos-offset]`, matching what the decompressor's window
/// holds). Produces the same `(offset, len)` the naive search would, so the
/// emitted stream is unchanged.
fn find_match_hashed(
    input: &[u8],
    pos: usize,
    head: &[i32],
    prev: &[i32],
    key: usize,
) -> (usize, usize) {
    let remaining = input.len() - pos;
    let max_len = remaining.min(MAX_MATCH);
    if max_len < THRESHOLD {
        return (0, 0);
    }

    let min_pos = pos - (WINDOW_SIZE - 1); // smallest allowed candidate (offset <= 4095)
    let mut best_offset = 0usize;
    let mut best_len = 0usize;

    let mut cand = head[key];
    while cand >= 0 {
        let cp = cand as usize;
        if cp < min_pos {
            break; // chain is newest→oldest; everything past here is out of window
        }
        let offset = pos - cp;
        // Extend, reproducing the decompressor's overlap behavior: once
        // `len >= offset` it re-reads the pattern it just emitted.
        let mut len = 0usize;
        while len < max_len {
            let b = if len < offset {
                input[cp + len]
            } else {
                input[pos + (len % offset)]
            };
            if b != input[pos + len] {
                break;
            }
            len += 1;
        }
        if len >= THRESHOLD && len > best_len {
            best_len = len;
            best_offset = offset;
            if best_len == max_len {
                break;
            }
        }
        cand = prev[cp];
    }

    (best_offset, best_len)
}

/// Find the longest match in the sliding window.
/// Handles the overlap case where offset < length — the decompressor reads
/// from its own recently-written output, creating a repeating pattern.
fn find_match(
    input: &[u8],
    pos: usize,
    window: &[u8; WINDOW_SIZE],
    w_cursor: usize,
) -> (usize, usize) {
    let remaining = input.len() - pos;
    let max_len = remaining.min(MAX_MATCH);
    if max_len < THRESHOLD {
        return (0, 0);
    }

    let mut best_offset = 0usize;
    let mut best_len = 0usize;

    for offset in 1..WINDOW_SIZE {
        let start = w_cursor.wrapping_sub(offset) & WINDOW_MASK;
        let mut len = 0;

        while len < max_len {
            // Simulate what the decompressor reads: it copies from the window
            // starting at `start`, but each byte it copies also advances the
            // window cursor. When len >= offset, it re-reads bytes it just wrote,
            // which are input[pos .. pos+offset] repeating.
            let src_idx = (start + len) & WINDOW_MASK;
            let decompressor_byte = if len < offset {
                window[src_idx]
            } else {
                // The decompressor would read what it already wrote:
                // the pattern repeats with period `offset`
                input[pos + (len % offset)]
            };
            if decompressor_byte != input[pos + len] {
                break;
            }
            len += 1;
        }

        if len >= THRESHOLD && len > best_len {
            best_len = len;
            best_offset = offset;
            if best_len == max_len {
                break;
            }
        }
    }

    (best_offset, best_len)
}

/// Decompress data using Konami's LZ77 variant.
pub fn decompress(input: &[u8]) -> Option<Vec<u8>> {
    let mut pos = 0;
    let mut window = [0u8; WINDOW_SIZE];
    let mut w_cursor = 0usize;
    let mut output = Vec::new();

    while pos < input.len() {
        let flag = input[pos];
        pos += 1;

        for bit in 0..8 {
            if (flag >> bit) & 1 == 1 {
                if pos >= input.len() {
                    return None;
                }
                let b = input[pos];
                pos += 1;
                output.push(b);
                window[w_cursor] = b;
                w_cursor = (w_cursor + 1) & WINDOW_MASK;
            } else {
                if pos + 1 >= input.len() {
                    return None;
                }
                let w = ((input[pos] as u16) << 8) | (input[pos + 1] as u16);
                pos += 2;
                if w == 0 {
                    return Some(output);
                }
                let offset = (w >> 4) as usize;
                let length = (w & 0x0F) as usize + THRESHOLD;
                let src = w_cursor.wrapping_sub(offset) & WINDOW_MASK;
                for i in 0..length {
                    let b = window[(src + i) & WINDOW_MASK];
                    output.push(b);
                    window[w_cursor] = b;
                    w_cursor = (w_cursor + 1) & WINDOW_MASK;
                }
            }
        }
    }

    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_small() {
        let data = b"Hello, e-Amusement!";
        let compressed = compress(data);
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, data.to_vec());
    }

    #[test]
    fn roundtrip_empty() {
        let compressed = compress(&[]);
        let decompressed = decompress(&compressed).unwrap();
        assert!(decompressed.is_empty());
    }

    #[test]
    fn roundtrip_exact_8_bytes() {
        let data = b"12345678";
        let compressed = compress(data);
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, data.to_vec());
    }

    #[test]
    fn roundtrip_various_sizes() {
        for size in [1, 3, 7, 9, 15, 17, 100, 255, 1024] {
            let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
            let compressed = compress(&data);
            let decompressed = decompress(&compressed).unwrap();
            assert_eq!(decompressed, data, "failed for size {size}");
        }
    }

    #[test]
    fn compresses_repetitive_data() {
        let data = vec![0xABu8; 4096];
        let compressed = compress(&data);
        assert!(
            compressed.len() < data.len(),
            "repetitive data should compress"
        );
    }

    #[test]
    fn roundtrip_large() {
        let data: Vec<u8> = (0..1024 * 64).map(|i| (i % 256) as u8).collect();
        let compressed = compress(&data);
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    /// Naive brute-force compressor — the pre-optimization reference. The
    /// hash-chain `compress` MUST produce byte-identical output to this for
    /// every input (see `differential_*` tests). Kept here so any future
    /// change to the matcher is caught immediately.
    fn compress_naive(input: &[u8]) -> Vec<u8> {
        let mut output = Vec::with_capacity(input.len());
        let mut window = [0u8; WINDOW_SIZE];
        let mut w_cursor: usize = 0;
        let mut pos: usize = 0;
        while pos < input.len() {
            let flag_pos = output.len();
            output.push(0u8);
            let mut flag: u8 = 0;
            for bit in 0..8u8 {
                if pos >= input.len() {
                    for _ in bit..8 {
                        output.extend_from_slice(&[0x00, 0x00]);
                    }
                    output[flag_pos] = flag;
                    return output;
                }
                let (mo, ml) = find_match(input, pos, &window, w_cursor);
                if ml >= THRESHOLD {
                    let w = ((mo as u16) << 4) | ((ml - THRESHOLD) as u16);
                    output.push((w >> 8) as u8);
                    output.push((w & 0xFF) as u8);
                    for _ in 0..ml {
                        window[w_cursor] = input[pos];
                        w_cursor = (w_cursor + 1) & WINDOW_MASK;
                        pos += 1;
                    }
                } else {
                    flag |= 1 << bit;
                    output.push(input[pos]);
                    window[w_cursor] = input[pos];
                    w_cursor = (w_cursor + 1) & WINDOW_MASK;
                    pos += 1;
                }
            }
            output[flag_pos] = flag;
        }
        output.extend_from_slice(&[0x00, 0x00, 0x00]);
        output
    }

    fn diff_check(data: &[u8], label: &str) {
        assert_eq!(
            compress(data),
            compress_naive(data),
            "hash-chain compress diverged from naive for {label} (len {})",
            data.len()
        );
        assert_eq!(
            decompress(&compress(data)).unwrap(),
            data,
            "roundtrip failed for {label}"
        );
    }

    #[test]
    fn differential_random_sizes() {
        let mut s: u32 = 0x12345678;
        let mut rng = || {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            (s >> 16) as u8
        };
        for &n in &[
            0usize, 1, 2, 3, 7, 8, 9, 16, 17, 255, 256, 257, 1000, 4095, 4096, 4097, 8192, 70000,
        ] {
            let data: Vec<u8> = (0..n).map(|_| rng()).collect();
            diff_check(&data, &format!("random_{n}"));
        }
    }

    #[test]
    fn differential_atlas_like() {
        // Large zero field with dense pseudo-random blocks — mirrors a cloned
        // texture atlas (mostly-transparent canvas with packed thumbnails),
        // the case the hash chain was added to speed up.
        let mut data = vec![0u8; 60000];
        let mut s: u32 = 0xABCDEF01;
        let mut rng = || {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            (s >> 16) as u8
        };
        for blk in 0..6 {
            let start = 4000 + blk * 8000;
            for i in 0..3000 {
                data[start + i] = rng();
            }
        }
        diff_check(&data, "atlas_like");
    }

    #[test]
    fn differential_repetitive() {
        diff_check(&vec![0xAA; 50000], "all_same");
        diff_check(&b"abcabcabc".repeat(3000), "abc_repeat");
        let d: Vec<u8> = (0..20000u32).map(|i| (i % 7) as u8).collect();
        diff_check(&d, "mod7");
    }
}
