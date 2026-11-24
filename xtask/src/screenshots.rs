//! README screenshots: export synthetic native captures, then write small PNGs.
//!
//! The UI uses a few thousand colors, mostly flat fills and anti-aliased text.
//! A 256-color palette without dithering keeps flat areas exact and moves only
//! edge pixels by a few levels, so the PNGs shrink several times over.
use std::{collections::HashMap, fs, path::Path, process::Command};
use windows::{
    Win32::{
        Foundation::GENERIC_WRITE,
        Graphics::Imaging::{
            CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat8bppIndexed,
            IWICImagingFactory, WICBitmapEncoderNoCache,
        },
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        },
    },
    core::HSTRING,
};

pub const PAGES: [&str; 5] = ["today", "history", "targets", "settings", "compact"];

pub fn export(root: &Path, offline: bool) -> Result<(), String> {
    let mut test = Command::new("cargo");
    test.args(["test", "--lib", "--locked"]);
    if offline {
        test.arg("--offline");
    }
    test.args([
        "native_ui_screenshots_for_readme",
        "--",
        "--ignored",
        "--nocapture",
    ]);
    super::run(&mut test)?;
    let source = root.join("target/readme-screenshots");
    let destination = root.join("docs/screenshots");
    fs::create_dir_all(&destination).map_err(|e| e.to_string())?;
    for page in PAGES {
        let bmp = fs::read(source.join(format!("{page}.bmp"))).map_err(|e| e.to_string())?;
        let image = Image::from_bmp(&bmp)?;
        let (palette, indices) = quantize(&image.pixels);
        let target = destination.join(format!("{page}.png"));
        write_png(&target, image.width, image.height, &palette, &indices)?;
        let bytes = fs::metadata(&target).map_err(|e| e.to_string())?.len();
        let error = max_error(&image.pixels, &palette, &indices);
        println!(
            "{page}.png: {}x{}, {} KB, largest channel change {error}",
            image.width,
            image.height,
            bytes.div_ceil(1024)
        );
    }
    Ok(())
}

#[derive(Debug, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// RGB triples, top row first.
    pub pixels: Vec<[u8; 3]>,
}

