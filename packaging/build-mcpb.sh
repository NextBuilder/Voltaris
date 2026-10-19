#!/usr/bin/env bash
# Build the one-click Claude Desktop extension (.mcpb) for Voltaris.
#
# Usage: packaging/build-mcpb.sh [--version X.Y.Z] [--binary PATH] [--platform darwin|linux|win32] [--out DIR]
# Defaults: version from Cargo.toml, binary target/release/voltaris, platform from the host, output dist/.
# Needs Node.js (npx) for the official @anthropic-ai/mcpb packer.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
version=""
binary="$repo_root/target/release/voltaris"
platform=""
out="$repo_root/dist"
mcpb="@anthropic-ai/mcpb@2.1.2"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) version="$2"; shift 2 ;;
    --binary) binary="$2"; shift 2 ;;
    --platform) platform="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

[[ -n "$version" ]] || version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo_root/Cargo.toml" | head -1)"
if [[ -z "$platform" ]]; then
  case "$(uname -s)" in
    Darwin) platform="darwin" ;;
    Linux) platform="linux" ;;
    MINGW*|MSYS*|CYGWIN*) platform="win32" ;;
    *) echo "cannot detect platform; pass --platform" >&2; exit 2 ;;
  esac
fi
bin_name="voltaris"
[[ "$platform" == "win32" ]] && bin_name="voltaris.exe"
[[ -f "$binary" ]] || { echo "binary not found: $binary (build with: cargo build --release -p voltaris)" >&2; exit 1; }

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/server" "$out"
cp "$binary" "$stage/server/$bin_name"
chmod +x "$stage/server/$bin_name"
cp "$repo_root/packaging/mcpb/icon.png" "$stage/icon.png"
cp "$repo_root/LICENSE" "$stage/LICENSE"
sed -e "s/{{VERSION}}/$version/" -e "s/{{BIN}}/$bin_name/g" -e "s/{{PLATFORM}}/$platform/" \
  "$repo_root/packaging/mcpb/manifest.template.json" > "$stage/manifest.json"

npx --yes "$mcpb" validate "$stage/manifest.json"
bundle="$out/voltaris-$version-$platform.mcpb"
npx --yes "$mcpb" pack "$stage" "$bundle"
echo "$bundle"
if command -v sha256sum >/dev/null; then sha256sum "$bundle"; else shasum -a 256 "$bundle"; fi
