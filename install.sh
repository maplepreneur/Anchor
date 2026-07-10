#!/usr/bin/env bash
# Install Anchor on Linux.
#
# Quick start (from a release or clone):
#   curl -fsSL https://raw.githubusercontent.com/maplepreneur/Anchor/main/install.sh | bash
#
# From a local clone:
#   ./install.sh
#   ./install.sh --user          # install to ~/.local (default, no root)
#   ./install.sh --system        # install system-wide via .deb or /usr/local
#   ./install.sh --from-source   # always build from source
#   ./install.sh --deb PATH      # install a local .deb
#   ./install.sh --uninstall
#
# Debian / Ubuntu / Zorin: prefers the latest GitHub Release .deb when available.
# Other distros (or offline): builds from source and installs to ~/.local.

set -euo pipefail

REPO_SLUG="maplepreneur/Anchor"
REPO_URL="https://github.com/${REPO_SLUG}.git"
RAW_BASE="https://raw.githubusercontent.com/${REPO_SLUG}/main"
RELEASES_API="https://api.github.com/repos/${REPO_SLUG}/releases/latest"

PREFIX_USER="${HOME}/.local"
MODE="auto"          # auto | user | system | source | deb | uninstall
DEB_PATH=""
ASSUME_YES=0

RED=$'\033[0;31m'
GREEN=$'\033[0;32m'
YELLOW=$'\033[1;33m'
BOLD=$'\033[1m'
RESET=$'\033[0m'

info()  { printf '%s==>%s %s\n' "$BOLD" "$RESET" "$*"; }
ok()    { printf '%s✓%s %s\n' "$GREEN" "$RESET" "$*"; }
warn()  { printf '%s!%s %s\n' "$YELLOW" "$RESET" "$*"; }
die()   { printf '%serror:%s %s\n' "$RED" "$RESET" "$*" >&2; exit 1; }

usage() {
  sed -n '2,16p' "$0" | sed 's/^# \?//'
  exit 0
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --user) MODE="user"; shift ;;
    --system) MODE="system"; shift ;;
    --from-source) MODE="source"; shift ;;
    --deb)
      MODE="deb"
      DEB_PATH="${2:-}"
      [[ -n "$DEB_PATH" ]] || die "--deb requires a path"
      shift 2
      ;;
    --uninstall) MODE="uninstall"; shift ;;
    -y|--yes) ASSUME_YES=1; shift ;;
    -h|--help) usage ;;
    *) die "unknown option: $1 (try --help)" ;;
  esac
done

have() { command -v "$1" >/dev/null 2>&1; }

is_debian_like() {
  have dpkg && { have apt-get || have apt; }
}

need_cmd() {
  have "$1" || die "required command not found: $1"
}

# Resolve the directory of this script when run from a clone; empty when piped.
script_dir() {
  local src="${BASH_SOURCE[0]:-}"
  if [[ -n "$src" && -f "$src" ]]; then
    cd "$(dirname "$src")" && pwd
  else
    echo ""
  fi
}

SCRIPT_DIR="$(script_dir)"
IN_SOURCE_TREE=0
if [[ -n "$SCRIPT_DIR" && -f "$SCRIPT_DIR/Cargo.toml" ]] && grep -q 'name = "anchor"' "$SCRIPT_DIR/Cargo.toml" 2>/dev/null; then
  IN_SOURCE_TREE=1
  ROOT="$SCRIPT_DIR"
else
  ROOT=""
fi

sudo_run() {
  if [[ "$(id -u)" -eq 0 ]]; then
    "$@"
  elif have sudo; then
    sudo "$@"
  else
    die "need root privileges (install sudo or run as root)"
  fi
}

install_build_deps_debian() {
  info "Installing build dependencies (Debian/Ubuntu/Zorin)…"
  sudo_run apt-get update -qq
  sudo_run apt-get install -y --no-install-recommends \
    build-essential pkg-config \
    libgtk-4-dev libadwaita-1-dev libglib2.0-dev \
    curl ca-certificates
}

