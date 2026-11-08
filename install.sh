#!/bin/sh
# Lumen installer — Linux only.
#
# From a source checkout (builds with cargo):
#   sh install.sh
# From a release archive (lumen-linux-x86_64.tar.gz with a .sha256 file next to it):
#   LUMEN_ARCHIVE=./lumen-linux-x86_64.tar.gz sh install.sh
# From GitHub releases, once the repository publishes them:
#   curl -fsSL https://raw.githubusercontent.com/OWNER/Lumen/main/install.sh | LUMEN_REPO=OWNER/Lumen sh
#
# Installs for the current user only (no sudo): binaries and the `lumen` launcher into
# ~/.local/bin, packs/avatars/TTS worker into ~/.local/share/lumen, and app-menu entries.
# Missing system packages are listed and installed only if you answer yes.
#
# Options (environment variables):
#   LUMEN_BIN_DIR=DIR     install binaries somewhere other than ~/.local/bin
#   LUMEN_YES=1           install missing packages without asking
#   LUMEN_NO_DEPS=1       never install system packages, only report them
#   LUMEN_TTS=1           also set up the Turkish TTS worker (uv; downloads CPU PyTorch)
#   LUMEN_VERSION=v0.1.0  release to fetch with LUMEN_REPO (default: latest)
#   sh install.sh --uninstall   remove Lumen (keeps your packs; LUMEN_PURGE=1 removes them too)
set -eu

BIN_DIR="${LUMEN_BIN_DIR:-$HOME/.local/bin}"
DATA_ROOT="${XDG_DATA_HOME:-$HOME/.local/share}"
DATA="$DATA_ROOT/lumen"
BINARIES="lumend lumen-studio lumen-live"
VERSION="${LUMEN_VERSION:-latest}"

say() { printf '%s\n' "$*"; }
fail() {
    printf 'Lumen install stopped: %s\n' "$*" >&2
    exit 1
}
have() { command -v "$1" >/dev/null 2>&1; }

[ "$(uname -s)" = Linux ] || fail "Lumen supports Linux only (found $(uname -s))"
case "$(uname -m)" in
    x86_64 | amd64) arch=x86_64 ;;
    arm64 | aarch64) arch=aarch64 ;;
    *) fail "unsupported processor architecture: $(uname -m)" ;;
esac

ask() { # $1 = question; returns 0 for yes
    [ -n "${LUMEN_YES:-}" ] && return 0
    [ -r /dev/tty ] || return 1
    printf '%s [Y/n] ' "$1"
    read -r answer </dev/tty || return 1
    case "${answer:-y}" in [YyEe]*) return 0 ;; *) return 1 ;; esac
}

# --- Uninstall --------------------------------------------------------------
if [ "${1:-}" = --uninstall ]; then
    for binary in $BINARIES lumen; do rm -f "$BIN_DIR/$binary"; done
    rm -rf "$BIN_DIR/.lumen-previous"
    rm -f "$DATA_ROOT/applications/org.lumen.Lumen.desktop" \
        "$DATA_ROOT/applications/org.lumen.Studio.desktop" \
        "$DATA_ROOT/icons/hicolor/scalable/apps/org.lumen.Lumen.svg"
    if [ -n "${LUMEN_PURGE:-}" ]; then
        rm -rf "$DATA"
        say "Lumen removed, including $DATA"
    else
        say "Lumen removed. Your packs are kept in $DATA (LUMEN_PURGE=1 removes them)"
    fi
    exit 0
fi

# --- System packages ----------------------------------------------------------
# Runtime: Vulkan loader + xkbcommon (GPU UI), ALSA (audio), fonts used by the frame/UI.
missing=""
ldconfig -p 2>/dev/null | grep -q 'libvulkan\.so\.1' || missing="$missing vulkan"
ldconfig -p 2>/dev/null | grep -q 'libxkbcommon\.so\.0' || missing="$missing xkbcommon"
ldconfig -p 2>/dev/null | grep -q 'libasound\.so\.2' || missing="$missing alsa"
have fc-list && fc-list | grep -qi 'Nimbus Sans' || missing="$missing fonts"

