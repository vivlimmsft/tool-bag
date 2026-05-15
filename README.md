# tool-bag

A small monorepo of personal tools.

## webp-tray

Windows tray app that watches your Downloads folder and converts new `.webp`
files into PNG (when transparent or small) or JPEG (when large and opaque),
moving the original to the Recycle Bin. See [webp-tray/README.md](webp-tray/README.md).

### Cutting a release

Releases are produced by GitHub Actions:

1. Bump version somewhere (cargo.toml is informational; the MSI version comes
   from the git tag).
2. Tag and push:
   ```
   git tag webp-tray-v0.2.0
   git push origin webp-tray-v0.2.0
   ```
3. The workflow at `.github/workflows/webp-tray-release.yml` runs `cargo test`,
   builds the release exe + MSI, and publishes a GitHub Release with the MSI
   attached.

To dry-run a build without publishing, use **Run workflow** in the Actions tab
and supply a version string.
