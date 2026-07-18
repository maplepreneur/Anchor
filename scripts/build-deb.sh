#!/usr/bin/env bash
# Build a Debian package for Mountie from a release binary.
#
# Usage:
#   ./scripts/build-deb.sh              # version from Cargo.toml
#   ./scripts/build-deb.sh 0.2.0        # override version
#   ./scripts/build-deb.sh --skip-build # reuse target/release/mountie
#
# Output:
#   dist/mountie_<version>_<arch>.deb

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

SKIP_BUILD=0
VERSION_OVERRIDE=""

for arg in "$@"; do
  case "$arg" in
    --skip-build) SKIP_BUILD=1 ;;
    -h|--help)
      sed -n '2,12p' "$0" | sed 's/^# \?//'
      exit 0
      ;;
    *)
      if [[ -n "$VERSION_OVERRIDE" ]]; then
        echo "error: unexpected argument: $arg" >&2
        exit 1
      fi
      VERSION_OVERRIDE="$arg"
      ;;
  esac
done

if [[ -n "$VERSION_OVERRIDE" ]]; then
  VERSION="$VERSION_OVERRIDE"
else
  VERSION="$(grep -E '^version\s*=' Cargo.toml | head -1 | sed -E 's/.*"([^"]+)".*/\1/')"
fi

if [[ -z "$VERSION" ]]; then
  echo "error: could not determine version from Cargo.toml" >&2
  exit 1
fi

# Map host arch to Debian arch
HOST_ARCH="$(uname -m)"
case "$HOST_ARCH" in
  x86_64|amd64) DEB_ARCH="amd64" ;;
  aarch64|arm64) DEB_ARCH="arm64" ;;
  armv7l|armhf) DEB_ARCH="armhf" ;;
  *)
    echo "error: unsupported architecture: $HOST_ARCH" >&2
    exit 1
    ;;
esac

PKG_NAME="mountie"
DEB_BASENAME="${PKG_NAME}_${VERSION}_${DEB_ARCH}"
DIST_DIR="$ROOT/dist"
STAGE="$DIST_DIR/${DEB_BASENAME}"
DEB_PATH="$DIST_DIR/${DEB_BASENAME}.deb"

echo "==> Building Mountie ${VERSION} (${DEB_ARCH})"

if [[ "$SKIP_BUILD" -eq 0 ]]; then
  if ! command -v cargo >/dev/null 2>&1; then
    echo "error: cargo not found. Install Rust: https://rustup.rs" >&2
    exit 1
  fi
  cargo build --release
else
  if [[ ! -x "$ROOT/target/release/mountie" ]]; then
    echo "error: --skip-build set but target/release/mountie is missing" >&2
    exit 1
  fi
fi

BINARY="$ROOT/target/release/mountie"
if [[ ! -x "$BINARY" ]]; then
  echo "error: release binary not found at $BINARY" >&2
  exit 1
fi

echo "==> Staging package tree"
rm -rf "$STAGE"
mkdir -p \
  "$STAGE/DEBIAN" \
  "$STAGE/usr/bin" \
  "$STAGE/usr/share/applications" \
  "$STAGE/usr/share/icons/hicolor/256x256/apps" \
  "$STAGE/usr/share/doc/${PKG_NAME}"

install -m 755 "$BINARY" "$STAGE/usr/bin/mountie"
install -m 644 "$ROOT/resources/com.voxelnorth.Mountie.desktop" \
  "$STAGE/usr/share/applications/com.voxelnorth.Mountie.desktop"
install -m 644 "$ROOT/resources/icons/com.voxelnorth.Mountie.png" \
  "$STAGE/usr/share/icons/hicolor/256x256/apps/com.voxelnorth.Mountie.png"

# Copyright / license for Debian policy
{
  echo "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/"
  echo "Upstream-Name: Mountie"
  echo "Source: https://github.com/maplepreneur/Mountie"
  echo
  echo "Files: *"
  echo "Copyright: 2026 Voxel North"
  echo "License: MIT"
  echo
  sed 's/^/ /' "$ROOT/LICENSE"
} > "$STAGE/usr/share/doc/${PKG_NAME}/copyright"
chmod 644 "$STAGE/usr/share/doc/${PKG_NAME}/copyright"

cat > "$STAGE/usr/share/doc/${PKG_NAME}/changelog.Debian" <<EOF
${PKG_NAME} (${VERSION}) unstable; urgency=medium

  * Release ${VERSION}

 -- Voxel North <noreply@voxelnorth.com>  $(date -Ru)
EOF
gzip -9n -f "$STAGE/usr/share/doc/${PKG_NAME}/changelog.Debian"
chmod 644 "$STAGE/usr/share/doc/${PKG_NAME}/changelog.Debian.gz"

# Normalize directory modes (Debian prefers 755, no group-write surprises)
find "$STAGE" -type d -exec chmod 755 {} +
find "$STAGE/usr" -type f -exec chmod a-w {} +
chmod 755 "$STAGE/usr/bin/mountie"

# Runtime dependencies (GTK4 + libadwaita stack)
# Keep Depends loose enough for recent Ubuntu/Debian/Zorin.
CONTROL_DEPENDS="libgtk-4-1, libadwaita-1-0, libglib2.0-0"

cat > "$STAGE/DEBIAN/control" <<EOF
Package: ${PKG_NAME}
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: ${DEB_ARCH}
Depends: ${CONTROL_DEPENDS}
Maintainer: Voxel North <noreply@voxelnorth.com>
Homepage: https://github.com/maplepreneur/Mountie
Description: Mount websites into desktop apps on Linux with ease
 Mountie helps you mount websites into desktop apps on Linux with ease.
 Give it a name and a URL — it fetches an icon, launches the site in its own
 window, and pins cleanly to your dock with a separate process from your
 everyday browser. Built for Zorin OS, Ubuntu, and GNOME desktops.
EOF

cat > "$STAGE/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications 2>/dev/null || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -f -t /usr/share/icons/hicolor 2>/dev/null || true
fi
exit 0
EOF

cat > "$STAGE/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications 2>/dev/null || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -f -t /usr/share/icons/hicolor 2>/dev/null || true
fi
exit 0
EOF

chmod 755 "$STAGE/DEBIAN/postinst" "$STAGE/DEBIAN/postrm"

# Installed-Size in KiB (payload only)
INSTALLED_SIZE="$(du -sk "$STAGE/usr" | cut -f1)"
echo "Installed-Size: ${INSTALLED_SIZE}" >> "$STAGE/DEBIAN/control"

if ! command -v dpkg-deb >/dev/null 2>&1; then
  echo "error: dpkg-deb not found. Install: sudo apt install dpkg-dev" >&2
  exit 1
fi

echo "==> Building ${DEB_PATH}"
mkdir -p "$DIST_DIR"
# Root ownership inside the package (standard for .deb)
dpkg-deb --root-owner-group --build "$STAGE" "$DEB_PATH"

# Cleanup staging tree (keep the .deb)
rm -rf "$STAGE"

echo
echo "Created: $DEB_PATH"
ls -lh "$DEB_PATH"
echo
echo "Install with:"
echo "  sudo apt install ./$(basename "$DEB_PATH")"
echo "  # or"
echo "  sudo dpkg -i ./$(basename "$DEB_PATH") && sudo apt-get install -f"
echo
echo "Or upload this file to a GitHub Release for install.sh to pick up."