source_dir=""
if [ -z "${LUMEN_ARCHIVE:-}" ] && [ -z "${LUMEN_REPO:-}" ]; then
    here="$(cd "$(dirname "$0")" 2>/dev/null && pwd || true)"
    [ -f "$here/Cargo.toml" ] && [ -d "$here/apps/lumend" ] && source_dir="$here"
    [ -n "$source_dir" ] || fail "run this from a Lumen checkout, or set LUMEN_ARCHIVE / LUMEN_REPO"
    have cc || missing="$missing compiler"
    have pkg-config || have pkgconf || missing="$missing pkg-config"
fi

if [ -n "$missing" ]; then
    command=""
    if have pacman; then
        command="sudo pacman -S --needed vulkan-icd-loader libxkbcommon alsa-lib gsfonts adwaita-fonts ttf-jetbrains-mono"
        [ -n "$source_dir" ] && command="$command base-devel pkgconf wayland"
    elif have apt-get; then
        command="sudo apt-get install -y libvulkan1 libxkbcommon0 libasound2 fonts-urw-base35"
        [ -n "$source_dir" ] && command="$command build-essential pkg-config libasound2-dev libxkbcommon-dev libwayland-dev libvulkan-dev"
    elif have dnf; then
        command="sudo dnf install -y vulkan-loader libxkbcommon alsa-lib urw-base35-fonts adwaita-sans-fonts"
        [ -n "$source_dir" ] && command="$command gcc pkgconf alsa-lib-devel libxkbcommon-devel wayland-devel"
    elif have zypper; then
        command="sudo zypper install -y libvulkan1 libxkbcommon0 alsa urw-base35-fonts"
        [ -n "$source_dir" ] && command="$command gcc pkg-config alsa-devel libxkbcommon-devel wayland-devel"
    fi
    say "Missing:$missing"
    if [ -z "$command" ]; then
        say "Package manager not recognised; install Vulkan, xkbcommon, ALSA and URW base35 fonts."
    elif [ -n "${LUMEN_NO_DEPS:-}" ]; then
        say "To install them: $command"
    elif ask "Install them now with: $command ?"; then
        sh -c "$command" || say "Packages could not be installed; you can run the command yourself."
    else
        say "To install them later: $command"
    fi
fi

# --- Get the files ------------------------------------------------------------
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM
package="$tmp/lumen"

if [ -n "$source_dir" ]; then
    have cargo || fail "cargo is required to build from source: https://rustup.rs"
    say "Building Lumen (release) from $source_dir — the first build takes a while…"
    (cd "$source_dir" && cargo build --release --locked -p lumend -p lumen-studio -p lumen-live-cli) ||
        fail "build failed"
    mkdir -p "$package"
    for binary in $BINARIES; do cp "$source_dir/target/release/$binary" "$package/"; done
    cp "$source_dir/packaging/linux/lumen" "$source_dir/packaging/linux/org.lumen.Lumen.svg" "$package/"
    cp -R "$source_dir/packs" "$package/packs"
    mkdir -p "$package/workers/tts-python"
    for f in pyproject.toml uv.lock worker.py; do
        cp "$source_dir/workers/tts-python/$f" "$package/workers/tts-python/"
    done
else
    asset="lumen-linux-$arch.tar.gz"
    if [ -n "${LUMEN_ARCHIVE:-}" ]; then
        cp "$LUMEN_ARCHIVE" "$tmp/$asset"
        cp "$LUMEN_ARCHIVE.sha256" "$tmp/$asset.sha256" || fail "missing $LUMEN_ARCHIVE.sha256"
    else
        if [ "$VERSION" = latest ]; then
            base="https://github.com/$LUMEN_REPO/releases/latest/download"
        else
            base="https://github.com/$LUMEN_REPO/releases/download/$VERSION"
        fi
        fetch() {
            if have curl; then curl -fsSL --retry 5 --retry-all-errors "$1" -o "$2"
            elif have wget; then wget -q "$1" -O "$2"
            else fail "curl or wget is required"; fi
        }
        say "Downloading Lumen ($arch)…"
        fetch "$base/$asset" "$tmp/$asset" || fail "could not download $asset from $LUMEN_REPO"
        fetch "$base/$asset.sha256" "$tmp/$asset.sha256" || fail "could not download the checksum"
    fi
    expected="$(cut -d ' ' -f 1 <"$tmp/$asset.sha256")"
    actual="$(sha256sum "$tmp/$asset" | cut -d ' ' -f 1)"
    [ -n "$expected" ] && [ "$expected" = "$actual" ] ||
        fail "SHA-256 mismatch; the download is damaged or altered. Nothing was installed"
    tar -xzf "$tmp/$asset" -C "$tmp"