ensure_rust() {
  if have cargo && have rustc; then
    return 0
  fi
  info "Installing Rust toolchain (rustup)…"
  need_cmd curl
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  # shellcheck disable=SC1091
  source "${HOME}/.cargo/env"
  have cargo || die "Rust install finished but cargo is still missing"
}

refresh_desktop_caches() {
  local apps_dir="$1"
  local icons_dir="$2"
  if have update-desktop-database; then
    update-desktop-database "$apps_dir" 2>/dev/null || true
  fi
  if have gtk-update-icon-cache && [[ -d "$icons_dir" ]]; then
    gtk-update-icon-cache -f -t "$icons_dir" 2>/dev/null || true
  fi
}

install_user_from_binary() {
  local binary="$1"
  [[ -x "$binary" ]] || die "binary not executable: $binary"

  local bin_dir="${PREFIX_USER}/bin"
  local apps_dir="${PREFIX_USER}/share/applications"
  local icons_dir="${PREFIX_USER}/share/icons/hicolor"
  local icon_dest="${icons_dir}/256x256/apps"

  info "Installing to ${PREFIX_USER}…"
  mkdir -p "$bin_dir" "$apps_dir" "$icon_dest"
  install -m 755 "$binary" "${bin_dir}/anchor"

  local desktop_src icon_src
  if [[ "$IN_SOURCE_TREE" -eq 1 ]]; then
    desktop_src="${ROOT}/resources/com.voxelnorth.Anchor.desktop"
    icon_src="${ROOT}/resources/icons/com.voxelnorth.Anchor.png"
  else
    desktop_src="$(mktemp)"
    icon_src="$(mktemp --suffix=.png)"
    curl -fsSL "${RAW_BASE}/resources/com.voxelnorth.Anchor.desktop" -o "$desktop_src"
    curl -fsSL "${RAW_BASE}/resources/icons/com.voxelnorth.Anchor.png" -o "$icon_src"
  fi

  install -m 644 "$desktop_src" "${apps_dir}/com.voxelnorth.Anchor.desktop"
  install -m 644 "$icon_src" "${icon_dest}/com.voxelnorth.Anchor.png"

  if [[ "$IN_SOURCE_TREE" -eq 0 ]]; then
    rm -f "$desktop_src" "$icon_src"
  fi

  refresh_desktop_caches "$apps_dir" "$icons_dir"

  # PATH hint
  if ! echo ":$PATH:" | grep -q ":${bin_dir}:"; then
    warn "${bin_dir} is not on your PATH"
    printf '    Add this to your shell rc file:\n'
    printf '      export PATH="%s:$PATH"\n' "$bin_dir"
  fi

  ok "Anchor installed. Run: anchor"
  printf '    Or open “Anchor” from the application menu.\n'
}

