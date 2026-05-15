use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// images at or below this many pixels (w*h) are emitted as PNG even when fully opaque.
    /// PNG keeps small UI/sprite/icon-style assets crisp; JPEG artifacts are most visible at small sizes.
    pub small_image_max_pixels: u32,
    /// JPEG output quality (1-100).
    pub jpeg_quality: u8,
    /// debounce ms for filesystem events. browsers may emit several events while a file lands.
    pub debounce_ms: u64,
    /// show Windows toast notifications when a file is converted.
    pub notifications: bool,
    /// also watch subdirectories of Downloads.
    pub recursive: bool,
    /// pixels with alpha < this value count as transparent. tolerates near-opaque rounding noise.
    pub alpha_threshold: u8,
    /// fraction of pixels that must be transparent to keep PNG (0.0 - 1.0).
    /// very small values (e.g. 0.001) effectively mean "any real transparency forces PNG".
    pub transparency_pixel_fraction: f32,
    /// override Downloads location. leave empty to use the OS-configured Downloads folder.
    pub downloads_override: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            small_image_max_pixels: 480_000, // ~800x600
            jpeg_quality: 90,
            debounce_ms: 750,
            notifications: true,
            recursive: false,
            alpha_threshold: 250,
            transparency_pixel_fraction: 0.001,
            downloads_override: String::new(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let s =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut cfg: Config =
            toml::from_str(&s).with_context(|| format!("parsing {}", path.display()))?;
        cfg.validate_and_clamp();
        Ok(cfg)
    }

    /// Coerce out-of-range values into safe ranges, logging a warning for
    /// each thing we touched. We don't bail on bad config — keeping the app
    /// running is usually more useful than refusing to load.
    fn validate_and_clamp(&mut self) {
        if self.jpeg_quality < 1 || self.jpeg_quality > 100 {
            log::warn!(
                "jpeg_quality {} out of range; clamping to 1..=100",
                self.jpeg_quality
            );
            self.jpeg_quality = self.jpeg_quality.clamp(1, 100);
        }
        if !(0.0..=1.0).contains(&self.transparency_pixel_fraction) {
            log::warn!(
                "transparency_pixel_fraction {} out of range; clamping to 0.0..=1.0",
                self.transparency_pixel_fraction
            );
            self.transparency_pixel_fraction = self.transparency_pixel_fraction.clamp(0.0, 1.0);
        }
        if self.debounce_ms < 50 {
            log::warn!("debounce_ms {} too low; clamping to 50", self.debounce_ms);
            self.debounce_ms = 50;
        }
    }
}

pub fn config_path() -> Result<PathBuf> {
    let dir = dirs::config_dir()
        .context("no config dir available")?
        .join("webp-tray");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("config.toml"))
}

pub fn ensure_default(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    let default = r#"# webp-tray configuration
# images at or below this many pixels become PNG even when opaque
small_image_max_pixels = 480000
# JPEG output quality (1-100)
jpeg_quality = 90
# debounce delay for file events (ms)
debounce_ms = 750
# show Windows toast notifications
notifications = true
# also watch subdirectories of Downloads
recursive = false
# pixels with alpha < this count as transparent (0-255)
alpha_threshold = 250
# fraction of transparent pixels needed to force PNG (0.0-1.0)
transparency_pixel_fraction = 0.001
# leave empty to auto-detect via the OS-configured Downloads folder.
# example: downloads_override = "D:/Downloads"
downloads_override = ""
"#;
    std::fs::write(path, default)?;
    Ok(())
}
