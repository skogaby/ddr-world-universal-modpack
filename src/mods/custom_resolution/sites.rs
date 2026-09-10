//! Immediate-site discovery for the Custom Resolution patch groups
//! (design §4.5/§4.6). Pure byte-slice scanners: callers hand in the bytes
//! that follow a resolved AOB match and get back **offsets** (relative to
//! the slice start) of the immediates to rewrite, together with the stock
//! value each must currently hold. Nothing here touches memory or the
//! module — the impure `patches` layer reads the slices, calls these, and
//! writes through `memory::apply_checked_patch` with the stock bytes as the
//! expected value.
//!
//! Byte shapes were taken from the four supported builds (20250805 /
//! 20260224 / 20260721 / 20260825; offsets identical on all — see the
//! planning record's `prototypes/aob_sweep/REPORT.md` and the 20260825
//! Ghidra listings in `research/`). Every finder is total: it returns
//! `None`/empty rather than guessing when a shape is not where it should be,
//! and the patch layer treats that as "skip this group" (fail-open).

/// Stock render width / height as they appear in immediates.
pub const STOCK_W: u32 = 0x500;
pub const STOCK_H: u32 = 0x2d0;
/// Stock SD (HD flag 0) back-buffer dims in the selector's else-branch.
pub const STOCK_SD_W: u32 = 0x280;
pub const STOCK_SD_H: u32 = 0x1e0;

/// An imm32 to rewrite: byte offset from the slice start + the value it must
/// hold today.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Imm32Site {
    pub off: usize,
    pub stock: u32,
}

fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn site_if(b: &[u8], off: usize, stock: u32) -> Option<Imm32Site> {
    (u32_at(b, off)? == stock).then_some(Imm32Site { off, stock })
}

/// Group 1 — back-buffer selector (`display_backbuffer_dims` match):
/// `CMP byte [RCX+0x12],0; MOV RSI,RCX; JZ +0x16; MOV [w],0x500; MOV [h],0x2d0;
/// JMP +0x14; MOV [w],0x280; MOV [h],0x1e0`. Returns `[hd_w, hd_h, sd_w, sd_h]`
/// imm sites — the patch writes the OUTPUT dims into all four so the HD flag
/// (machine type) no longer matters.
pub fn backbuffer_sites(b: &[u8]) -> Option<[Imm32Site; 4]> {
    Some([
        site_if(b, 15, STOCK_W)?,
        site_if(b, 25, STOCK_H)?,
        site_if(b, 37, STOCK_SD_W)?,
        site_if(b, 47, STOCK_SD_H)?,
    ])
}

/// Group 1b — the application window's client size (`window_client_size`
/// match, in the game's `main`): `MOV word [RBP+d],1; MOV dword [RBP+d],0x500;
/// MOV dword [RBP+d],0x2d0` — the window descriptor `AdjustWindowRectEx` +
/// `CreateWindowExW` consume BEFORE display init. Under spice2x `-w` this is
/// the size the window keeps (spice2x swallows the game's later
/// `SetWindowPos` calls for MDX), so a back-buffer of another size would be
/// stretched into a 1280×720 client without this patch. Fullscreen ignores
/// it. Returns `(w_site, h_site)`.
pub fn window_client_sites(b: &[u8]) -> Option<(Imm32Site, Imm32Site)> {
    if b.get(0..3)? != [0x66, 0xC7, 0x45]
        || b.get(6..8)? != [0xC7, 0x45]
        || b.get(13..15)? != [0xC7, 0x45]
    {
        return None;
    }
    Some((site_if(b, 9, STOCK_W)?, site_if(b, 16, STOCK_H)?))
}

/// Group 1 companion — the RIP disp32 offsets (from the same match) of the
/// two HD-branch stores: `C7 05 <disp32> <imm32>` at +9 and +19. The
/// derivation layer decodes them into the screen w/h globals.
pub const BACKBUFFER_W_DISP_OFF: usize = 11;
pub const BACKBUFFER_H_DISP_OFF: usize = 21;

