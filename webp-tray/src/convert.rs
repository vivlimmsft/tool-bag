use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use image::{DynamicImage, GenericImageView, ImageFormat};

use crate::config::Config;
use crate::notify;
use crate::sanitize::sanitize_stem;

#[derive(Debug, Clone, Copy)]
pub enum OutputFormat {
    Png,
    Jpeg,
}

impl OutputFormat {
    fn ext(self) -> &'static str {
        match self {
            OutputFormat::Png => "png",
            OutputFormat::Jpeg => "jpg",
        }
    }
    fn label(self) -> &'static str {
        match self {
            OutputFormat::Png => "PNG",
            OutputFormat::Jpeg => "JPEG",
        }
    }
}

pub fn convert_one(path: &Path, cfg: &Config) -> Result<()> {
    let img = image::open(path).with_context(|| format!("decode {}", path.display()))?;
    let (w, h) = img.dimensions();
    let pixels = (w as u64) * (h as u64);

    // only scan pixels when there's an alpha channel to potentially scan.
    let has_real_alpha = if img.color().has_alpha() {
        has_transparent_pixels(&img, cfg.alpha_threshold, cfg.transparency_pixel_fraction)
    } else {
        false
    };

    let small = pixels <= cfg.small_image_max_pixels as u64;
    let format = if has_real_alpha || small {
        OutputFormat::Png
    } else {
        OutputFormat::Jpeg
    };

    let out_path = pick_output_path(path, format.ext())?;
    write_image(&img, &out_path, format, cfg.jpeg_quality)?;

    // recycle the original .webp; never hard-delete.
    trash::delete(path).with_context(|| format!("recycle {}", path.display()))?;

    log::info!(
        "{}x{} ({}px), alpha={} -> {} ({})",
        w,
        h,
        pixels,
        has_real_alpha,
        out_path.display(),
        format.label()
    );

    if cfg.notifications {
        notify_toast(path, &out_path, format);
    }
    Ok(())
}

fn write_image(img: &DynamicImage, out: &Path, fmt: OutputFormat, jpeg_quality: u8) -> Result<()> {
    // Atomic write: encode into a sibling temp file, then rename onto the
    // final path. Same-volume rename on Windows is atomic — readers either
    // see the old name (which doesn't exist) or the fully-written new file,
    // never a half-encoded image.
    let parent = out.parent().context("output has no parent dir")?;
    let stem = out
        .file_name()
        .context("output has no file name")?
        .to_string_lossy()
        .into_owned();
    let tmp = parent.join(format!(".{stem}.webp-tray.tmp"));

    // Ensure no stale temp file is hanging around (e.g. from a prior crash).
    let _ = std::fs::remove_file(&tmp);

    let res: Result<()> = (|| {
        match fmt {
            OutputFormat::Png => {
                img.save_with_format(&tmp, ImageFormat::Png)
                    .with_context(|| format!("write png {}", tmp.display()))?;
            }
            OutputFormat::Jpeg => {
                // jpeg has no alpha; drop it explicitly to avoid surprises.
                let rgb_owned;
                let to_encode: &DynamicImage = if img.color().has_alpha() {
                    rgb_owned = DynamicImage::ImageRgb8(img.to_rgb8());
                    &rgb_owned
                } else {
                    img
                };
                let f = std::fs::File::create(&tmp)?;
                let mut bw = std::io::BufWriter::new(f);
                let mut enc =
                    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bw, jpeg_quality);
                enc.encode_image(to_encode)
                    .with_context(|| format!("write jpeg {}", tmp.display()))?;
            }
        }
        Ok(())
    })();

    if let Err(e) = res {
        // Best-effort cleanup of the partial temp; propagate the original error.
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }

    std::fs::rename(&tmp, out)
        .with_context(|| format!("rename {} -> {}", tmp.display(), out.display()))?;
    Ok(())
}

fn pick_output_path(src: &Path, ext: &str) -> Result<PathBuf> {
    let raw_stem = src
        .file_stem()
        .context("no file stem")?
        .to_string_lossy()
        .into_owned();
    let stem = sanitize_stem(&raw_stem);
    let parent = src.parent().context("no parent dir")?;

    let mut candidate = parent.join(format!("{stem}.{ext}"));
    if !candidate.exists() && candidate != src {
        return Ok(candidate);
    }
    // disambiguate; never clobber an existing file (or the source itself).
    for n in 1..1000 {
        candidate = parent.join(format!("{stem} ({n}).{ext}"));
        if !candidate.exists() && candidate != src {
            return Ok(candidate);
        }
    }
    bail!("could not find a free filename next to {}", src.display());
}

fn has_transparent_pixels(img: &DynamicImage, alpha_thresh: u8, frac: f32) -> bool {
    // matching directly avoids an extra full-image conversion when we can read alpha in place.
    fn scan<I: Iterator<Item = u8>>(alphas: I, total: usize, frac: f32, thresh: u8) -> bool {
        if total == 0 {
            return false;
        }
        let needed = ((total as f32) * frac).ceil().max(1.0) as usize;
        let mut count = 0usize;
        for a in alphas {
            if a < thresh {
                count += 1;
                if count >= needed {
                    return true;
                }
            }
        }
        false
    }
    match img {
        DynamicImage::ImageRgba8(b) => {
            let total = (b.width() as usize) * (b.height() as usize);
            scan(
                b.pixels().map(|p| p.0[3]),
                total,
                frac,
                thresh_or(alpha_thresh),
            )
        }
        DynamicImage::ImageLumaA8(b) => {
            let total = (b.width() as usize) * (b.height() as usize);
            scan(
                b.pixels().map(|p| p.0[1]),
                total,
                frac,
                thresh_or(alpha_thresh),
            )
        }
        DynamicImage::ImageRgba16(b) => {
            let total = (b.width() as usize) * (b.height() as usize);
            let t = thresh_or(alpha_thresh);
            // 16->8 by truncating high byte.
            scan(b.pixels().map(|p| (p.0[3] >> 8) as u8), total, frac, t)
        }
        DynamicImage::ImageLumaA16(b) => {
            let total = (b.width() as usize) * (b.height() as usize);
            let t = thresh_or(alpha_thresh);
            scan(b.pixels().map(|p| (p.0[1] >> 8) as u8), total, frac, t)
        }
        DynamicImage::ImageRgba32F(b) => {
            let total = (b.width() as usize) * (b.height() as usize);
            let t_f = (thresh_or(alpha_thresh) as f32) / 255.0;
            // can't reuse scan() directly because of float threshold; inline.
            if total == 0 {
                return false;
            }
            let needed = ((total as f32) * frac).ceil().max(1.0) as usize;
            let mut count = 0usize;
            for p in b.pixels() {
                if p.0[3] < t_f {
                    count += 1;
                    if count >= needed {
                        return true;
                    }
                }
            }
            false
        }
        // formats without alpha (we already filtered, but be safe).
        _ => false,
    }
}

#[inline]
fn thresh_or(t: u8) -> u8 {
    if t == 0 {
        1
    } else {
        t
    }
}

fn notify_toast(orig: &Path, new: &Path, fmt: OutputFormat) {
    let body = format!(
        "{}  →  {}",
        orig.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        new.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    );
    notify::info_toast(&format!("Converted to {}", fmt.label()), &body);
}
