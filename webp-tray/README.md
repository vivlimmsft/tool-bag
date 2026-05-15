# webp-tray

A tiny Windows system tray app that watches your Downloads folder and converts
new `.webp` files into PNG (with transparency / for small images) or JPEG (for
larger opaque images), then sends the original to the Recycle Bin.

## Features

- Resolves the real Downloads folder via `SHGetKnownFolderPath`, so a relocated
  Downloads folder (e.g. on `D:\`) is picked up automatically.
- Uses `ReadDirectoryChangesW` (via the `notify` crate) — no polling.
- Heuristic format choice:
  - has a meaningful amount of transparency? → **PNG**
  - small image (≤ `small_image_max_pixels`)?  → **PNG**
  - otherwise → **JPEG**
- TOML config at `%APPDATA%\webp-tray\config.toml`. Live-reloaded when changed.
- System tray menu with "Edit config…" that opens the file in your default
  editor.
- Optional Windows toast notifications when a conversion happens.
- The original `.webp` goes to the Recycle Bin (never hard-deleted).

## Build

```powershell
cd misc\webp-tray
cargo build --release
```

The release binary lives at `target\release\webp-tray.exe`. To autorun on login,
drop a shortcut into `shell:startup`.

## Configuration

```toml
small_image_max_pixels = 480000
jpeg_quality = 90
debounce_ms = 750
notifications = true
recursive = false
alpha_threshold = 250
transparency_pixel_fraction = 0.001
downloads_override = ""    # set to override the OS Downloads folder
```

Edit the file and changes apply within ~1 second — no restart needed.