/// Group 2 — the AA-config store in onBoot, anchored on the `fps_target_imm32`
/// match: `C7 44 24 ?? 03 00 00 00` at +0x69 (imm at +0x6D). The RSP disp8 is
/// not compared (it is `0x68` on every build, but it is a frame-layout
/// artefact, not a semantic).
pub fn aa_config_site(b: &[u8]) -> Option<Imm32Site> {
    const AT: usize = 0x69;
    let head = b.get(AT..AT + 4)?;
    if head[0] != 0xC7 || head[1] != 0x44 || head[2] != 0x24 {
        return None;
    }
    site_if(b, AT + 4, 3)
}

/// Offset (from the `fps_target_imm32` match) at which to start looking for
/// the `CALL rel32` into graphics init — the first `E8` after the AA store
/// and its trailing `LEA RCX,[RSP+disp8]`.
pub const GRAPHICS_INIT_SCAN_START: usize = 0x71;
pub const GRAPHICS_INIT_SCAN_LEN: usize = 0x20;

/// Group 3a — the hoisted registers in the render-surface ctor
/// (`render_surface_hoist` match): `41 BF 00 05 00 00` (MOV R15D,0x500) at +7
/// and `BE D0 02 00 00` (MOV ESI,0x2d0) at +0x2B. Returns `(w_site, h_site)`.
pub fn hoist_sites(b: &[u8]) -> Option<(Imm32Site, Imm32Site)> {
    if b.get(7..9)? != [0x41, 0xBF] {
        return None;
    }
    if *b.get(0x2B)? != 0xBE {
        return None;
    }
    Some((site_if(b, 9, STOCK_W)?, site_if(b, 0x2C, STOCK_H)?))
}

/// Offset of the `E8` (CALL rel32 → the surface-create function, the first
/// `(R15D, R15D, 0x15)` OFFSCREEN1 create) inside the hoist match — a shape
/// fact the fixture test pins; nothing derives from it any more.
pub const HOIST_SURFACE_CREATE_CALL_OFF: usize = 0x13;

/// Group 3b — RT-struct dimension stores inside the render-surface ctor body
/// (scan window after the hoist match).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RtDimSites {
    /// `C7 41 14 00 05 D0 02` — `MOV dword [RCX+0x14], 0x02d00500` (w|h<<16):
    /// rt[0xb] RENDER, rt[0xc], rt[0xe] RENDER_2D. Imm offsets.
    pub packed_wide: Vec<Imm32Site>,
    /// `C7 41 14 00 05 00 05` — rt[0x12] OFFSCREEN1 (w|w<<16). Imm offset.
    pub packed_square: Vec<Imm32Site>,
    /// `C7 40 16 D0 02 00 00` — `MOV dword [RAX+0x16], 0x2d0` (height + zeroed
    /// msaa/pad); the width for these two structs comes from `R15W`. FIRST
    /// occurrence = rt[0x10] PRESENT, second = rt[0xd]. Imm offsets.
    pub height_only: Vec<Imm32Site>,
}

/// Expected counts on every supported build.
pub const RT_PACKED_WIDE_COUNT: usize = 3;
pub const RT_PACKED_SQUARE_COUNT: usize = 1;
pub const RT_HEIGHT_ONLY_COUNT: usize = 2;

pub fn rt_dim_sites(b: &[u8]) -> RtDimSites {
    let mut out = RtDimSites::default();
    let mut i = 0;
    while i + 7 <= b.len() {
        let w = &b[i..i + 7];
        if w[0] == 0xC7 && w[1] == 0x41 && w[2] == 0x14 {
            if w[3..7] == [0x00, 0x05, 0xD0, 0x02] {
                out.packed_wide.push(Imm32Site {
                    off: i + 3,
                    stock: STOCK_W | (STOCK_H << 16),
                });
            } else if w[3..7] == [0x00, 0x05, 0x00, 0x05] {
                out.packed_square.push(Imm32Site {
                    off: i + 3,
                    stock: STOCK_W | (STOCK_W << 16),
                });
            }
        } else if w == [0xC7, 0x40, 0x16, 0xD0, 0x02, 0x00, 0x00] {
            out.height_only.push(Imm32Site {
                off: i + 3,
                stock: STOCK_H,
            });
        }
        i += 1;
    }
    out
}