install_deb_file() {
  local deb="$1"
  [[ -f "$deb" ]] || die "deb not found: $deb"
  # apt requires a path that looks local (./ or absolute)
  if [[ "$deb" != /* ]]; then
    deb="$(pwd)/$deb"
  fi
  info "Installing package $(basename "$deb")…"
  if have apt-get; then
    # apt resolves runtime deps (gtk, libadwaita)
    if ! sudo_run apt-get install -y "$deb"; then
      sudo_run dpkg -i "$deb" || true
      sudo_run apt-get install -f -y
    fi
  else
    sudo_run dpkg -i "$deb"
  fi
  ok "Anchor installed system-wide. Run: anchor"
}

map_deb_arch() {
  case "$(uname -m)" in
    x86_64|amd64) echo "amd64" ;;
    aarch64|arm64) echo "arm64" ;;
    armv7l|armhf) echo "armhf" ;;
    *) echo "" ;;
  esac
}

download_latest_deb() {
  local arch dest json asset_url name
  arch="$(map_deb_arch)"
  [[ -n "$arch" ]] || return 1
  have curl || return 1

  info "Looking up latest GitHub Release…"
  if ! json="$(curl -fsSL "$RELEASES_API" 2>/dev/null)"; then
    warn "No GitHub releases found (or network error)"
    return 1
  fi

  # Prefer: anchor_*_${arch}.deb
  name="$(printf '%s' "$json" | grep -oE "\"name\":\\s*\"anchor_[^\"]+_${arch}\\.deb\"" | head -1 | sed -E 's/.*"([^"]+)".*/\1/' || true)"
  asset_url="$(printf '%s' "$json" | grep -oE "\"browser_download_url\":\\s*\"[^\"]*anchor_[^\"]+_${arch}\\.deb\"" | head -1 | sed -E 's/.*"([^"]+)".*/\1/' || true)"

  if [[ -z "$asset_url" ]]; then
    warn "No .deb asset for architecture ${arch} in the latest release"
    return 1
  fi

  dest="$(mktemp --suffix=".deb")"
  info "Downloading ${name:-anchor.deb}…"
  curl -fsSL -o "$dest" "$asset_url" || { rm -f "$dest"; return 1; }
  printf '%s\n' "$dest"
}

build_from_source() {
  local workdir binary
  if [[ "$IN_SOURCE_TREE" -eq 1 ]]; then
    workdir="$ROOT"
  else
    need_cmd git
    workdir="$(mktemp -d)"
    info "Cloning ${REPO_SLUG}…"
    git clone --depth 1 "$REPO_URL" "$workdir"
    # Ensure cleanup of temp clone only
    trap 'rm -rf "$workdir"' EXIT
  fi

  if is_debian_like; then
    install_build_deps_debian
  else
    warn "Not a Debian-like system — ensure GTK4 / libadwaita development packages are installed"
  fi

  ensure_rust
  # shellcheck disable=SC1091
  [[ -f "${HOME}/.cargo/env" ]] && source "${HOME}/.cargo/env"

  info "Building release binary (this may take a few minutes)…"
  (
    cd "$workdir"
    cargo build --release
  )

  binary="${workdir}/target/release/anchor"
  [[ -x "$binary" ]] || die "build finished but binary missing"

  # For --system on non-deb systems, install to /usr/local
  if [[ "$MODE" == "system" ]] && ! is_debian_like; then
    info "Installing system-wide to /usr/local…"
    sudo_run install -m 755 "$binary" /usr/local/bin/anchor
    sudo_run mkdir -p \
      /usr/local/share/applications \
      /usr/local/share/icons/hicolor/256x256/apps
    if [[ -f "${workdir}/resources/com.voxelnorth.Anchor.desktop" ]]; then
      sudo_run install -m 644 \
        "${workdir}/resources/com.voxelnorth.Anchor.desktop" \
        /usr/local/share/applications/
      sudo_run install -m 644 \
        "${workdir}/resources/icons/com.voxelnorth.Anchor.png" \
        /usr/local/share/icons/hicolor/256x256/apps/
    fi
    refresh_desktop_caches /usr/local/share/applications /usr/local/share/icons/hicolor
    ok "Anchor installed. Run: anchor"
  else
    # User install — point ROOT resources at workdir for desktop/icon copy
    IN_SOURCE_TREE=1
    ROOT="$workdir"
    install_user_from_binary "$binary"
  fi
}

build_and_install_deb_from_source() {
  local workdir
  if [[ "$IN_SOURCE_TREE" -eq 1 ]]; then
    workdir="$ROOT"
  else
    need_cmd git
    workdir="$(mktemp -d)"
    info "Cloning ${REPO_SLUG}…"
    git clone --depth 1 "$REPO_URL" "$workdir"
    trap 'rm -rf "$workdir"' EXIT
  fi

  install_build_deps_debian
  # dpkg-deb for packaging
  sudo_run apt-get install -y --no-install-recommends dpkg-dev gzip
  ensure_rust
  # shellcheck disable=SC1091
  [[ -f "${HOME}/.cargo/env" ]] && source "${HOME}/.cargo/env"

  (
    cd "$workdir"
    bash scripts/build-deb.sh
  )

  local deb
  deb="$(ls -1t "${workdir}"/dist/anchor_*.deb 2>/dev/null | head -1 || true)"
  [[ -n "$deb" ]] || die "deb build produced no package"
  install_deb_file "$deb"
}

uninstall_user() {
  info "Removing user install…"
  rm -f "${PREFIX_USER}/bin/anchor"
  rm -f "${PREFIX_USER}/share/applications/com.voxelnorth.Anchor.desktop"
  rm -f "${PREFIX_USER}/share/icons/hicolor/256x256/apps/com.voxelnorth.Anchor.png"
  refresh_desktop_caches \
    "${PREFIX_USER}/share/applications" \
    "${PREFIX_USER}/share/icons/hicolor"
  ok "User install removed"
  warn "Web apps and data under ~/.local/share/anchor were left in place"
}

uninstall_system() {
  if is_debian_like && dpkg -l anchor 2>/dev/null | grep -q '^ii'; then
    info "Removing Debian package…"
    sudo_run apt-get remove -y anchor || sudo_run dpkg -r anchor
    ok "Package removed"
    return
  fi
  if [[ -x /usr/local/bin/anchor ]]; then
    info "Removing /usr/local install…"
    sudo_run rm -f /usr/local/bin/anchor
    sudo_run rm -f /usr/local/share/applications/com.voxelnorth.Anchor.desktop
    sudo_run rm -f /usr/local/share/icons/hicolor/256x256/apps/com.voxelnorth.Anchor.png
    ok "System install removed"
    return
  fi
  warn "No system install of Anchor found"
}

# ── main ────────────────────────────────────────────────────────────────────

case "$MODE" in
  uninstall)
    uninstall_user
    uninstall_system
    exit 0
    ;;
  deb)
    install_deb_file "$DEB_PATH"
    exit 0
    ;;
  source)
    build_from_source
    exit 0
    ;;
  user)
    if [[ "$IN_SOURCE_TREE" -eq 1 ]]; then
      ensure_rust
      # shellcheck disable=SC1091
      [[ -f "${HOME}/.cargo/env" ]] && source "${HOME}/.cargo/env"
      if is_debian_like; then
        # Dev headers only if needed
        if ! pkg-config --exists gtk4 libadwaita-1 2>/dev/null; then
          install_build_deps_debian
        fi
      fi
      info "Building release binary…"
      (cd "$ROOT" && cargo build --release)
      install_user_from_binary "${ROOT}/target/release/anchor"
    else
      build_from_source
    fi
    exit 0
    ;;
  system)
    if is_debian_like; then
      # Prefer prebuilt release deb, else build one
      if tmp_deb="$(download_latest_deb)"; then
        install_deb_file "$tmp_deb"
        rm -f "$tmp_deb"
      else
        build_and_install_deb_from_source
      fi
    else
      build_from_source
    fi
    exit 0
    ;;
  auto)
    # 1) Local .deb in dist/ (developer machine)
    if [[ "$IN_SOURCE_TREE" -eq 1 ]] && is_debian_like; then
      local_deb="$(ls -1t "${ROOT}"/dist/anchor_*.deb 2>/dev/null | head -1 || true)"
      if [[ -n "${local_deb:-}" ]]; then
        info "Found local package: $(basename "$local_deb")"
        if [[ "$ASSUME_YES" -eq 1 ]] || [[ ! -t 0 ]]; then
          install_deb_file "$local_deb"
          exit 0
        fi
        printf 'Install system-wide with this .deb? [Y/n] '
        read -r ans || ans=y
        case "${ans:-y}" in
          n|N|no|No) ;;
          *) install_deb_file "$local_deb"; exit 0 ;;
        esac
      fi
    fi

    # 2) GitHub Release .deb on Debian-like systems
    if is_debian_like; then
      if tmp_deb="$(download_latest_deb)"; then
        install_deb_file "$tmp_deb"
        rm -f "$tmp_deb"
        exit 0
      fi
      warn "Falling back to build from source"
    fi

    # 3) Build from source → user install
    build_from_source
    exit 0
    ;;
  *)
    die "internal error: unknown mode $MODE"
    ;;
esac
