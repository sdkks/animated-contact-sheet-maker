# Changelog

## [Unreleased]

### 0.1.0

- Start the acsm CLI-only release line for video contact sheets, screenshots, preview reels, animated sheets and metadata probing.
- Name the Cargo package, executable and implementation library `acsm`.
- Use `~/.acsm.toml` and `ACSM_CONFIG` for configuration, without legacy aliases or automatic migration. Rendering options, defaults and output formats are unchanged.
- Provide shell completions and a man page under the acsm identity.
- Name build artifacts `acsm-*`, retaining macOS universal signing/notarization, Windows x86_64/aarch64, Linux x86_64 GNU and static musl targets, and SHA-256 checksums.

This version has not been released or tagged. Earlier implementation history remains in Git; original copyright and font notices remain in their license files.

Repository: https://github.com/sdkks/animated-contact-sheet-maker