impl RtDimSites {
    pub fn is_complete(&self) -> bool {
        self.packed_wide.len() == RT_PACKED_WIDE_COUNT
            && self.packed_square.len() == RT_PACKED_SQUARE_COUNT
            && self.height_only.len() == RT_HEIGHT_ONLY_COUNT
    }
}

/// Group 4 — the list-viewport `{name, w, h}` stack table in
/// `FUN_1801f5d10` (`list_viewport_table` match, forward window): every
/// `C7 85 <disp32> <imm32>` (`MOV dword [RBP+disp32], imm32`) whose imm is
/// `0x500` or `0x2d0`, paired by `(disp, disp + 4)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewportKind {
    /// `(0x500, 0x2d0)` — FRONT/MIDDLE/BACK/OFFSCREEN0/RENDER_CAPTURE.
    Wide,
    /// `(0x500, 0x500)` — OFFSCREEN1.
    Square,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewportPair {
    pub w: Imm32Site,
    pub h: Imm32Site,
    pub kind: ViewportKind,
}

pub const VIEWPORT_WIDE_COUNT: usize = 5;
pub const VIEWPORT_SQUARE_COUNT: usize = 1;

pub fn viewport_pairs(b: &[u8]) -> Vec<ViewportPair> {
    // (disp, imm_off, imm)
    let mut stores: Vec<(i32, usize, u32)> = Vec::new();
    let mut i = 0;
    while i + 10 <= b.len() {
        if b[i] == 0xC7 && b[i + 1] == 0x85 {
            let disp = i32::from_le_bytes([b[i + 2], b[i + 3], b[i + 4], b[i + 5]]);
            let imm = u32::from_le_bytes([b[i + 6], b[i + 7], b[i + 8], b[i + 9]]);
            if imm == STOCK_W || imm == STOCK_H {
                stores.push((disp, i + 6, imm));
                i += 10;
                continue;
            }
        }
        i += 1;
    }
    let mut out = Vec::new();
    for (idx, &(disp, w_off, w_imm)) in stores.iter().enumerate() {
        if w_imm != STOCK_W {
            continue;
        }
        // Partner = the store at disp+4 that is not itself already a width
        // consumed as a partner (a (0x500,0x500) pair's second element is
        // also 0x500 — resolve by position: the partner must come AFTER).
        let Some(&(_, h_off, h_imm)) = stores[idx + 1..]
            .iter()
            .find(|(d, _, _)| *d == disp.wrapping_add(4))
        else {
            continue;
        };
        let kind = match h_imm {
            STOCK_H => ViewportKind::Wide,
            STOCK_W => ViewportKind::Square,
            _ => continue,
        };
        // A square pair's second store must not also start a pair.
        out.push(ViewportPair {
            w: Imm32Site {
                off: w_off,
                stock: STOCK_W,
            },
            h: Imm32Site {
                off: h_off,
                stock: h_imm,
            },
            kind,
        });
    }
    // Drop any "pair" whose width store was the height half of a square
    // pair (0x500 at disp+4 of a previous width).
    let heights: Vec<usize> = out.iter().map(|p| p.h.off).collect();
    out.retain(|p| !heights.contains(&p.w.off));
    out
}

pub fn viewport_pairs_complete(pairs: &[ViewportPair]) -> bool {
    pairs
        .iter()
        .filter(|p| p.kind == ViewportKind::Wide)
        .count()
        == VIEWPORT_WIDE_COUNT
        && pairs
            .iter()
            .filter(|p| p.kind == ViewportKind::Square)
            .count()
            == VIEWPORT_SQUARE_COUNT
}

