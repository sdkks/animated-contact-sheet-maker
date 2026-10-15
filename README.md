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

Release workflows produce four ZIP archives: `acsm-windows-x86_64.zip`, `acsm-windows-aarch64.zip`, `acsm-linux-x86_64.zip` (GNU), and `acsm-linux-x86_64-musl.zip`. Each contains `acsm.exe` or `acsm`, the project's `LICENSE`, and the font notices in `FONT-LICENSE`. Verify the ZIP bytes against the matching `.zip.sha256` file (for example, `sha256sum -c acsm-linux-x86_64-musl.zip.sha256`) or `SHA256SUMS` before extracting and installing. Windows binaries remain unsigned. Version tags run shared CI at the same commit and all four release builds; assets are staged as a draft, verified, then automatically published as the latest release. Failures leave the release unpublished; no manual Publish click is required after success. These are CLI downloads, not GUI installers. Previously published releases retain their original asset format.

### macOS: install from source

No macOS binaries are built by CI or attached to releases. With the Rust and media-tool prerequisites above installed, use `cargo install --path . --locked` from the repository root, or install directly from Git:

```sh
cargo install --git https://github.com/sdkks/animated-contact-sheet-maker.git --locked acsm
```

macOS runtime support and local build/test commands remain available.

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

Successfully published output paths go to **stdout**; progress, errors and run summaries go to **stderr**. Use `--quiet` to suppress progress and summaries (not errors or produced paths), or `--verbose` to inspect ffmpeg arguments. Summary counts refer to input videos; screenshots also report images produced/skipped per video. Each subcommand's `--help` lists its options.

### Batch input and resuming

All four rendering commands accept files/directories and one literal `-` to read a path list from stdin at that position:

```sh
printf '%s\n' './first video.mkv' './second.mp4' | acsm sheet -
acsm reel - < paths.txt
find ./videos -type f -name '*.mkv' -print0 | acsm screenshots --null -
acsm sheet ./before.mkv - ./after.mkv < paths.txt
acsm animated-sheet --overwrite-existing - < paths.txt
```

Lists use LF or CRLF by default; `--null` (or `-0`) selects NUL separation and preserves literal CR/LF in filenames. Empty records are ignored; a final record needs no terminator. Paths must be UTF-8. Spaces, quotes, backslashes and Unicode are literal, not shell syntax; paths are relative to the current directory and directories use the usual scanning/recursion rules. A list record `-` names a literal file; use `./-` for that file directly on the command line. Repeated argv `-` markers and `--null` without `-` are input errors. No marker means stdin is not read. Probe remains single-input JSON.

Existing exact output names are skipped regardless of contents or the options that produced them. Use `--overwrite-existing` to regenerate them. No numbered duplicate is created merely because an output exists; older numbered files are left alone. Screenshots resume individually, retaining the original requested indices and timestamps. Two inputs targeting the same output name follow the same policy: the first wins by default, or the last successfully published overwrite wins.

Each completed media file is prepared privately on its destination filesystem and then published without exposing partial contents. Default publication cannot overwrite another writer that wins the same name; explicit overwrite replaces only after preparation succeeds. Failures or cancellation before commit preserve the old output. Existing symlinks are skipped by default; overwrite replaces the link itself, not its target. Directories and special-file obstructions are errors.

Atomicity is **per file**, not batch rollback or power-loss durability. Earlier published files remain and their paths are reported if later work fails. Cancellation during an entered commit cannot undo it; cancellation during a blocking copy may wait for that copy before discarding the prepared file. Rename preserves rendered metadata; cross-filesystem copying preserves bytes and source permission bits, not a portable guarantee for all timestamps/ACLs/xattrs. Unsupported publication operations fail safely rather than copying directly over an existing output.

All-skipped runs exit 0 after normal startup validation; processing errors exit 1, setup/input errors 2, cancellation 130. Stdout remains newline-framed, even with `--null` input, so output paths containing newlines are not unambiguously line-delimited.

Configuration uses `~/.acsm.toml`, created with commented defaults on first normal run. Override its location with `ACSM_CONFIG`. CLI options take precedence over configuration values. This is a clean identity break: previous config paths and environment variables are not read or migrated automatically. Existing user files are left untouched.

Use the global `--tmp-dir PATH` before or after a subcommand to place temporary media and embedded fonts in an existing writable directory:

```sh
acsm --tmp-dir /mnt/disk/scratch sheet movie.mkv -o ./sheets
```

This does not change final output locations, create the scratch directory, or guarantee sufficient free space. Without it, platform-default temporary storage is used. Final publication staging always lives in the destination directory, independently of `--tmp-dir`, and needs enough free space for the replacement alongside any old output. Help, completions and man-page generation do not access the temporary directory, config file or media tools:

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

Maintainer releases follow `.agents/commands/release.md`: update the root package version in `Cargo.toml`, refresh `Cargo.lock` with `cargo update --workspace`, and verify the diff does not upgrade dependencies. Release packaging uses native ZIP tooling and Python 3.11+ standard-library validation in CI; these are not application runtime dependencies.

## License and attribution

ACSM is [MIT-licensed](LICENSE). See the unchanged license for original copyright attribution and permission terms.

The bundled DejaVu Sans font is not MIT-licensed: Bitstream copyright and permission terms, DejaVu public-domain changes, and Arev notices are reproduced in [assets/fonts/LICENSE](assets/fonts/LICENSE), from https://github.com/dejavu-fonts/dejavu-fonts/blob/master/LICENSE. The font is retained unchanged.
