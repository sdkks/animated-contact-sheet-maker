# Changelog

## [Unreleased]

## [0.2.0] - 2026-10-15

### Added

- Read batch file lists from stdin using `-`, with LF/CRLF lines or `--null` / `-0` for NUL-separated paths.
- Use `--overwrite-existing` to regenerate exact output paths.

### Changed

- All rendering commands skip existing exact outputs by default instead of creating numbered duplicates.
- Screenshot batches resume missing images without changing their indices or sample times.
- Downloads cover Windows x64/ARM64 and Linux GNU/musl; macOS uses Cargo source installation.
- GNU Linux builds now use Ubuntu 26.04; the static musl build remains available for Alpine.

### Fixed

- Final media files are prepared privately on the destination filesystem before publication, preserving previous outputs on pre-publication failures or cancellation.
- Partial screenshot failures report successfully published files and accurate completion counts.

## [0.1.0] (tagged draft)

- Start the acsm CLI-only release line for video contact sheets, screenshots, preview reels, animated sheets and metadata probing.
- Name the Cargo package, executable and implementation library `acsm`.
- Use `~/.acsm.toml` and `ACSM_CONFIG` for configuration, without legacy aliases or automatic migration. Rendering options, defaults and output formats are unchanged.
- Provide shell completions and a man page under the acsm identity.
- Set up `acsm-*` artifact names and SHA-256 checksums.

The initial tag's release remained a draft. Earlier implementation history remains in Git; original copyright and font notices remain in their license files.

Repository: https://github.com/sdkks/animated-contact-sheet-maker

[Unreleased]: https://github.com/sdkks/animated-contact-sheet-maker/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/sdkks/animated-contact-sheet-maker/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/sdkks/animated-contact-sheet-maker/tree/v0.1.0
