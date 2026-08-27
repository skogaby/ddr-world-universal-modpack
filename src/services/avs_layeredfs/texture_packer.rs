//! Texture packer — minimal GuillotineBinPack port for packing new textures into IFS atlases.
//!
//! Only implements RectBestAreaFit + SplitLongerAxis, which is all ifs_layeredfs uses.

const MAX_TEXTURE: i32 = 4096;

#[derive(Clone)]
pub struct Bitmap {
    pub name: String,
    pub width: i32,
    pub height: i32,
    pub pack_x: i32,
    pub pack_y: i32,
}

pub struct PackedCanvas {
    pub width: i32,
    pub height: i32,
    pub bitmaps: Vec<Bitmap>,
}

struct Rect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

struct GuillotinePacker {
    bin_w: i32,
    bin_h: i32,
    free_rects: Vec<Rect>,
}

impl GuillotinePacker {
    fn new(w: i32, h: i32) -> Self {
        Self {
            bin_w: w,
            bin_h: h,
            free_rects: vec![Rect {
                x: 0,
                y: 0,
                width: w,
                height: h,
            }],
        }
    }

    /// Insert a rectangle using BestAreaFit + SplitLongerAxis. Returns None if it doesn't fit.
    fn insert(&mut self, w: i32, h: i32) -> Option<(i32, i32)> {
        // Find best free rect (smallest area that fits)
        let mut best_idx = None;
        let mut best_area = i32::MAX;
        for (i, r) in self.free_rects.iter().enumerate() {
            if r.width >= w && r.height >= h {
                let area = r.width * r.height;
                if area < best_area {
                    best_area = area;
                    best_idx = Some(i);
                }
            }
        }

        let idx = best_idx?;
        let free = self.free_rects.remove(idx);
        let placed = Rect {
            x: free.x,
            y: free.y,
            width: w,
            height: h,
        };

        // Split remaining space along longer axis
        let remain_w = free.width - w;
        let remain_h = free.height - h;
        let split_horizontal = remain_w < remain_h;

        if split_horizontal {
            // Bottom remainder
            if free.height - h > 0 {
                self.free_rects.push(Rect {
                    x: free.x,
                    y: free.y + h,
                    width: free.width,
                    height: free.height - h,
                });
            }
            // Right remainder
            if free.width - w > 0 {
                self.free_rects.push(Rect {
                    x: free.x + w,
                    y: free.y,
                    width: free.width - w,
                    height: h,
                });
            }
        } else {
            // Right remainder
            if free.width - w > 0 {
                self.free_rects.push(Rect {
                    x: free.x + w,
                    y: free.y,
                    width: free.width - w,
                    height: free.height,
                });
            }
            // Bottom remainder
            if free.height - h > 0 {
                self.free_rects.push(Rect {
                    x: free.x,
                    y: free.y + h,
                    width: w,
                    height: free.height - h,
                });
            }
        }

        Some((placed.x, placed.y))
    }
}

/// Pack bitmaps into atlas canvases. Consumes the input vec.
pub fn pack_textures(mut bitmaps: Vec<Bitmap>) -> Option<Vec<PackedCanvas>> {
    // Sort by area (smallest first, pack from back)
    bitmaps.sort_by_key(|b| b.width * b.height);

    let mut canvases = Vec::new();

    while !bitmaps.is_empty() {
        let mut packer = GuillotinePacker::new(MAX_TEXTURE, MAX_TEXTURE);
        let mut canvas = PackedCanvas {
            width: MAX_TEXTURE,
            height: MAX_TEXTURE,
            bitmaps: Vec::new(),
        };

        let mut max_x = 0;
        let mut max_y = 0;

        while let Some(bitmap) = bitmaps.last() {
            if let Some((px, py)) = packer.insert(bitmap.width, bitmap.height) {
                let mut b = bitmaps.pop().unwrap();
                b.pack_x = px;
                b.pack_y = py;
                max_x = max_x.max(px + b.width);
                max_y = max_y.max(py + b.height);
                canvas.bitmaps.push(b);
            } else {
                break;
            }
        }

        if canvas.bitmaps.is_empty() {
            return None; // Can't fit even one bitmap
        }

        // Shrink canvas to power-of-two that fits
        while canvas.width / 2 >= max_x {
            canvas.width /= 2;
        }
        while canvas.height / 2 >= max_y {
            canvas.height /= 2;
        }

        canvases.push(canvas);
    }

    Some(canvases)
}
