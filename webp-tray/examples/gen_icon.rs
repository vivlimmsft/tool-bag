//! Generates a multi-resolution Windows .ico file from `assets/icon.png`.
//! Run once with `cargo run --example gen_icon`. The output is committed
//! so neither `build.rs` nor the WiX installer needs to regenerate it on
//! every build.

use std::path::PathBuf;

use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::imageops::FilterType;
use image::{ExtendedColorType, ImageFormat, ImageReader};

const SIZES: &[u32] = &[16, 24, 32, 48, 64, 128, 256];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("assets").join("icon.png");
    let dst = manifest_dir.join("assets").join("webp-tray.ico");

    println!("reading {}", src.display());
    let img = ImageReader::open(&src)?
        .with_guessed_format()?
        .decode()?
        .into_rgba8();

    // Pre-encode each size as a PNG payload, which the ICO container accepts
    // directly. Larger sizes (>= 256) MUST be PNG-encoded inside the ICO;
    // older sub-256 sizes can also be PNG and Windows handles it fine since
    // Vista. This keeps the file small.
    let mut frames = Vec::with_capacity(SIZES.len());
    let mut buffers: Vec<Vec<u8>> = Vec::with_capacity(SIZES.len());
    for &size in SIZES {
        let resized = image::imageops::resize(&img, size, size, FilterType::Lanczos3);
        let mut buf = Vec::new();
        resized.write_to(&mut std::io::Cursor::new(&mut buf), ImageFormat::Png)?;
        buffers.push(buf);
    }
    for (i, &size) in SIZES.iter().enumerate() {
        // ICO size field uses 0 to mean "256". For the encoded payload's
        // declared dimensions we still pass the real size; the size byte in
        // the directory entry is what wraps to 0 for 256.
        frames.push(IcoFrame::with_encoded(
            &buffers[i],
            size,
            size,
            ExtendedColorType::Rgba8,
        )?);
    }

    let out = std::fs::File::create(&dst)?;
    let writer = std::io::BufWriter::new(out);
    let enc = IcoEncoder::new(writer);
    enc.encode_images(&frames)?;
    println!(
        "wrote {} ({} bytes, {} frames)",
        dst.display(),
        std::fs::metadata(&dst)?.len(),
        SIZES.len()
    );
    Ok(())
}
