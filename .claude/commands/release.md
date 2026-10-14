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
```

On macOS prefix the test command with `PATH="/opt/homebrew/opt/ffmpeg-full/bin:$PATH"`. Tests require ffmpeg, ffprobe and mediainfo, including drawtext and WebP support. There is one package; all integration tests run without feature flags. Do not skip failing tests or silence warnings.

## 3. Approve release notes

Read commits and relevant diffs since the previous tag. Draft user-facing Added/Fixed/Changed/Removed notes based on actual behavior, omitting version bumps and test-only work. Show the full draft and reasons for omissions. Obtain approval before writing notes.

## 4. Write changelog and workflow notes

Insert the approved version/date entry below `[Unreleased]` and update compare links. In `.github/workflows/release.yml`, edit only the finalizer's `body: |` notes block; title and notes belong to the existing `softprops/action-gh-release` step, not a matrix job. Keep media-tool requirements, checksum guidance and accurate platform-signing notes. Do not add GUI download or auto-update instructions.

## 5. Bump and verify

Run `node scripts/bump-version.mjs <version>` **without `--tag`**. This optional maintainer utility changes exactly root `Cargo.toml` and `Cargo.lock`, using `cargo update --workspace` to avoid dependency upgrades. Node is not required by application build/install/test.

Review the two version changes and confirm the `acsm` lockfile package entry matches the manifest. Re-run the preflight checks after these final input changes. Stage exactly:

```sh
git add Cargo.toml Cargo.lock CHANGELOG.md .github/workflows/release.yml
```

Inspect `git status --short` and the staged diff; stop if relevant modifications are unstaged or unrelated changes are staged. Commit with the approved release message and create `v<version>` locally. Never force a tag.

## 6. Confirm push

Show commits about to ship, tag and Actions URL. Obtain explicit confirmation before pushing `main` and then `v<version>`. Skip both pushes in dry-run mode. If either fails, stop without force/retry tricks.

## 7. Verify and hand off

After an authorized push, check the release workflow run. It builds only these CLI artifacts:

- `acsm-macos-universal` (Developer-ID signed and notarized)
- `acsm-windows-x86_64.exe` (unsigned)
- `acsm-windows-aarch64.exe` (unsigned)
- `acsm-linux-x86_64` (GNU)
- `acsm-linux-x86_64-musl` (static musl; native x86_64 build with Alpine smoke)

Verify all five artifacts, their `.sha256` companions and sorted `SHA256SUMS` are present. The finalizer supplies the release title/body. Provide Actions and Releases URLs. The release remains a **draft** until the human reviews notes and assets and chooses Publish. No updater metadata or desktop installers are expected.

In dry-run mode report the local commit/tag and that nothing was pushed; do not silently clean them up. Any failing command aborts the release. Never skip a gate or access signing secrets to make a check pass.
