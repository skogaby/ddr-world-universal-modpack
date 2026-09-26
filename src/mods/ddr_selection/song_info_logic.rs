//! Legacy song info — the pure patch plan (host-tested).
//!
//! Dependency-free on purpose: `scripts/validate_ddr_selection.sh` mounts this
//! file into a throwaway host crate.
//!
//! Two legacy shapes of World's `SongInfoActor` (RE:
//! `.agents/planning/2026-09-22-ddr-selection/research/legacy-score.md` §6 /
//! §7):
//!
//! * **Band** (skin 2, MAX-EXTREME): both card-name LEAs → `"dance_song_info"`
//!   and the layer priority `5 → 9`. The band has no text children.
//! * **Panel** (skins 3–5): A3's own skin-0 panel `dance_song_info0000_v2`.
//!   The same two card LEAs (priority stays 5, like A3), plus World's
//!   SongInfoChild turned into A3's: the child ctor's font `4 → 3`
//!   (`2d_font_songtitle_s` → `_m`), the colour skip `JNZ` NOPed (white text
//!   — World writes it only for the double card), the four ctor + four
//!   update name LEAs → `music_name_usr` / `artist_name_usr`, and in the text
//!   helper the horizontal alignment `0 → 1` (centred: `MOV [rdx+0xA8],r12d`
//!   → `MOV BYTE [rdx+0xA8],1`, the dword is freshly zeroed) and the `SUB`
//!   that moves World's fit box one placeholder width left NOPed.

/// World's / A3's layer priorities.
pub const WORLD_PRIORITY: u8 = 5;
pub const BAND_PRIORITY: u8 = 9;
/// World's / A3's SongInfoChild font ids (index into the font table:
/// 3 = `2d_font_songtitle_m`, 4 = `2d_font_songtitle_s`).
pub const WORLD_FONT: u8 = 4;
pub const A3_FONT: u8 = 3;

/// A3's export in both legacy packages and the panel's two text children.
pub const EXPORT: &[u8] = b"dance_song_info\0";
pub const MUSIC_NAME: &[u8] = b"music_name_usr\0";
pub const ARTIST_NAME: &[u8] = b"artist_name_usr\0";

/// Which legacy shape a skin uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Band,
    Panel,
}

pub fn mode_for_skin(skin: u8) -> Option<Mode> {
    match skin {
        2 => Some(Mode::Band),
        // Skins 3–5 and the themes: A3's own skin-0 panel (the themes from
        // their own generation's `dance_song_info0000_vN`).
        3..=8 => Some(Mode::Panel),
        _ => None,
    }
}

/// One checked byte patch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Patch {
    pub at: usize,
    pub stock: Vec<u8>,
    pub legacy: Vec<u8>,
}

/// The card-name site (`LEA RAX,[single]; LEA R8,[double]; …; MOV R9D,5`)
/// with its stock bytes.
#[derive(Clone, Copy, Debug)]
pub struct CardSites {
    pub site: usize,
    pub lea_stock: [[u8; 4]; 2],
}

/// `(disp32 offset, next-instruction offset)` of the two card LEAs, and the
/// priority imm, in the card site.
pub const CARD_LEAS: [(usize, usize); 2] = [(3, 7), (10, 14)];
pub const CARD_PRIORITY_IMM: usize = 31;

/// The panel sites with their stock bytes (see the module docs).
#[derive(Clone, Copy, Debug)]
pub struct PanelSites {
    pub font_imm: usize,
    pub font_stock: u8,
    pub color_jcc: usize,
    pub color_stock: [u8; 2],
    /// Ctor music ×2, artist ×2, update music ×2, artist ×2 (disp32 at +3,
    /// next instruction at +7).
    pub name_leas: [usize; 8],
    pub name_stock: [[u8; 4]; 8],
    pub align_store: usize,
    pub align_stock: [u8; 7],
    pub x_sub: usize,
    pub x_sub_stock: [u8; 2],
}

/// Where the near strings live.
#[derive(Clone, Copy, Debug)]
pub struct Buffers {
    pub export: usize,
    pub music: usize,
    pub artist: usize,
}

pub fn rel32(from_next: usize, to: usize) -> Option<[u8; 4]> {
    i32::try_from(to as i64 - from_next as i64)
        .ok()
        .map(i32::to_le_bytes)
}