/// Group 5 — letterbox source rect inside `letterbox_rect_fn` (match =
/// function entry): `MOV EDX,0x500` (`BA 00 05 00 00`) at +0x34 (imm at
/// +0x35; doubles as the `screen_w == render_w` comparand) and
/// `MOV dword [RBX+0x298],0x2d0` (`C7 83 98 02 00 00 D0 02 00 00`) somewhere
/// within the function's 0xFA bytes (at +0xDD on every build; window 0x100). Returns `(src_x1_site, src_y1_site)`.
/// Bytes to read after the `letterbox_rect_fn` match for [`letterbox_sites`].
pub const LETTERBOX_SCAN_LEN: usize = 0x100;

pub fn letterbox_sites(b: &[u8]) -> Option<(Imm32Site, Imm32Site)> {
    if *b.get(0x34)? != 0xBA {
        return None;
    }
    let x1 = site_if(b, 0x35, STOCK_W)?;
    const Y1: [u8; 10] = [0xC7, 0x83, 0x98, 0x02, 0x00, 0x00, 0xD0, 0x02, 0x00, 0x00];
    let end = b.len().min(LETTERBOX_SCAN_LEN);
    let pos = b[..end].windows(10).position(|w| w == Y1)?;
    Some((
        x1,
        Imm32Site {
            off: pos + 6,
            stock: STOCK_H,
        },
    ))
}