impl Image {
    /// Reads the uncompressed 32-bit BMPs the screenshot exporter writes.
    pub fn from_bmp(data: &[u8]) -> Result<Self, String> {
        let u32_at = |at: usize| {
            data.get(at..at + 4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .ok_or("Truncated BMP header")
        };
        if data.get(..2) != Some(b"BM") {
            return Err("Not a BMP file".into());
        }
        let offset = u32_at(10)? as usize;
        let width = u32_at(18)? as i32;
        let height = u32_at(22)? as i32;
        let bits = data.get(28..30).ok_or("Truncated BMP header")?;
        if u16::from_le_bytes([bits[0], bits[1]]) != 32 || width <= 0 || height == 0 {
            return Err("Expected a 32-bit BMP".into());
        }
        let (width, rows) = (width as usize, height.unsigned_abs() as usize);
        let body = data
            .get(offset..offset + width * rows * 4)
            .ok_or("Truncated BMP pixels")?;
        let mut pixels = Vec::with_capacity(width * rows);
        for row in 0..rows {
            // A positive height stores the bottom row first.
            let stored = if height < 0 { row } else { rows - 1 - row };
            let line = &body[stored * width * 4..(stored + 1) * width * 4];
            pixels.extend(line.as_chunks::<4>().0.iter().map(|p| [p[2], p[1], p[0]]));
        }
        Ok(Self {
            width: width as u32,
            height: rows as u32,
            pixels,
        })
    }
}

fn distance(a: [u8; 3], b: [u8; 3]) -> u32 {
    a.iter()
        .zip(b)
        .map(|(&x, y)| (x as i32 - y as i32).pow(2) as u32)
        .sum()
}

/// Returns at most 256 colors and one palette index per pixel. Images with
/// 256 colors or fewer are kept exact. Otherwise a weighted median cut, refined
/// by a few k-means passes, picks the palette, and every pixel takes its
/// nearest entry without dithering, so flat areas stay flat and compress well.
pub fn quantize(pixels: &[[u8; 3]]) -> (Vec<[u8; 3]>, Vec<u8>) {
    let mut counts = HashMap::<[u8; 3], u64>::new();
    for &pixel in pixels {
        *counts.entry(pixel).or_default() += 1;
    }
    let mut colors = counts.into_iter().collect::<Vec<_>>();
    colors.sort_unstable();
    let palette = if colors.len() <= 256 {
        colors.iter().map(|&(c, _)| c).collect()
    } else {
        refine(&colors, median_cut(&colors, 256))
    };
    let mut lookup = HashMap::with_capacity(colors.len());
    for &(color, _) in &colors {
        lookup.insert(color, nearest(&palette, color));
    }
    let indices = pixels.iter().map(|p| lookup[p]).collect();
    (palette, indices)
}

fn nearest(palette: &[[u8; 3]], color: [u8; 3]) -> u8 {
    (0..palette.len())
        .min_by_key(|&i| distance(palette[i], color))
        .unwrap() as u8
}

fn mean(colors: &[([u8; 3], u64)]) -> [u8; 3] {
    let total = colors.iter().map(|&(_, n)| n).sum::<u64>().max(1);
    std::array::from_fn(|channel| {
        let sum = colors
            .iter()
            .map(|&(c, n)| c[channel] as u64 * n)
            .sum::<u64>();
        ((sum + total / 2) / total) as u8
    })
}

fn median_cut(colors: &[([u8; 3], u64)], size: usize) -> Vec<[u8; 3]> {
    let mut boxes = vec![colors.to_vec()];
    while boxes.len() < size {
        // Split the box whose widest channel, weighted by pixel count, is largest.
        let score = |b: &Vec<([u8; 3], u64)>| {
            if b.len() < 2 {
                return (0, 0);
            }
            let (channel, range) = (0..3)
                .map(|ch| {
                    let low = b.iter().map(|(c, _)| c[ch]).min().unwrap();
                    let high = b.iter().map(|(c, _)| c[ch]).max().unwrap();
                    (ch, (high - low) as u64)
                })
                .max_by_key(|&(_, range)| range)
                .unwrap();
            let weight = b.iter().map(|&(_, n)| n).sum::<u64>();
            (range * (weight as f64).sqrt() as u64, channel)
        };
        let (index, (value, channel)) = boxes
            .iter()
            .map(score)
            .enumerate()
            .max_by_key(|&(_, (value, _))| value)
            .unwrap();
        if value == 0 {
            break;
        }
        let mut split = boxes.swap_remove(index);
        split.sort_unstable_by_key(|(c, _)| c[channel]);
        let half = split.iter().map(|&(_, n)| n).sum::<u64>() / 2;
        let mut seen = 0;
        let at = split
            .iter()
            .position(|&(_, n)| {
                seen += n;
                seen >= half
            })
            .unwrap()
            .clamp(1, split.len() - 1);
        let upper = split.split_off(at);
        boxes.push(split);
        boxes.push(upper);
    }
    boxes.iter().map(|b| mean(b)).collect()
}

fn refine(colors: &[([u8; 3], u64)], mut palette: Vec<[u8; 3]>) -> Vec<[u8; 3]> {
    for _ in 0..6 {
        let mut groups = vec![Vec::new(); palette.len()];
        for &entry in colors {
            groups[nearest(&palette, entry.0) as usize].push(entry);
        }
        for (slot, group) in palette.iter_mut().zip(&groups) {
            if !group.is_empty() {
                *slot = mean(group);
            }
        }
    }
    palette
}

pub fn max_error(pixels: &[[u8; 3]], palette: &[[u8; 3]], indices: &[u8]) -> u8 {
    pixels
        .iter()
        .zip(indices)
        .flat_map(|(p, &i)| (0..3).map(move |ch| p[ch].abs_diff(palette[i as usize][ch])))
        .max()
        .unwrap_or(0)
}

fn write_png(
    path: &Path,
    width: u32,
    height: u32,
    palette: &[[u8; 3]],
    indices: &[u8],
) -> Result<(), String> {
    let entries = palette
        .iter()
        .map(|&[r, g, b]| 0xff00_0000 | (r as u32) << 16 | (g as u32) << 8 | b as u32)
        .collect::<Vec<_>>();
    unsafe {
        // S_FALSE (already initialized) is fine; only a mode conflict fails.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| e.to_string())?;
        let result = (|| {
            let stream = factory.CreateStream()?;
            stream.InitializeFromFilename(&HSTRING::from(path.as_os_str()), GENERIC_WRITE.0)?;
            let encoder = factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
            encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
            let (mut frame, mut options) = (None, None);
            encoder.CreateNewFrame(&mut frame, &mut options)?;
            let frame = frame.unwrap();
            frame.Initialize(options.as_ref())?;
            frame.SetSize(width, height)?;
            let mut format = GUID_WICPixelFormat8bppIndexed;
            frame.SetPixelFormat(&mut format)?;
            if format != GUID_WICPixelFormat8bppIndexed {
                return Err(windows::core::Error::empty());
            }
            let wic_palette = factory.CreatePalette()?;
            wic_palette.InitializeCustom(&entries)?;
            frame.SetPalette(&wic_palette)?;
            frame.WritePixels(height, width, indices)?;
            frame.Commit()?;
            encoder.Commit()
        })();
        result.map_err(|e| format!("Could not write {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bmp(width: i32, height: i32, rows: &[[[u8; 3]; 2]]) -> Vec<u8> {
        let mut data = b"BM".to_vec();
        data.extend([0; 8]);
        data.extend(54u32.to_le_bytes());
        data.extend(40u32.to_le_bytes());
        data.extend(width.to_le_bytes());
        data.extend(height.to_le_bytes());
        data.extend(1u16.to_le_bytes());
        data.extend(32u16.to_le_bytes());
        data.extend([0; 24]);
        for row in rows {
            for [r, g, b] in row {
                data.extend([*b, *g, *r, 0]);
            }
        }
        data
    }

    #[test]
    fn bmp_rows_are_read_top_first_in_either_storage_order() {
        let (top, bottom) = ([[1, 2, 3], [4, 5, 6]], [[7, 8, 9], [10, 11, 12]]);
        let expected = [top, bottom].concat();
        let top_down = Image::from_bmp(&bmp(2, -2, &[top, bottom])).unwrap();
        let bottom_up = Image::from_bmp(&bmp(2, 2, &[bottom, top])).unwrap();
        assert_eq!(top_down.pixels, expected);
        assert_eq!(bottom_up, top_down);
        assert_eq!((top_down.width, top_down.height), (2, 2));
        assert!(Image::from_bmp(b"PNG").is_err());
        assert!(Image::from_bmp(&bmp(2, -2, &[top])).is_err());
    }

    #[test]
    fn few_colors_stay_exact() {
        let pixels = (0..1000u32)
            .map(|i| [(i % 200) as u8, 7, 9])
            .collect::<Vec<_>>();
        let (palette, indices) = quantize(&pixels);
        assert_eq!(palette.len(), 200);
        assert_eq!(max_error(&pixels, &palette, &indices), 0);
    }

    #[test]
    fn many_colors_fit_a_palette_with_small_error_and_exact_dominant_colors() {
        // A flat background, a frequent accent and a smooth gradient.
        let mut pixels = vec![[250, 250, 250]; 50_000];
        pixels.extend(vec![[20, 120, 200]; 10_000]);
        pixels.extend((0..4096u32).map(|i| [(i % 64 * 4) as u8, (i / 64 * 4) as u8, 128]));
        let (palette, indices) = quantize(&pixels);
        assert!(palette.len() <= 256);
        assert_eq!(palette[indices[0] as usize], [250, 250, 250]);
        assert_eq!(palette[indices[50_000] as usize], [20, 120, 200]);
        assert!(max_error(&pixels, &palette, &indices) <= 24);
    }
}