fi

for f in $BINARIES lumen org.lumen.Lumen.svg; do
    [ -f "$package/$f" ] || fail "$f is missing from the package"
done

# --- Install --------------------------------------------------------------------
mkdir -p "$BIN_DIR"
for binary in $BINARIES lumen; do
    if [ -f "$BIN_DIR/$binary" ]; then
        mkdir -p "$BIN_DIR/.lumen-previous"
        cp "$BIN_DIR/$binary" "$BIN_DIR/.lumen-previous/$binary"
    fi
    # Copy beside the target, then rename: a running old copy keeps working.
    cp "$package/$binary" "$BIN_DIR/.$binary.new"
    chmod 755 "$BIN_DIR/.$binary.new"
    mv -f "$BIN_DIR/.$binary.new" "$BIN_DIR/$binary"
done

# Shipped packs are refreshed; packs you added yourself are left alone.
mkdir -p "$DATA/packs"
cp -R "$package/packs/." "$DATA/packs/"
find "$DATA/packs" -name '.codex.log' -delete 2>/dev/null || true
mkdir -p "$DATA/workers/tts-python"
cp "$package/workers/tts-python/"* "$DATA/workers/tts-python/"

mkdir -p "$DATA_ROOT/applications" "$DATA_ROOT/icons/hicolor/scalable/apps"
cp "$package/org.lumen.Lumen.svg" "$DATA_ROOT/icons/hicolor/scalable/apps/org.lumen.Lumen.svg"
# One app: rehearsal, control room and appearance live in the same window.
rm -f "$DATA_ROOT/applications/org.lumen.Studio.desktop"
cat >"$DATA_ROOT/applications/org.lumen.Lumen.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Lumen
Comment=Münazara yayını: prova, kontrol ve görünüm tek pencerede
Exec="$BIN_DIR/lumen-studio"
Icon=org.lumen.Lumen
Terminal=false
StartupWMClass=lumen
Categories=AudioVideo;Video;
DESKTOP
have update-desktop-database && update-desktop-database "$DATA_ROOT/applications" >/dev/null 2>&1 || true

# --- Optional tools ---------------------------------------------------------------
if ! have codex; then
    say ""
    say "codex CLI not found. GPT-powered characters and persona/avatar generation need it:"
    say "  npm i -g @openai/codex && codex login"
fi
if ! have obs; then
    say "OBS Studio not found. To broadcast from the Yayın page install it (e.g. pacman -S obs-studio)."
fi
if ! have pactl; then
    say "pactl not found; the Lumen audio sink for OBS needs pipewire-pulse (or PulseAudio)."
fi
if [ -n "${LUMEN_TTS:-}" ]; then
    if have uv; then
        say "Setting up the Turkish TTS worker (downloads CPU PyTorch)…"
        uv sync --project "$DATA/workers/tts-python" --python 3.12 || say "TTS setup failed; retry: uv sync --project $DATA/workers/tts-python --python 3.12"
    else
        say "uv not found; TTS skipped. Install uv (https://docs.astral.sh/uv/) and run the installer with LUMEN_TTS=1"
    fi
else
    say "Live voice (TTS) not set up; for \`lumen yayin\` run the installer again with LUMEN_TTS=1"
fi

say ""
say "Installed Lumen in $BIN_DIR (data: $DATA)"
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) say "Note: $BIN_DIR is not on your PATH. Add: export PATH=\"$BIN_DIR:\$PATH\"" ;;
esac
say "Open \"Lumen\" from your app menu, or run: lumen"
