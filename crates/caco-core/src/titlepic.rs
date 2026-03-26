//! TITLEPIC lump extraction and Doom patch format decoding.
//!
//! Extracts the TITLEPIC lump from WAD files and decodes it to RGBA pixels.
//! Supports both Doom patch format and PNG-encoded TITLEPICs.

use std::path::Path;

use std::sync::LazyLock;

use crate::utils::{load_wad_data, parse_wad_directory};

/// Pre-computed grayscale palette (256 × RGB = 768 bytes).
static GRAYSCALE_PALETTE: LazyLock<Vec<u8>> =
    LazyLock::new(|| (0..=255u8).flat_map(|i| [i, i, i]).collect());

/// RGBA image extracted from a WAD's TITLEPIC lump.
pub struct TitlepicImage {
    pub width: u32,
    pub height: u32,
    /// RGBA pixels (4 bytes per pixel, row-major).
    pub pixels: Vec<u8>,
}

/// Extract the TITLEPIC from a WAD file on disk.
///
/// Handles ZIP-wrapped WADs via `load_wad_data()`.
pub fn extract_titlepic(wad_path: &Path) -> Option<TitlepicImage> {
    let data = load_wad_data(wad_path)?;
    extract_titlepic_from_data(&data)
}

/// Extract the TITLEPIC from raw WAD bytes.
pub fn extract_titlepic_from_data(wad_data: &[u8]) -> Option<TitlepicImage> {
    let directory = parse_wad_directory(wad_data);

    // Find TITLEPIC lump
    let (_, offset, size) = directory.iter().find(|(name, _, _)| name == "TITLEPIC")?;
    let offset = *offset as usize;
    let size = *size as usize;

    if offset + size > wad_data.len() || size == 0 {
        return None;
    }

    let lump_data = &wad_data[offset..offset + size];

    // Try PNG first (some modern WADs use PNG-encoded lumps)
    if lump_data.len() >= 8 && &lump_data[..4] == b"\x89PNG" {
        return decode_png(lump_data);
    }

    // Doom patch format — also need PLAYPAL for colors
    let palette = find_playpal(&directory, wad_data);
    decode_doom_patch(lump_data, palette.as_deref())
}

/// PNG magic → decode via image crate.
fn decode_png(data: &[u8]) -> Option<TitlepicImage> {
    let img = image::load_from_memory(data).ok()?;
    let rgba = img.to_rgba8();
    Some(TitlepicImage {
        width: rgba.width(),
        height: rgba.height(),
        pixels: rgba.into_raw(),
    })
}

/// Find the PLAYPAL lump and return its 768-byte palette (256 × RGB).
fn find_playpal(directory: &[(String, u32, u32)], wad_data: &[u8]) -> Option<Vec<u8>> {
    let (_, offset, size) = directory.iter().find(|(name, _, _)| name == "PLAYPAL")?;
    let offset = *offset as usize;
    let size = *size as usize;

    // PLAYPAL is 256 * 3 = 768 bytes minimum (often 14 palettes × 768)
    if size < 768 || offset + 768 > wad_data.len() {
        return None;
    }

    Some(wad_data[offset..offset + 768].to_vec())
}

/// Decode a Doom column-based patch format image.
///
/// Format:
/// - Header: width(u16 LE), height(u16 LE), left_offset(i16 LE), top_offset(i16 LE)
/// - Column offsets: width × u32 LE
/// - Columns: sequence of posts (top_delta u8, length u8, pad 1, pixels `length`, pad 1), ends 0xFF
fn decode_doom_patch(data: &[u8], palette: Option<&[u8]>) -> Option<TitlepicImage> {
    if data.len() < 8 {
        return None;
    }

    let width = u16::from_le_bytes([data[0], data[1]]) as u32;
    let height = u16::from_le_bytes([data[2], data[3]]) as u32;

    // Sanity checks
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return None;
    }

    let col_offsets_start = 8; // after width(2) + height(2) + offsets(2+2)
    let col_offsets_end = col_offsets_start + (width as usize) * 4;
    if col_offsets_end > data.len() {
        return None;
    }

    // Read column offsets
    let mut col_offsets = Vec::with_capacity(width as usize);
    for i in 0..width as usize {
        let off = col_offsets_start + i * 4;
        let col_off = u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        col_offsets.push(col_off as usize);
    }

    // Allocate output (transparent black)
    let pixel_count = (width * height) as usize;
    let mut pixels = vec![0u8; pixel_count * 4];

    let pal = palette.unwrap_or(&GRAYSCALE_PALETTE);

    // Decode each column
    for (x, &col_off) in col_offsets.iter().enumerate() {
        if col_off >= data.len() {
            continue;
        }

        let mut pos = col_off;
        loop {
            if pos >= data.len() {
                break;
            }

            let top_delta = data[pos];
            if top_delta == 0xFF {
                break;
            }
            pos += 1;

            if pos >= data.len() {
                break;
            }
            let length = data[pos] as usize;
            pos += 1;

            // Skip padding byte
            pos += 1;

            // Read pixel data
            for i in 0..length {
                if pos >= data.len() {
                    break;
                }

                let y = top_delta as usize + i;
                if y < height as usize {
                    let palette_idx = data[pos] as usize;
                    let pixel_offset = (y * width as usize + x) * 4;

                    let r = pal.get(palette_idx * 3).copied().unwrap_or(0);
                    let g = pal.get(palette_idx * 3 + 1).copied().unwrap_or(0);
                    let b = pal.get(palette_idx * 3 + 2).copied().unwrap_or(0);

                    pixels[pixel_offset] = r;
                    pixels[pixel_offset + 1] = g;
                    pixels[pixel_offset + 2] = b;
                    pixels[pixel_offset + 3] = 255; // opaque
                }
                pos += 1;
            }

            // Skip padding byte
            pos += 1;
        }
    }

    Some(TitlepicImage {
        width,
        height,
        pixels,
    })
}