/// `MOV [r64+disp32],r12d` (`44 89 modrm disp32`, ModRM mod=10 reg=100) →
/// `MOV BYTE [r64+disp32],1` (`C6 modrm' disp32 01`) — same 7 bytes. `None`
/// for any other shape (extended base registers would need a REX byte).
pub fn align_store_replacement(stock: [u8; 7]) -> Option<[u8; 7]> {
    let modrm = stock[2];
    if stock[0] != 0x44 || stock[1] != 0x89 || modrm >> 6 != 2 || (modrm >> 3) & 7 != 4 {
        return None;
    }
    let rm = modrm & 7;
    if rm == 4 {
        return None;
    }
    Some([
        0xC6,
        0x80 | rm,
        stock[3],
        stock[4],
        stock[5],
        stock[6],
        0x01,
    ])
}

/// Whether the stock bytes are the shapes the plan rewrites.
pub fn panel_stock_ok(p: &PanelSites) -> bool {
    p.font_stock == WORLD_FONT
        && p.color_stock[0] == 0x75
        && align_store_replacement(p.align_stock).is_some()
        && p.x_sub_stock[0] == 0x2B
        && p.x_sub_stock[1] >> 6 == 3
}

/// The patches that turn World's song info into `mode`. `None` when a string
/// is out of rel32 reach or a stock shape is unexpected.
pub fn plan(
    mode: Mode,
    card: &CardSites,
    panel: Option<&PanelSites>,
    b: &Buffers,
) -> Option<Vec<Patch>> {
    let mut out = Vec::new();
    for (i, (disp, next)) in CARD_LEAS.iter().enumerate() {
        out.push(Patch {
            at: card.site + disp,
            stock: card.lea_stock[i].to_vec(),
            legacy: rel32(card.site + next, b.export)?.to_vec(),
        });
    }
    match mode {
        Mode::Band => out.push(Patch {
            at: card.site + CARD_PRIORITY_IMM,
            stock: vec![WORLD_PRIORITY],
            legacy: vec![BAND_PRIORITY],
        }),
        Mode::Panel => {
            let p = panel?;
            if !panel_stock_ok(p) {
                return None;
            }
            out.push(Patch {
                at: p.font_imm,
                stock: vec![WORLD_FONT],
                legacy: vec![A3_FONT],
            });
            out.push(Patch {
                at: p.color_jcc,
                stock: p.color_stock.to_vec(),
                legacy: vec![0x90, 0x90],
            });
            for (i, lea) in p.name_leas.iter().enumerate() {
                let to = if (i / 2) % 2 == 0 { b.music } else { b.artist };
                out.push(Patch {
                    at: lea + 3,
                    stock: p.name_stock[i].to_vec(),
                    legacy: rel32(lea + 7, to)?.to_vec(),
                });
            }
            out.push(Patch {
                at: p.align_store,
                stock: p.align_stock.to_vec(),
                legacy: align_store_replacement(p.align_stock)?.to_vec(),
            });
            out.push(Patch {
                at: p.x_sub,
                stock: p.x_sub_stock.to_vec(),
                legacy: vec![0x90, 0x90],
            });
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> CardSites {
        CardSites {
            site: 0x1000,
            lea_stock: [[1, 2, 3, 4], [5, 6, 7, 8]],
        }
    }

    fn panel() -> PanelSites {
        PanelSites {
            font_imm: 0x2000,
            font_stock: 4,
            color_jcc: 0x2100,
            color_stock: [0x75, 0x40],
            name_leas: [
                0x3000, 0x3100, 0x3200, 0x3300, 0x4000, 0x4100, 0x4200, 0x4300,
            ],
            name_stock: [[9; 4]; 8],
            align_store: 0x5000,
            // 20260825: MOV [RDX+0xA8],R12D
            align_stock: [0x44, 0x89, 0xA2, 0xA8, 0, 0, 0],
            x_sub: 0x5100,
            // SUB EBX,EAX
            x_sub_stock: [0x2B, 0xD8],
        }
    }

    fn bufs() -> Buffers {
        Buffers {
            export: 0x9000,
            music: 0x9020,
            artist: 0x9040,
        }
    }

    #[test]
    fn modes_by_skin() {
        assert_eq!(mode_for_skin(0), None);
        assert_eq!(mode_for_skin(1), None);
        assert_eq!(mode_for_skin(2), Some(Mode::Band));
        for s in 3..=8 {
            assert_eq!(mode_for_skin(s), Some(Mode::Panel));
        }
        assert_eq!(mode_for_skin(9), None);
    }

    #[test]
    fn align_store_encodes_a_byte_store_of_one() {
        assert_eq!(
            align_store_replacement([0x44, 0x89, 0xA2, 0xA8, 0, 0, 0]),
            Some([0xC6, 0x82, 0xA8, 0, 0, 0, 1])
        );
        // RBX base
        assert_eq!(
            align_store_replacement([0x44, 0x89, 0xA3, 0xA8, 0, 0, 0]),
            Some([0xC6, 0x83, 0xA8, 0, 0, 0, 1])
        );
        // SIB, wrong reg, wrong opcode, extended base (REX.B)
        assert_eq!(
            align_store_replacement([0x44, 0x89, 0xA4, 0xA8, 0, 0, 0]),
            None
        );
        assert_eq!(
            align_store_replacement([0x44, 0x89, 0x82, 0xA8, 0, 0, 0]),
            None
        );
        assert_eq!(
            align_store_replacement([0x44, 0x8B, 0xA2, 0xA8, 0, 0, 0]),
            None
        );
        assert_eq!(
            align_store_replacement([0x45, 0x89, 0xA2, 0xA8, 0, 0, 0]),
            None
        );
    }

    #[test]
    fn band_plan_is_the_card_names_and_priority() {
        let p = plan(Mode::Band, &card(), None, &bufs()).unwrap();
        assert_eq!(p.len(), 3);
        assert_eq!(p[0].at, 0x1003);
        assert_eq!(p[0].legacy, rel32(0x1007, 0x9000).unwrap().to_vec());
        assert_eq!(p[1].at, 0x100A);
        assert_eq!(p[1].legacy, rel32(0x100E, 0x9000).unwrap().to_vec());
        assert_eq!(
            p[2],
            Patch {
                at: 0x101F,
                stock: vec![5],
                legacy: vec![9]
            }
        );
    }

    #[test]
    fn panel_plan_keeps_priority_and_rewrites_the_child() {
        let pan = panel();
        let p = plan(Mode::Panel, &card(), Some(&pan), &bufs()).unwrap();
        assert_eq!(p.len(), 2 + 1 + 1 + 8 + 1 + 1);
        assert!(p.iter().all(|x| x.at != 0x1000 + CARD_PRIORITY_IMM));
        assert_eq!(
            p[2],
            Patch {
                at: 0x2000,
                stock: vec![4],
                legacy: vec![3]
            }
        );
        assert_eq!(
            p[3],
            Patch {
                at: 0x2100,
                stock: vec![0x75, 0x40],
                legacy: vec![0x90, 0x90]
            }
        );
        // music, music, artist, artist — ctor then update
        let wants = [
            0x9020, 0x9020, 0x9040, 0x9040, 0x9020, 0x9020, 0x9040, 0x9040,
        ];
        for (i, want) in wants.iter().enumerate() {
            let x = &p[4 + i];
            assert_eq!(x.at, pan.name_leas[i] + 3);
            assert_eq!(
                x.legacy,
                rel32(pan.name_leas[i] + 7, *want).unwrap().to_vec()
            );
        }
        assert_eq!(p[12].legacy, vec![0xC6, 0x82, 0xA8, 0, 0, 0, 1]);
        assert_eq!(
            p[13],
            Patch {
                at: 0x5100,
                stock: vec![0x2B, 0xD8],
                legacy: vec![0x90, 0x90]
            }
        );
        // every patch keeps its length
        assert!(p.iter().all(|x| x.stock.len() == x.legacy.len()));
    }

    #[test]
    fn panel_plan_needs_the_panel_sites_and_stock_shapes() {
        assert!(plan(Mode::Panel, &card(), None, &bufs()).is_none());
        let mut pan = panel();
        pan.color_stock = [0x74, 0x40];
        assert!(plan(Mode::Panel, &card(), Some(&pan), &bufs()).is_none());
        let mut pan = panel();
        pan.x_sub_stock = [0x29, 0xC3];
        assert!(plan(Mode::Panel, &card(), Some(&pan), &bufs()).is_none());
        assert!(panel_stock_ok(&panel()));
        let mut pan = panel();
        pan.font_stock = 3;
        assert!(!panel_stock_ok(&pan));
    }

    #[test]
    fn out_of_reach_strings_refuse() {
        let far = Buffers {
            export: 0x1_0000_0000 + 0x9000,
            music: 0x9020,
            artist: 0x9040,
        };
        assert!(plan(Mode::Band, &card(), None, &far).is_none());
        let far = Buffers {
            export: 0x9000,
            music: 0x2_0000_0000,
            artist: 0x9040,
        };
        assert!(plan(Mode::Panel, &card(), Some(&panel()), &far).is_none());
        assert!(plan(Mode::Band, &card(), None, &far).is_some());
    }

    #[test]
    fn strings_are_a3s() {
        assert_eq!(EXPORT, b"dance_song_info\0");
        assert_eq!(MUSIC_NAME, b"music_name_usr\0");
        assert_eq!(ARTIST_NAME, b"artist_name_usr\0");
        assert_eq!(A3_FONT, 3);
        assert_eq!(WORLD_FONT, 4);
    }
}
