#!/bin/sh
# Builds dist/lumen-<os>-<arch>.tar.gz (+ .sha256), the archive install.sh downloads.
set -eu
root="$(cd "$(dirname "$0")/.." && pwd)"
case "$(uname -s)" in Linux) os=linux ;; Darwin) os=macos ;; *) echo "unsupported OS" >&2; exit 1 ;; esac
arch="$(uname -m)"; [ "$arch" = amd64 ] && arch=x86_64; [ "$arch" = arm64 ] && arch=aarch64
cd "$root"
cargo build --release --locked -p lumend -p lumen-studio -p lumen-live-cli
stage="$(mktemp -d)"; trap 'rm -rf "$stage"' EXIT
pkg="$stage/lumen"
mkdir -p "$pkg/workers/tts-python" dist
cp target/release/lumend target/release/lumen-studio target/release/lumen-live packaging/lumen packs/brand/org.lumen.Lumen.svg packs/brand/org.lumen.Lumen.png "$pkg/"
cp -R packs "$pkg/packs"
find "$pkg/packs" -name '.codex.log' -delete
cp workers/tts-python/pyproject.toml workers/tts-python/uv.lock workers/tts-python/worker.py "$pkg/workers/tts-python/"
out="dist/lumen-$os-$arch.tar.gz"
tar -czf "$out" -C "$stage" lumen
if command -v sha256sum >/dev/null; then sha256sum "$out"; else shasum -a 256 "$out"; fi | sed "s|dist/||" >"$out.sha256"
echo "$out"
