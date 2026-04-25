#!/bin/sh
# servo-fetch installer — downloads a prebuilt binary from GitHub Releases.
# Usage: curl -fsSL https://raw.githubusercontent.com/konippi/servo-fetch/main/install.sh | sh
set -eu

REPO="konippi/servo-fetch"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

say() { printf '%s\n' "$@"; }
err() { say "error: $*" >&2; exit 1; }

need() { command -v "$1" > /dev/null 2>&1 || err "need '$1' (command not found)"; }

get_latest_version() {
  version=$(curl --proto '=https' --tlsv1.2 -fsSL \
    "https://api.github.com/repos/$REPO/releases/latest" \
    | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"//;s/".*//')
  [ -n "$version" ] || err "failed to determine latest version (GitHub API may be rate-limited)"
  echo "$version"
}

detect_target() {
  os=$(uname -s)
  arch=$(uname -m)
  case "$os" in
    Linux)
      case "$arch" in
        x86_64) echo "x86_64-unknown-linux-gnu" ;;
        *) err "unsupported architecture: $arch" ;;
      esac ;;
    Darwin)
      case "$arch" in
        arm64|aarch64) echo "aarch64-apple-darwin" ;;
        x86_64) echo "x86_64-apple-darwin" ;;
        *) err "unsupported architecture: $arch" ;;
      esac ;;
    *) err "unsupported OS: $os (use GitHub Releases for Windows)" ;;
  esac
}

verify_checksum() {
  archive_path="$1"
  checksum_path="$2"
  expected=$(awk '{print $1}' "$checksum_path")
  if command -v sha256sum > /dev/null 2>&1; then
    actual=$(sha256sum "$archive_path" | awk '{print $1}')
  elif command -v shasum > /dev/null 2>&1; then
    actual=$(shasum -a 256 "$archive_path" | awk '{print $1}')
  else
    say "warning: sha256sum/shasum not found, skipping checksum verification"
    return 0
  fi
  [ "$expected" = "$actual" ] || err "checksum mismatch (expected $expected, got $actual)"
}

main() {
  need curl
  need tar

  version=$(get_latest_version)
  target=$(detect_target)
  archive="servo-fetch-${version}-${target}.tar.gz"
  base_url="https://github.com/$REPO/releases/download/${version}"

  say "Installing servo-fetch ${version} (${target})..."
  tmpdir=$(mktemp -d)
  trap 'rm -rf "$tmpdir"' EXIT

  curl --proto '=https' --tlsv1.2 -fsSL "$base_url/$archive" -o "$tmpdir/$archive"
  curl --proto '=https' --tlsv1.2 -fsSL "$base_url/$archive.sha256" -o "$tmpdir/$archive.sha256"

  verify_checksum "$tmpdir/$archive" "$tmpdir/$archive.sha256"

  tar xzf "$tmpdir/$archive" -C "$tmpdir"

  mkdir -p "$INSTALL_DIR"
  cp "$tmpdir/servo-fetch-${version}-${target}/servo-fetch" "$INSTALL_DIR/servo-fetch"
  chmod +x "$INSTALL_DIR/servo-fetch"

  say "Installed servo-fetch to $INSTALL_DIR/servo-fetch"

  case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *) say "Add $INSTALL_DIR to your PATH to use servo-fetch." ;;
  esac
}

main
