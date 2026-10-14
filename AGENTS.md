# CLAUDE.md

## Commands

This is one root Cargo package, `acsm`, with the implementation library `acsm`. Run all commands from the repository root:

- Development: `cargo run --locked -- <subcommand> [args]`
- Build: `cargo build --locked --release`
- Install: `cargo install --path . --locked`
- Tests: `cargo test --locked` (unit and integration tests, no feature gates)
- Required Rust lint gates: `cargo clippy --locked -- -D warnings` and `cargo clippy --locked --all-targets -- -D warnings`. Fix warnings; do not silence them.
- Workflow validation: `actionlint .github/workflows/ci.yml .github/workflows/release.yml`

Integration tests need ffmpeg with drawtext, libwebp and the other pipeline encoders, plus ffprobe and mediainfo. On macOS install `brew install ffmpeg-full mediainfo` and run tests with `PATH="/opt/homebrew/opt/ffmpeg-full/bin:$PATH" cargo test --locked`. The default Homebrew ffmpeg lacks required features. No Node, frontend or GUI framework is needed for build/install/test.

## Architecture and invariants

**Pipeline separation.** Pure logic (`drawtext.rs`, `layout.rs`, `header.rs`, `output_path.rs`, `video_info.rs`) remains independently testable. Orchestration modules (`contact_sheet.rs`, `screenshots.rs`, `preview_reel.rs`, `animated_sheet.rs`) construct ffmpeg arguments and delegate subprocess I/O to `ffmpeg.rs`. `src/run/` contains CLI command loops and progress adapters. Keep new math/parsing in pure modules rather than inline orchestration. The library is an implementation library, not a separately promised SDK; header, layout and probe stay private.

**Four output types.** Contact sheets (JPEG/PNG), screenshots (individual frames), preview reels (WebP/WebM/GIF), animated sheets (WebP). Each has its own pipeline and output path builder. For animated outputs use `layout::sample_clip_timestamps(dur, n, clip_len)`, not `sample_timestamps`, to leave room for the full clip.

**Selected video and seeking.** Preserve the selected stream's absolute index, duration and start/seek offsets; audio/container tails must not extend sampling past that video stream. Still extraction has fast input-seek and accurate output-seek modes, including the existing fallback. Preserve their timestamp-origin handling and explicit stream mapping. Animated clips use input-level seeking. Do not simplify either path or change seek placement without discussing the behavioral consequences. Bare streams with estimated duration but no seekable timestamps are rejected for rendering.

**Probe.** Private `probe.rs` runs ffprobe and MediaInfo concurrently, preserving stream selection, duration hints and conditional packet counting. Malformed MediaInfo output degrades gracefully. `VideoInfo` serialization and `duration_source` are CLI contracts. Historical `ffmpeg_test_hook_locate`, `ffmpeg_test_hook_probe`, and `video_info_test_hook_parse` names are also production CLI entry points; do not rename incidentally.

**Subprocesses and cancellation.** All pipelines begin with `ffmpeg::base_args()` (`-hide_banner -loglevel error -y`). CLI `signals.rs` owns the shared `Arc<AtomicBool>` for Ctrl-C. Long-running ffmpeg calls must use `run_cancellable` / `run_batch_cancellable`, not `run_capture`. Preserve kill-on-drop, concurrent stderr draining, cancellation polling and bounded batch parallelism. Tokio's explicit `time` feature is required by polling. `PipelineContext` and `ProgressReporter` pass execution settings and callback progress without a GUI event bus.

**Output publication.** Every pipeline renders into a TempDir and finishes with `jobs::move_into_place`; preserve the existing rename/cross-filesystem fallback. Do not change cleanup or publication behavior incidentally. Filename builders in `output_path.rs` own collision suffixes via an `exists_fn` callback. Do not duplicate their logic. Empty configurable suffixes fall back to the existing defaults. JPEG quality mapping to ffmpeg's 2–31 scale lives in `output_path::jpeg_qv`.

**Literal text and paths.** Header lines are plain UTF-8 written verbatim to files in the job TempDir. `drawtext::header_overlay` uses `textfile` with `expansion=none`; never interpolate filenames/metadata into filtergraph text. Keep files alive through rendering. `path_for_ffmpeg` / `font_for_ffmpeg` escape both option and filtergraph layers and return unquoted values: do not add quotes. Timestamp text is internally generated with `format_hms_escaped`.

**Tools and capabilities.** `locate_tools()` resolves ffmpeg, ffprobe, mediainfo, preferring ffmpeg-full on macOS before PATH/Homebrew fallbacks. All three are required. Each options struct has `validate()` and `needs()`; CLI calls these before rendering. Add any newly needed encoder/filter to `ffmpeg::Need` and the relevant `needs()`. Preserve zscale-dependent HDR handling and MediaInfo enrichment fallbacks.

**Defaults and CLI identity.** `defaults.rs` is the source of shipping option defaults. Keep binary name `acsm`, config `~/.acsm.toml`, override `ACSM_CONFIG`, CLI-over-config precedence, stdout paths/JSON and stderr progress/errors. `input_scan.rs` owns folder scanning and its canonical extension list. Do not duplicate scanning. Global `--tmp-dir` uses tempfile's existing process-wide override; keep validation before config/tool work. Font bytes in `assets/fonts/DejaVuSans.ttf` are embedded via `src/font.rs`; retain font licensing notices with the asset.

**Tool-free commands.** Help, `completions <shell>` and `manpage` work without config access, temporary-directory validation or media tools. Completions/manpage use runtime clap introspection, not generated build assets.

## Packaging and releases

Keep root `.cargo/config.toml`'s static MSVC runtime setting. Cargo package contents must include the font, font notices, MIT license and test fixtures. Do not add GUI dependencies or reintroduce obsolete `cli` / `test-api` feature gates.

CI keeps Linux media tests and Windows lint/link checks, plus a native Ubuntu x86_64 musl build uploaded as an Actions artifact with its checksum. Releases produce four binaries: Windows x86_64/aarch64 and Linux x86_64 GNU/musl, with per-binary hashes and aggregate `SHA256SUMS`. macOS is source-only: no macOS CI/release builds or Apple signing/notarization steps. Preserve macOS runtime support and local build/test commands; users install from the repository root with `cargo install --path . --locked` or via `cargo install --git https://github.com/sdkks/animated-contact-sheet-maker.git --locked acsm`. Musl jobs install musl-tools and the Rust target, check static linkage, and smoke-test in Alpine; do not replace the existing GNU artifact. Releases are tag-triggered drafts with title/body owned by the softprops finalizer. No GUI bundles or updater assets are produced.

Optional maintainer utility: `node scripts/bump-version.mjs <version> [--tag]` updates root `Cargo.toml` and `Cargo.lock`; Node is not an application prerequisite. Follow `.claude/commands/release.md` for explicit release authorization. Version changes, tags, pushes and release execution must not happen as an incidental check.

## Separate and local content

Project documentation lives at `https://github.com/sdkks/animated-contact-sheet-maker`; do not recreate a separate showcase site or installer scripts here. `docs/` is ignored local-only maintainer material, absent from fresh clones. Do not recreate missing local design docs or remove ignored caches left by previous package layouts. Preserve `AGENT.md` as a symlink to this file.

Test media reference: https://kodi.wiki/view/Samples
