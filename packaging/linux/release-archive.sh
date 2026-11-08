#!/bin/sh
# Builds dist/lumen-linux-<arch>.tar.gz (+ .sha256) for install.sh's LUMEN_ARCHIVE / LUMEN_REPO.
set -eu
root="$(cd "$(dirname "$0")/../.." && pwd)"
arch="$(uname -m)"; [ "$arch" = amd64 ] && arch=x86_64; [ "$arch" = arm64 ] && arch=aarch64
cd "$root"
cargo build --release --locked -p lumend -p lumen-studio -p lumen-live-cli
stage="$(mktemp -d)"; trap 'rm -rf "$stage"' EXIT
pkg="$stage/lumen"
mkdir -p "$pkg/workers/tts-python" dist
cp target/release/lumend target/release/lumen-studio target/release/lumen-live packaging/linux/lumen packaging/linux/org.lumen.Lumen.svg "$pkg/"
cp -R packs "$pkg/packs"
find "$pkg/packs" -name '.codex.log' -delete
cp workers/tts-python/pyproject.toml workers/tts-python/uv.lock workers/tts-python/worker.py "$pkg/workers/tts-python/"
out="dist/lumen-linux-$arch.tar.gz"
tar -czf "$out" -C "$stage" lumen
sha256sum "$out" | sed "s|dist/||" >"$out.sha256"
echo "$out"
