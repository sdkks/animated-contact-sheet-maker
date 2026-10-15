---
description: Cut an ACSM CLI release — preflight, approved notes, version bump, commit, tag and confirmed push.
argument-hint: <version> [--dry-run]
---

Follow these steps in order. Arguments: **$ARGUMENTS**. Require a semver `X.Y.Z` or `X.Y.Z-pre`; ask rather than guessing. `--dry-run` performs local work including commit/tag, but skips every push and remote release verification. Announce this clearly.

## 1. Resolve WIP

Require branch `main`. Inspect `git status --short`, show any pending changes and draft a commit message from their actual diff. Obtain approval before staging explicit paths and committing. Do not include unrelated changes. Any push needs explicit confirmation, including WIP pushes.

Record the repository URL from `git remote get-url origin`, owner/repository from `gh repo view --json nameWithOwner`, and current date from `date +%Y-%m-%d`.

## 2. Blocking preflight

- Require an `[Unreleased]` section in `CHANGELOG.md`.
- Require the requested tag to be absent locally and remotely; do not force-delete an existing tag. The source version reset to 0.1.0 does not authorize reusing inherited tags; stop for a human version/tag decision if the name already exists.
- Fetch origin and require no commits in `HEAD..origin/main`; if behind, stop.
- Require new commits since the last release tag.
- Run these root checks, capturing complete output. Every check must pass:

```sh
cargo check --locked --all-targets
cargo clippy --locked -- -D warnings
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked --release
actionlint .github/workflows/ci.yml .github/workflows/release.yml
python3 -B scripts/test-release.py
```

On macOS prefix the test command with `PATH="/opt/homebrew/opt/ffmpeg-full/bin:$PATH"`. Tests require ffmpeg, ffprobe and mediainfo, including drawtext and WebP support. There is one package; all integration tests run without feature flags. Do not skip failing tests or silence warnings.

## 3. Approve release notes

Read commits and relevant diffs since the previous tag. Draft user-facing Added/Fixed/Changed/Removed notes based on actual behavior, omitting version bumps and test-only work. Show the full draft and reasons for omissions. Obtain approval before writing notes.

## 4. Write changelog and workflow notes

Insert the approved version/date entry below `[Unreleased]` and update compare links. In `.github/workflows/release.yml`, edit only the finalizer's `body: |` notes block; title and notes belong to the existing `softprops/action-gh-release` step, not a matrix job. Keep media-tool requirements, checksum guidance, unsigned Windows notes and macOS source-only installation instructions. Do not add GUI download or auto-update instructions.

## 5. Bump and verify

Edit only the root `[package].version` in `Cargo.toml`, then run `cargo update --workspace` to refresh `Cargo.lock` without intentionally upgrading dependencies. Review the diff and stop if unrelated dependency changes appear. No Node helper is required.

Confirm the `acsm` lockfile package entry matches the manifest. Run `python3 scripts/verify-release.py tag v<version>` to check tag/version agreement without creating a tag. Re-run the preflight checks after these final input changes. Stage exactly:

```sh
git add Cargo.toml Cargo.lock CHANGELOG.md .github/workflows/release.yml
```

Inspect `git status --short` and the staged diff; stop if relevant modifications are unstaged or unrelated changes are staged. Commit with the approved release message and create `v<version>` locally. Never force a tag.

## 6. Confirm push

Show commits about to ship, tag and Actions URL. Obtain explicit confirmation before pushing `main` and then `v<version>`. Wait for main CI to pass, including native Windows/Linux packaging smoke tests, before pushing the tag. Tag push authorizes automatic publication after same-tag CI, all builds and asset verification succeed; there is no later manual Publish gate. Skip both pushes in dry-run mode. If either fails, stop without force/retry tricks.

## 7. Verify and hand off

After an authorized tag push, check the release workflow run. It first calls shared CI at the same commit and checks manifest/lockfile/tag agreement, then builds only these CLI ZIP archives:

- `acsm-windows-x86_64.zip` (unsigned `acsm.exe`)
- `acsm-windows-aarch64.zip` (unsigned `acsm.exe`)
- `acsm-linux-x86_64.zip` (GNU `acsm`)
- `acsm-linux-x86_64-musl.zip` (static musl `acsm`; native x86_64 build with Alpine smoke)

Each archive also contains `LICENSE` and `FONT-LICENSE`, copied verbatim from the source notices. Verify all four ZIPs, their `.zip.sha256` companions and sorted `SHA256SUMS` are present. Checksums cover the ZIP bytes, not the extracted executable. The finalizer downloads the draft assets, checks exact inventory, recomputes hashes and verifies ZIP integrity/entries and license contents before uploading `SHA256SUMS`. It verifies the uploaded aggregate, then publishes and marks latest as the last step. Any failed/skipped/cancelled prerequisite prevents publication; partial uploads remain drafts. The finalizer supplies the release title/body. Provide Actions and Releases URLs and confirm the release is published/latest. Do not manually publish a failed draft or modify an older release. No updater metadata or desktop installers are expected. The separate Actions artifact download remains its existing ZIP wrapper around a binary and checksum, not a nested release ZIP.

macOS has no CI/release binary or signing/notarization job. Preserve local macOS support and document source installation: `cargo install --path . --locked` from the repository root, or `cargo install --git https://github.com/sdkks/animated-contact-sheet-maker.git --locked acsm`. Rust stable and a platform C linker are required; keep `brew install ffmpeg-full mediainfo` for runtime media tools.

In dry-run mode report the local commit/tag and that nothing was pushed; do not silently clean them up. Any failing command aborts the release. Never skip a gate or access signing secrets to make a check pass.
