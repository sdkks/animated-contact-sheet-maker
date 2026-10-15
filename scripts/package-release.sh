#!/usr/bin/env bash
# Run from the repository root; output must be a fresh directory/archive.
set -euo pipefail
binary=$1
platform=$2
out=$3
case "$platform" in
  windows-x86_64|windows-aarch64) executable=acsm.exe ;;
  linux-x86_64|linux-x86_64-musl) executable=acsm ;;
  *) echo "Unknown release platform: $platform" >&2; exit 1 ;;
esac
test -s "$binary"
mkdir -p "$out"
out=$(cd "$out" && pwd)
archive="$out/acsm-$platform.zip"
test ! -e "$archive"
stage=$(mktemp -d)
# Only the unique directory created above is removed.
trap 'rm -rf "$stage"' EXIT
cp "$binary" "$stage/$executable"
cp LICENSE "$stage/LICENSE"
cp assets/fonts/LICENSE "$stage/FONT-LICENSE"
if [ "${RUNNER_OS:-}" = Windows ]; then
  export PACKAGE_STAGE PACKAGE_ARCHIVE
  PACKAGE_STAGE=$(cygpath -w "$stage")
  PACKAGE_ARCHIVE=$(cygpath -w "$archive")
  sha=$(pwsh -NoProfile -File scripts/package-release.ps1 | tr -d '\r')
else
  (cd "$stage" && zip -q "$archive" "$executable" LICENSE FONT-LICENSE)
  sha=$(shasum -a 256 "$archive")
  sha=${sha%% *}
fi
printf '%s  %s\n' "$sha" "acsm-$platform.zip" > "$archive.sha256"
