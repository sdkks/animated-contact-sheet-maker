# acsm

ACSM generates video contact sheets, screenshots, animated preview reels and animated contact sheets using local `ffmpeg` processes. This checkout is **CLI-only**: no desktop frontend, Tauri, updater, Node build step, GTK or WebKit is required.

Repository: https://github.com/sdkks/animated-contact-sheet-maker. Version 0.1.0 starts this CLI-only project's release line; earlier implementation history remains in Git. Original copyright and permission notices are preserved in [LICENSE](LICENSE).

## Outputs

| Command | Output | Formats |
|---|---|---|
| `sheet` | Timestamped frame grid with optional metadata header | PNG, JPEG |
| `screenshots` | Numbered individual frames | PNG, JPEG |
| `reel` | Short clips stitched into a preview | WebP, WebM, GIF |
| `animated-sheet` | Grid of looping clips | WebP |
| `probe` | Structured video metadata, optionally including MediaInfo | JSON |

HDR tonemapping includes HDR10, HLG and Dolby Vision. MediaInfo enriches headers with codec names, HDR format, bit depth, channel layout and language. Inputs may be files or directories; folder scanning recognizes 43 video extensions. Ctrl-C cancels running rendering work. Media stays on your machine; the CLI has no updater or telemetry.

## Prerequisites

Install Rust stable and a platform C linker to build. Rendering and probing require **ffmpeg**, **ffprobe**, and **MediaInfo CLI**:

```sh
# macOS
brew install ffmpeg-full mediainfo

# Windows
winget install Gyan.FFmpeg MediaArea.MediaInfo

# Debian / Ubuntu
sudo apt install ffmpeg mediainfo
```

Use `ffmpeg-full` on macOS: the default Homebrew ffmpeg bottle lacks features such as drawtext and WebP encoding. ACSM checks the filters/encoders required by each output before rendering. HDR tonemapping uses zscale when available.

## Build and install this checkout

Run from the repository root:

```sh
cargo build --locked --release
cargo install --path . --locked
acsm --help
```

Cargo installs the binary into its configured bin directory (normally `~/.cargo/bin`). Use `--root /your/install/path` for an isolated installation and add that directory's `bin` to PATH. No crates.io publication is assumed by these instructions.

Release workflows retain standalone CLI binaries named `acsm-macos-universal`, `acsm-windows-x86_64.exe`, `acsm-windows-aarch64.exe`, and `acsm-linux-x86_64` (GNU), and add `acsm-linux-x86_64-musl`, plus individual SHA-256 hashes and `SHA256SUMS`. The macOS workflow signs and notarizes its binary; Windows binaries remain unsigned. Release creation is tag-triggered and produces a draft for human review, not GUI installers.

### Download the Linux musl CI build

After the updated CI workflow runs on GitHub, open **Actions → CI → the successful run → Artifacts → acsm-linux-x86_64-musl** in this repository (GitHub sign-in is required). The artifact ZIP contains the x86_64 binary and its `.sha256` file. This is a native Ubuntu x86_64 build, with static-linkage checks and an Alpine smoke test; it is not a local emulated build. An artifact is only available after that job succeeds, and expires according to the repository's Actions retention policy.

Extract the ZIP, then verify and make the downloaded file executable (Actions ZIP downloads do not retain executable permissions):

```sh
sha256sum -c acsm-linux-x86_64-musl.sha256
chmod +x acsm-linux-x86_64-musl
./acsm-linux-x86_64-musl --help
# Alpine runtime tools for rendering/probing (not bundled into the binary):
apk add ffmpeg mediainfo
```

The musl binary does not require glibc or GUI libraries. Media-tool packages have their own dependencies. The existing GNU Linux download remains available; use the musl build for Alpine. No GitHub build or release is implied merely by checking out this workflow.

## Usage

```sh
acsm sheet movie.mkv --cols 5 --rows 4
acsm screenshots movie.mkv -o ./out --count 4
acsm reel movie.mkv --count 2 --clip-length 1 -o ./out
acsm animated-sheet movie.mkv --cols 2 --rows 1 --clip-length 1 -o ./out
acsm probe movie.mkv
acsm probe --mediainfo movie.mkv
cargo run --locked -- sheet movie.mkv
```

Output paths go to **stdout**; progress, errors and run summaries go to **stderr**. Use `--quiet` to suppress progress or `--verbose` to inspect ffmpeg arguments. Each subcommand's `--help` lists its options.

Configuration uses `~/.acsm.toml`, created with commented defaults on first normal run. Override its location with `ACSM_CONFIG`. CLI options take precedence over configuration values. This is a clean identity break: previous config paths and environment variables are not read or migrated automatically. Existing user files are left untouched.

Use the global `--tmp-dir PATH` before or after a subcommand to place temporary media and embedded fonts in an existing writable directory:

```sh
acsm --tmp-dir /mnt/disk/scratch sheet movie.mkv -o ./sheets
```

This does not change final output locations, create the scratch directory, or guarantee sufficient free space. Without it, platform-default temporary storage is used. Help, completions and man-page generation do not access the temporary directory, config file or media tools:

```sh
acsm completions bash > acsm.bash
acsm manpage > acsm.1
```

## Development and verification

One root Cargo package contains binary `acsm` and implementation library `acsm`. `src/run/` handles CLI command loops; pure modules handle layout, drawtext, headers, filenames and metadata parsing; the four pipeline modules construct ffmpeg arguments; `ffmpeg.rs` owns subprocess I/O and cancellation. `probe.rs` preserves concurrent ffprobe/MediaInfo probing and duration fallbacks. Tests and media fixtures live in `tests/`; the embedded font and its notices live in `assets/fonts/`.

```sh
cargo check --locked --all-targets
cargo clippy --locked -- -D warnings
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
actionlint .github/workflows/ci.yml .github/workflows/release.yml
```

Tests include real rendering and require the media tools above. On macOS run tests with `PATH="/opt/homebrew/opt/ffmpeg-full/bin:$PATH" cargo test --locked`. There are no integration-test feature gates. CI runs Rust checks on Linux and Windows, with media integration tests on Linux.

`scripts/bump-version.mjs` is an optional maintainer-only Node utility for updating `Cargo.toml` and `Cargo.lock`; Node is not needed to build, install or test the application.

## License and attribution

ACSM is [MIT-licensed](LICENSE). See the unchanged license for original copyright attribution and permission terms.

The bundled DejaVu Sans font is not MIT-licensed: Bitstream copyright and permission terms, DejaVu public-domain changes, and Arev notices are reproduced in [assets/fonts/LICENSE](assets/fonts/LICENSE), from https://github.com/dejavu-fonts/dejavu-fonts/blob/master/LICENSE. The font is retained unchanged.
