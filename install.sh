#!/bin/sh
# Installs ashira-core on Linux x64 / macOS arm64 from the latest GitHub Release:
#   curl -fsSL https://raw.githubusercontent.com/Night-stars-1/Ashira/master/install.sh | sh
# A specific version: ASHIRA_VERSION=v0.2.0
# Install directory: ASHIRA_INSTALL_DIR (default ~/.local/bin)
set -eu

repo="Night-stars-1/Ashira"
dir="${ASHIRA_INSTALL_DIR:-$HOME/.local/bin}"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64 | Linux-amd64) platform=linux-x64 ;;
  Darwin-arm64 | Darwin-aarch64) platform=macos-arm64 ;;
  *) echo "Unsupported platform: $(uname -s) $(uname -m). Builds exist for Linux x64 and macOS arm64." >&2; exit 1 ;;
esac

if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL -H "User-Agent: ashira-install" "$1"; }
  download() { curl -fsSL -o "$2" "$1"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -qO- --header="User-Agent: ashira-install" "$1"; }
  download() { wget -qO "$2" "$1"; }
else
  echo "curl or wget is required." >&2; exit 1
fi

if [ -n "${ASHIRA_VERSION:-}" ]; then
  api="https://api.github.com/repos/$repo/releases/tags/$ASHIRA_VERSION"
else
  api="https://api.github.com/repos/$repo/releases/latest"
fi
release=$(fetch "$api")

# Download URLs straight from the API JSON, without needing jq.
urls=$(printf '%s\n' "$release" | grep -o '"browser_download_url": *"[^"]*"' | sed 's/.*: *"\([^"]*\)"/\1/')
url=$(printf '%s\n' "$urls" | grep "/ashira-core-[^/]*-$platform\.tar\.gz\$" | head -n 1 || true)
if [ -z "$url" ]; then
  echo "This release has no $platform ashira-core build." >&2; exit 1
fi
name=${url##*/}

temp=$(mktemp -d)
trap 'rm -rf "$temp"' EXIT INT TERM

echo "Downloading $name"
download "$url" "$temp/$name"

# Checked against the release's SHA256SUMS.txt when it has one.
sums=$(printf '%s\n' "$urls" | grep '/SHA256SUMS\.txt$' | head -n 1 || true)
if [ -n "$sums" ]; then
  download "$sums" "$temp/SHA256SUMS.txt"
  expected=$(grep " \*\{0,1\}$name\$" "$temp/SHA256SUMS.txt" | awk '{print $1}' | head -n 1)
  if [ -z "$expected" ]; then echo "SHA256SUMS.txt has no entry for $name." >&2; exit 1; fi
  if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$temp/$name" | awk '{print $1}')
  else
    actual=$(shasum -a 256 "$temp/$name" | awk '{print $1}')
  fi
  if [ "$expected" != "$actual" ]; then echo "Checksum mismatch for $name." >&2; exit 1; fi
fi

tar -xzf "$temp/$name" -C "$temp"
binary=$(find "$temp" -type f -name ashira | head -n 1)
if [ -z "$binary" ]; then echo "The archive has no ashira binary." >&2; exit 1; fi

mkdir -p "$dir"
# Replace through a temp file so a running ashira keeps its old binary.
cp "$binary" "$dir/.ashira.new"
chmod 755 "$dir/.ashira.new"
mv -f "$dir/.ashira.new" "$dir/ashira"
# Downloaded binaries are quarantined on macOS; this one came over HTTPS from the release.
if [ "$platform" = macos-arm64 ]; then xattr -d com.apple.quarantine "$dir/ashira" 2>/dev/null || true; fi

echo "Installed $("$dir/ashira" --version) to $dir/ashira"
case ":$PATH:" in
  *":$dir:"*) ;;
  *) echo "$dir is not on PATH. Add this line to your shell profile (e.g. ~/.bashrc or ~/.zshrc):
  export PATH=\"$dir:\$PATH\"" ;;
esac
