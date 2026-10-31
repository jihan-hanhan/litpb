//! Core conversion: image -> LittleTiles blueprint SNBT.

use crate::snbt::Snbt;
use image::imageops::FilterType;

/// Warn when the resized image has at least this many cells.
pub const WARN_CELLS: u64 = 10000;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Resampling {
    Nearest,
    Bilinear,
    Bicubic,
    Lanczos,
}

impl Resampling {
    pub const ALL: [Self; 4] = [
        Self::Nearest,
        Self::Bilinear,
        Self::Bicubic,
        Self::Lanczos,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Nearest => "nearest",
            Self::Bilinear => "bilinear",
            Self::Bicubic => "bicubic",
            Self::Lanczos => "lanczos",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|r| r.name() == s)
    }

    fn filter(self) -> FilterType {
        match self {
            Self::Nearest => FilterType::Nearest,
            Self::Bilinear => FilterType::Triangle,
            Self::Bicubic => FilterType::CatmullRom,
            Self::Lanczos => FilterType::Lanczos3,
        }
    }
}

pub struct Options {
    pub scale: f64,
    pub grid: i32,
    pub block: String,
    pub resampling: Resampling,
    /// Merge adjacent tiles with identical RGBA color into larger bBoxes
    /// (greedy meshing). Reduces tile count on flat-color images.
    pub merge: bool,
    /// Posterize RGB channels to multiples of this step (0/1 = off), so
    /// similar colors collapse into identical ones. Alpha is kept exact.
    pub quantize: u8,
}

pub struct Converted {
    pub snbt: Snbt,
    pub cells: u64,
    pub tiles: i32,
    pub width: u32,
    pub height: u32,
}

pub fn validate_grid(grid: i32) -> bool {
    (grid == 1 || grid % 4 == 0) && (1..=32).contains(&grid)
}

/// Target dimensions after scaling, matching Python's int() truncation,
/// clamped to a minimum of 1.
pub fn scaled_dims(width: u32, height: u32, scale: f64) -> (u32, u32) {
    (
        ((width as f64 * scale) as u32).max(1),
        ((height as f64 * scale) as u32).max(1),
    )
}

/// Round each RGB channel to the nearest multiple of `step` (clamped to
/// 255). Alpha is left untouched. Makes similar colors identical, which
/// greatly improves merge rates on smooth/anti-aliased artwork.
fn posterize(img: &mut image::RgbaImage, step: u8) {
    if step <= 1 {
        return;
    }
    let s = step as u32;
    for p in img.pixels_mut() {
        for c in 0..3 {
            let v = p.0[c] as u32;
            p.0[c] = ((v + s / 2) / s * s).min(255) as u8;
        }
    }
}

fn make_tile(bbox: [u32; 6], color: i32, block: &str) -> Snbt {
    Snbt::Compound(vec![
        (
            "bBox".to_string(),
            Snbt::IntArray(bbox.map(|v| v as i32).to_vec()),
        ),
        (
            "tile".to_string(),
            Snbt::Compound(vec![
                ("color".to_string(), Snbt::Int(color)),
                ("block".to_string(), Snbt::Str(block.to_string())),
            ]),
        ),
    ])
}

/// Convert an image file to a blueprint. `progress(done, cells)` is called
/// periodically (and always once at the end).
pub fn img_to_snbt(
    input: &str,
    opts: &Options,
    mut progress: impl FnMut(u64, u64),
) -> Result<Converted, String> {
    let img = image::open(input)
        .map_err(|e| format!("Cannot open image {input}: {e}"))?
        .to_rgba8();

    let (new_width, new_height) = scaled_dims(img.width(), img.height(), opts.scale);
    let mut img = image::imageops::resize(&img, new_width, new_height, opts.resampling.filter());
    posterize(&mut img, opts.quantize);
    let (width, height) = (img.width(), img.height());

    let cells = width as u64 * height as u64;
    let idx = |x: u32, z: u32| (x * height + z) as usize;

    // Scan pixels into a color grid (None = fully transparent, skipped)
    let mut colors: Vec<Option<i32>> = vec![None; (width * height) as usize];
    let mut done: u64 = 0;
    let bar_step = (cells / 1000).max(1);
    for x in 0..width {
        for z in 0..height {
            let [r, g, b, a] = img.get_pixel(x, z).0;
            done += 1;
            if a != 0 {
                // ARGB packed into a signed i32, same as the Python version
                colors[idx(x, z)] =
                    Some(((a as u32) << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32) as i32);
            }
            if done % bar_step == 0 || done == cells {
                progress(done, cells);
            }
        }
    }

    let mut tiles: Vec<Snbt> = Vec::new();
    if opts.merge {
        // Greedy meshing: from each unclaimed cell, grow a run along +z,
        // then widen along +x while the whole z-span matches.
        let mut claimed = vec![false; (width * height) as usize];
        for x in 0..width {
            for z in 0..height {
                let Some(color) = colors[idx(x, z)] else { continue };
                if claimed[idx(x, z)] {
                    continue;
                }
                let mut z2 = z + 1;
                while z2 < height && !claimed[idx(x, z2)] && colors[idx(x, z2)] == Some(color) {
                    z2 += 1;
                }
                let mut x2 = x + 1;
                'expand: while x2 < width {
                    for zz in z..z2 {
                        if claimed[idx(x2, zz)] || colors[idx(x2, zz)] != Some(color) {
                            break 'expand;
                        }
                    }
                    x2 += 1;
                }
                for xx in x..x2 {
                    for zz in z..z2 {
                        claimed[idx(xx, zz)] = true;
                    }
                }
                tiles.push(make_tile([x, 0, z, x2, 1, z2], color, &opts.block));
            }
        }
    } else {
        for x in 0..width {
            for z in 0..height {
                if let Some(color) = colors[idx(x, z)] {
                    tiles.push(make_tile([x, 0, z, x + 1, 1, z + 1], color, &opts.block));
                }
            }
        }
    }
    let count = tiles.len() as i32;

    let snbt = Snbt::Compound(vec![
        ("tiles".to_string(), Snbt::List(tiles)),
        ("min".to_string(), Snbt::IntArray(vec![0, 0, 0])),
        (
            "size".to_string(),
            Snbt::IntArray(vec![width as i32, 1, height as i32]),
        ),
        ("grid".to_string(), Snbt::Int(opts.grid)),
        ("count".to_string(), Snbt::Int(count)),
    ]);

    Ok(Converted {
        snbt,
        cells,
        tiles: count,
        width,
        height,
    })
}