/// The 16-bit halves of a packed `w | h << 16` immediate.
pub fn pack_dims(w: u32, h: u32) -> Option<u32> {
    (w <= 0xFFFF && h <= 0xFFFF).then_some(w | (h << 16))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        s.split_whitespace()
            .map(|t| u8::from_str_radix(t, 16).unwrap())
            .collect()
    }

    // `display_backbuffer_dims` match bytes (RIP disps arbitrary).
    const BACKBUFFER: &str = "80 79 12 00 48 8B F1 74 16 \
        C7 05 11 22 33 44 00 05 00 00 \
        C7 05 55 66 77 88 D0 02 00 00 EB 14 \
        C7 05 99 AA BB CC 80 02 00 00 \
        C7 05 DD EE FF 00 E0 01 00 00";

    #[test]
    fn backbuffer_four_imms() {
        let b = hex(BACKBUFFER);
        let s = backbuffer_sites(&b).unwrap();
        assert_eq!(
            s[0],
            Imm32Site {
                off: 15,
                stock: 0x500
            }
        );
        assert_eq!(
            s[1],
            Imm32Site {
                off: 25,
                stock: 0x2d0
            }
        );
        assert_eq!(
            s[2],
            Imm32Site {
                off: 37,
                stock: 0x280
            }
        );
        assert_eq!(
            s[3],
            Imm32Site {
                off: 47,
                stock: 0x1e0
            }
        );
        assert_eq!(
            &b[BACKBUFFER_W_DISP_OFF..BACKBUFFER_W_DISP_OFF + 4],
            [0x11, 0x22, 0x33, 0x44]
        );
        assert_eq!(
            &b[BACKBUFFER_H_DISP_OFF..BACKBUFFER_H_DISP_OFF + 4],
            [0x55, 0x66, 0x77, 0x88]
        );
        let mut bad = b.clone();
        bad[16] = 0x06;
        assert!(backbuffer_sites(&bad).is_none());
        assert!(backbuffer_sites(&b[..40]).is_none());
    }

    #[test]
    fn window_client_two_imms() {
        // 66 C7 45 27 01 00 | C7 45 EF 00 05 00 00 | C7 45 F3 D0 02 00 00 | 48 89 7D 97 ...
        let b = hex("66 C7 45 27 01 00 C7 45 EF 00 05 00 00 C7 45 F3 D0 02 00 00 48 89 7D 97 48 89 7D 9F C7 45 A7 00 00 01 00");
        let (w, h) = window_client_sites(&b).unwrap();
        assert_eq!(
            w,
            Imm32Site {
                off: 9,
                stock: 0x500
            }
        );
        assert_eq!(
            h,
            Imm32Site {
                off: 16,
                stock: 0x2d0
            }
        );
        let mut bad = b.clone();
        bad[17] = 0xD1;
        assert!(window_client_sites(&bad).is_none());
    }

    #[test]
    fn aa_config_anchor() {
        let mut b = vec![0x90u8; 0x80];
        b[0x69..0x71].copy_from_slice(&[0xC7, 0x44, 0x24, 0x68, 0x03, 0x00, 0x00, 0x00]);
        assert_eq!(
            aa_config_site(&b),
            Some(Imm32Site {
                off: 0x6D,
                stock: 3
            })
        );
        b[0x6D] = 0x02;
        assert!(aa_config_site(&b).is_none());
        b[0x6D] = 0x03;
        b[0x6A] = 0x45;
        assert!(aa_config_site(&b).is_none());
    }

    #[test]
    fn hoist_two_registers() {
        // 45 33 C9 45 8D 41 15 41 BF 00 05 00 00 41 8B D7 41 8B CF E8 xx xx xx xx ...
        let mut b = hex("45 33 C9 45 8D 41 15 41 BF 00 05 00 00 41 8B D7 41 8B CF E8 00 00 00 00");
        b.resize(0x2B, 0x90);
        b.extend_from_slice(&[0xBE, 0xD0, 0x02, 0x00, 0x00]);
        let (w, h) = hoist_sites(&b).unwrap();
        assert_eq!(
            w,
            Imm32Site {
                off: 9,
                stock: 0x500
            }
        );
        assert_eq!(
            h,
            Imm32Site {
                off: 0x2C,
                stock: 0x2d0
            }
        );
        assert_eq!(b[HOIST_SURFACE_CREATE_CALL_OFF], 0xE8);
        let mut bad = b.clone();
        bad[0x2B] = 0xBF;
        assert!(hoist_sites(&bad).is_none());
    }

    #[test]
    fn rt_dims_counts_and_order() {
        let mut b = vec![0x90u8; 0x40];
        // PRESENT (height-only) first, then a wide, a square, two wides, rt[0xd] height-only.
        let ho = [0xC7, 0x40, 0x16, 0xD0, 0x02, 0x00, 0x00];
        let wide = [0xC7, 0x41, 0x14, 0x00, 0x05, 0xD0, 0x02];
        let sq = [0xC7, 0x41, 0x14, 0x00, 0x05, 0x00, 0x05];
        let seq: [&[u8]; 6] = [&ho, &wide, &sq, &wide, &wide, &ho];
        let mut offs = Vec::new();
        for ins in seq {
            offs.push(b.len());
            b.extend_from_slice(ins);
            b.extend_from_slice(&[0x90; 5]);
        }
        let s = rt_dim_sites(&b);
        assert!(s.is_complete());
        assert_eq!(s.height_only[0].off, offs[0] + 3);
        assert_eq!(s.height_only[1].off, offs[5] + 3);
        assert_eq!(s.height_only[0].stock, 0x2d0);
        assert_eq!(
            s.packed_wide.iter().map(|x| x.off).collect::<Vec<_>>(),
            vec![offs[1] + 3, offs[3] + 3, offs[4] + 3]
        );
        assert_eq!(s.packed_wide[0].stock, 0x02d0_0500);
        assert_eq!(
            s.packed_square,
            vec![Imm32Site {
                off: offs[2] + 3,
                stock: 0x0500_0500
            }]
        );
        // A missing site makes the set incomplete.
        assert!(!rt_dim_sites(&b[..offs[5]]).is_complete());
    }

    fn mov_rbp(disp: u32, imm: u32) -> Vec<u8> {
        let mut v = vec![0xC7, 0x85];
        v.extend_from_slice(&disp.to_le_bytes());
        v.extend_from_slice(&imm.to_le_bytes());
        v
    }

    #[test]
    fn viewport_pairs_from_the_stack_table() {
        // Mirrors FUN_1801f6dc0's table: 3 wide, SYSTEM (register stores, not
        // literal), OFFSCREEN0 wide, OFFSCREEN1 square, DEBUG (registers),
        // RENDER_CAPTURE wide — with LEA/MOV noise between entries.
        let mut b = Vec::new();
        let noise = [
            0x48, 0x8D, 0x05, 0x10, 0x20, 0x30, 0x40, 0x48, 0x89, 0x85, 0x30, 0x03, 0x00, 0x00,
        ];
        let mut expect_w = Vec::new();
        for base in [0x338u32, 0x348, 0x358] {
            b.extend_from_slice(&noise);
            expect_w.push(b.len() + 6);
            b.extend_from_slice(&mov_rbp(base, 0x500));
            b.extend_from_slice(&mov_rbp(base + 4, 0x2d0));
        }
        // SYSTEM: MOV dword [RBP+0x368],EDX (89 95 ...) — no literal
        b.extend_from_slice(&[0x89, 0x95, 0x68, 0x03, 0x00, 0x00]);
        b.extend_from_slice(&noise);
        expect_w.push(b.len() + 6);
        b.extend_from_slice(&mov_rbp(0x378, 0x500));
        b.extend_from_slice(&mov_rbp(0x37c, 0x2d0));
        b.extend_from_slice(&noise);
        let sq_w = b.len() + 6;
        b.extend_from_slice(&mov_rbp(0x388, 0x500));
        b.extend_from_slice(&mov_rbp(0x38c, 0x500));
        b.extend_from_slice(&[0x89, 0x95, 0x98, 0x03, 0x00, 0x00]);
        b.extend_from_slice(&noise);
        expect_w.push(b.len() + 6);
        b.extend_from_slice(&mov_rbp(0x3a8, 0x500));
        b.extend_from_slice(&mov_rbp(0x3ac, 0x2d0));

        let pairs = viewport_pairs(&b);
        assert!(viewport_pairs_complete(&pairs), "{pairs:?}");
        let wides: Vec<usize> = pairs
            .iter()
            .filter(|p| p.kind == ViewportKind::Wide)
            .map(|p| p.w.off)
            .collect();
        assert_eq!(wides, expect_w);
        let sq = pairs
            .iter()
            .find(|p| p.kind == ViewportKind::Square)
            .unwrap();
        assert_eq!(sq.w.off, sq_w);
        assert_eq!(sq.h.off, sq_w + 10);
        assert_eq!(sq.h.stock, 0x500);
        for p in &pairs {
            assert_eq!(p.h.off, p.w.off + 10);
        }
    }

    #[test]
    fn letterbox_two_imms() {
        let mut b = vec![0x90u8; 0x34];
        b.extend_from_slice(&[0xBA, 0x00, 0x05, 0x00, 0x00]);
        b.resize(0xDD, 0x90);
        b.extend_from_slice(&[0xC7, 0x83, 0x98, 0x02, 0x00, 0x00, 0xD0, 0x02, 0x00, 0x00]);
        b.resize(0x100, 0x90);
        let (x1, y1) = letterbox_sites(&b).unwrap();
        assert_eq!(
            x1,
            Imm32Site {
                off: 0x35,
                stock: 0x500
            }
        );
        assert_eq!(
            y1,
            Imm32Site {
                off: 0xE3,
                stock: 0x2d0
            }
        );
        let mut bad = b.clone();
        bad[0xE3] = 0xD1;
        assert!(letterbox_sites(&bad).is_none());
    }

    #[test]
    fn pack_dims_halves() {
        assert_eq!(pack_dims(0x500, 0x2d0), Some(0x02d0_0500));
        assert_eq!(pack_dims(1920, 1080), Some(1920 | (1080 << 16)));
        assert_eq!(pack_dims(70000, 1), None);
    }
}