/// Decode a Doom flat (raw 64×64 or 320×200 pixel data).
///
/// Flats are simpler than patches: just raw palette indices, no posts/columns.
/// Used as a fallback if patch decoding produces nothing sensible for full-screen images.
#[allow(dead_code)]
fn decode_doom_flat(data: &[u8], palette: Option<&[u8]>) -> Option<TitlepicImage> {
    // Standard Doom TITLEPIC is often stored as a 320×200 raw flat
    let (width, height) = if data.len() == 320 * 200 {
        (320u32, 200u32)
    } else if data.len() == 64 * 64 {
        (64u32, 64u32)
    } else {
        return None;
    };

    let pal = palette.unwrap_or(&GRAYSCALE_PALETTE);

    let mut pixels = Vec::with_capacity(data.len() * 4);
    for &idx in data {
        let i = idx as usize;
        pixels.push(pal.get(i * 3).copied().unwrap_or(0));
        pixels.push(pal.get(i * 3 + 1).copied().unwrap_or(0));
        pixels.push(pal.get(i * 3 + 2).copied().unwrap_or(0));
        pixels.push(255);
    }

    Some(TitlepicImage {
        width,
        height,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal WAD with given lumps. Each lump is (name, data).
    fn build_wad(lumps: &[(&str, &[u8])]) -> Vec<u8> {
        let mut wad = Vec::new();
        let num_lumps = lumps.len() as i32;

        // Header placeholder (will fill in after)
        wad.extend_from_slice(b"PWAD");
        wad.extend_from_slice(&num_lumps.to_le_bytes());
        wad.extend_from_slice(&0i32.to_le_bytes()); // dir_offset placeholder

        // Write lump data, tracking offsets
        let mut lump_info: Vec<(u32, u32, &str)> = Vec::new();
        for (name, data) in lumps {
            let offset = wad.len() as u32;
            let size = data.len() as u32;
            wad.extend_from_slice(data);
            lump_info.push((offset, size, name));
        }

        // Write directory
        let dir_offset = wad.len() as i32;
        for (offset, size, name) in &lump_info {
            wad.extend_from_slice(&offset.to_le_bytes());
            wad.extend_from_slice(&size.to_le_bytes());
            let mut name_bytes = [0u8; 8];
            let name_upper = name.to_uppercase();
            let src = name_upper.as_bytes();
            let len = src.len().min(8);
            name_bytes[..len].copy_from_slice(&src[..len]);
            wad.extend_from_slice(&name_bytes);
        }

        // Patch dir_offset in header
        let offset_bytes = dir_offset.to_le_bytes();
        wad[8..12].copy_from_slice(&offset_bytes);

        wad
    }

    /// Build a minimal Doom patch (1×1 pixel).
    fn build_patch_1x1(palette_index: u8) -> Vec<u8> {
        let mut data = Vec::new();
        // Header: width=1, height=1, left=0, top=0
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&0i16.to_le_bytes());
        data.extend_from_slice(&0i16.to_le_bytes());

        // Column offset (points to position 12)
        data.extend_from_slice(&12u32.to_le_bytes());

        // Column data: one post
        data.push(0); // top_delta = 0
        data.push(1); // length = 1
        data.push(0); // pad
        data.push(palette_index); // pixel
        data.push(0); // pad
        data.push(0xFF); // end of column

        data
    }

    /// Build a simple RGB palette (all entries same color).
    fn build_palette(r: u8, g: u8, b: u8) -> Vec<u8> {
        let mut pal = Vec::with_capacity(768);
        for _ in 0..256 {
            pal.push(r);
            pal.push(g);
            pal.push(b);
        }
        pal
    }

    #[test]
    fn test_extract_titlepic_patch_format() {
        let patch = build_patch_1x1(42);
        let palette = build_palette(0xFF, 0x00, 0x33);
        let wad = build_wad(&[("PLAYPAL", &palette), ("TITLEPIC", &patch)]);

        let result = extract_titlepic_from_data(&wad);
        assert!(result.is_some());

        let img = result.unwrap();
        assert_eq!(img.width, 1);
        assert_eq!(img.height, 1);
        assert_eq!(img.pixels.len(), 4);
        // Palette index 42 → all entries are (0xFF, 0x00, 0x33)
        assert_eq!(img.pixels[0], 0xFF);
        assert_eq!(img.pixels[1], 0x00);
        assert_eq!(img.pixels[2], 0x33);
        assert_eq!(img.pixels[3], 0xFF); // alpha
    }

    #[test]
    fn test_extract_titlepic_no_palette_uses_grayscale() {
        let patch = build_patch_1x1(128);
        let wad = build_wad(&[("TITLEPIC", &patch)]);

        let result = extract_titlepic_from_data(&wad);
        assert!(result.is_some());

        let img = result.unwrap();
        assert_eq!(img.width, 1);
        assert_eq!(img.height, 1);
        // Grayscale: index 128 → (128, 128, 128)
        assert_eq!(img.pixels[0], 128);
        assert_eq!(img.pixels[1], 128);
        assert_eq!(img.pixels[2], 128);
        assert_eq!(img.pixels[3], 255);
    }

    #[test]
    fn test_extract_titlepic_missing_lump() {
        let wad = build_wad(&[("PLAYPAL", &build_palette(0, 0, 0))]);
        assert!(extract_titlepic_from_data(&wad).is_none());
    }

    #[test]
    fn test_extract_titlepic_empty_wad() {
        assert!(extract_titlepic_from_data(&[]).is_none());
        assert!(extract_titlepic_from_data(b"PWAD").is_none());
    }

    #[test]
    fn test_extract_titlepic_png_format() {
        // Build a minimal 1×1 red PNG using the image crate
        let mut png_data = Vec::new();
        {
            let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
            let mut cursor = std::io::Cursor::new(&mut png_data);
            img.write_to(&mut cursor, image::ImageFormat::Png).unwrap();
        }

        let wad = build_wad(&[("TITLEPIC", &png_data)]);
        let result = extract_titlepic_from_data(&wad);
        assert!(result.is_some());

        let img = result.unwrap();
        assert_eq!(img.width, 1);
        assert_eq!(img.height, 1);
        assert_eq!(img.pixels[0], 255); // R
        assert_eq!(img.pixels[1], 0);   // G
        assert_eq!(img.pixels[2], 0);   // B
        assert_eq!(img.pixels[3], 255); // A
    }

    #[test]
    fn test_build_patch_2x2() {
        // Build a 2×2 patch with different palette indices per column
        let mut data = Vec::new();
        // Header: width=2, height=2, left=0, top=0
        data.extend_from_slice(&2u16.to_le_bytes());
        data.extend_from_slice(&2u16.to_le_bytes());
        data.extend_from_slice(&0i16.to_le_bytes());
        data.extend_from_slice(&0i16.to_le_bytes());

        // Column offsets (2 columns)
        let col0_off = 8 + 2 * 4; // 16
        data.extend_from_slice(&(col0_off as u32).to_le_bytes());
        // Column 0: post with 2 pixels = 1(top) + 1(len) + 1(pad) + 2(pixels) + 1(pad) + 1(end) = 7
        let col1_off = col0_off + 7;
        data.extend_from_slice(&(col1_off as u32).to_le_bytes());

        // Column 0: one post covering rows 0-1
        data.push(0);  // top_delta
        data.push(2);  // length
        data.push(0);  // pad
        data.push(10); // pixel 0
        data.push(20); // pixel 1
        data.push(0);  // pad
        data.push(0xFF); // end

        // Column 1: one post covering rows 0-1
        data.push(0);  // top_delta
        data.push(2);  // length
        data.push(0);  // pad
        data.push(30); // pixel 0
        data.push(40); // pixel 1
        data.push(0);  // pad
        data.push(0xFF); // end

        let wad = build_wad(&[("TITLEPIC", &data)]);
        let result = extract_titlepic_from_data(&wad);
        assert!(result.is_some());

        let img = result.unwrap();
        assert_eq!(img.width, 2);
        assert_eq!(img.height, 2);
        assert_eq!(img.pixels.len(), 2 * 2 * 4);

        // Row 0: (10,10,10,255), (30,30,30,255) — grayscale fallback
        assert_eq!(img.pixels[0], 10);  // [0,0].r
        assert_eq!(img.pixels[4], 30);  // [0,1].r
        // Row 1: (20,20,20,255), (40,40,40,255)
        assert_eq!(img.pixels[8], 20);  // [1,0].r
        assert_eq!(img.pixels[12], 40); // [1,1].r
    }
}
