# Packaging & installation guide

This document describes how **Mountie** is installed and packaged for Linux: the user-facing install script, Debian (`.deb`) package generation, release publishing, file layouts, and troubleshooting.

For day-to-day end-user steps, see also [INSTALL.md](../INSTALL.md) and the [README](../README.md#install).

---

## Table of contents

1. [Overview](#overview)
2. [Quick reference](#quick-reference)
3. [Install script (`install.sh`)](#install-script-installsh)
4. [Debian package builder (`scripts/build-deb.sh`)](#debian-package-builder-scriptsbuild-debsh)
5. [Package contents and filesystem layout](#package-contents-and-filesystem-layout)
6. [Dependencies](#dependencies)
7. [Versioning](#versioning)
8. [Publishing a GitHub Release](#publishing-a-github-release)
9. [Supported platforms and architectures](#supported-platforms-and-architectures)
10. [Uninstall](#uninstall)
11. [Security notes](#security-notes)
12. [Troubleshooting](#troubleshooting)
13. [Maintainer checklist](#maintainer-checklist)
14. [Future packaging (out of scope today)](#future-packaging-out-of-scope-today)

---

## Overview

Mountie ships two complementary install paths:

| Path | Tool | Best for |
|---|---|---|
| **One-shot installer** | [`install.sh`](../install.sh) | End users on any Linux desktop; curl one-liner or local clone |
| **Debian package** | [`scripts/build-deb.sh`](../scripts/build-deb.sh) | Debian, Ubuntu, Zorin OS, and derivatives; GitHub Releases |

```text
                    ┌─────────────────────┐
                    │   GitHub Release    │
                    │ mountie_X.Y.Z_*.deb  │
                    └──────────┬──────────┘
                               │ download
           ┌───────────────────┼───────────────────┐
           │                   ▼                   │
           │            install.sh --system        │
           │            (or default on deb-like)   │
           │                   │                   │
           │                   ▼                   │
           │         apt / dpkg → /usr/...         │
           │                                       │
  no release / non-Debian                          │
           │                                       │
           ▼                                       │
   cargo build --release                           │
           │                                       │
           ▼                                       │
   ~/.local/bin + desktop + icon                   │
                                                   │
  Maintainer:                                      │
  ./scripts/build-deb.sh  ──► dist/*.deb ──────────┘
```

Design goals:

- **Easy default** — one command installs something usable.
- **No forced root** — user install to `~/.local` when building from source.
- **Native packages on Debian-family** — real `.deb` with Depends, desktop file, icon caches.
- **Versioned artifacts** — package name encodes version and architecture for releases.

---

## Quick reference

### End users

```bash
# Recommended: install script (deb if available, else source → ~/.local)
curl -fsSL https://raw.githubusercontent.com/maplepreneur/Mountie/main/install.sh | bash

# System-wide on Debian/Ubuntu/Zorin
curl -fsSL https://raw.githubusercontent.com/maplepreneur/Mountie/main/install.sh | bash -s -- --system

# From a git clone
git clone https://github.com/maplepreneur/Mountie.git
cd Mountie
./install.sh
```

### Maintainers / packagers

```bash
# Build versioned .deb from Cargo.toml version
./scripts/build-deb.sh
# → dist/mountie_<version>_<arch>.deb

# Reuse existing target/release/mountie
./scripts/build-deb.sh --skip-build

# Stamp a specific version in the package metadata
./scripts/build-deb.sh 0.2.0

# Install the local package
sudo apt install ./dist/mountie_*.deb
```

---

## Install script (`install.sh`)

### Location and invocation

| Context | Command |
|---|---|
| Remote (pipe) | `curl -fsSL https://raw.githubusercontent.com/maplepreneur/Mountie/main/install.sh \| bash` |
| Remote with flags | `curl -fsSL …/install.sh \| bash -s -- --system` |
| Local clone | `./install.sh [flags]` |

The script is POSIX-ish Bash with `set -euo pipefail`. It must be run with Bash (not plain `sh`).

### Modes and flags

| Flag | Mode | Behavior |
|---|---|---|
| *(none)* | `auto` | Smart path — see [Auto mode](#auto-mode) |
| `--user` | `user` | Build (if needed) and install under `~/.local` |
| `--system` | `system` | Prefer `.deb` / system paths (`/usr` or `/usr/local`) |
| `--from-source` | `source` | Always compile with Cargo; install per mode rules (user unless combined with system path logic) |
| `--deb PATH` | `deb` | Install the given `.deb` with `apt`/`dpkg` |
| `--uninstall` | `uninstall` | Remove user install and/or Debian package / `/usr/local` files |
| `-y` / `--yes` | — | Non-interactive where a prompt would appear (e.g. local `dist/*.deb`) |
| `-h` / `--help` | — | Print usage |

### Auto mode

When no flag is passed, the script chooses:

1. **Local package** — if run from a clone that has `dist/mountie_*.deb` and the OS is Debian-like:
   - Interactive TTY: prompt to install that package system-wide.
   - Non-interactive (`curl | bash` or `--yes`): install it without prompting.
2. **GitHub Release `.deb`** — on Debian-like systems, query  
   `https://api.github.com/repos/maplepreneur/Mountie/releases/latest`  
   and download an asset matching  
   `mountie_*_<arch>.deb`  
   (e.g. `mountie_0.1.0_amd64.deb`). Install with `apt`/`dpkg`.
3. **Build from source** — clone (if needed), install build deps on Debian-like systems, ensure Rust via rustup, `cargo build --release`, install to `~/.local`.

### Debian-like detection

A system is treated as Debian-like when both are true:

- `dpkg` is on `PATH`
- `apt` or `apt-get` is on `PATH`

That covers Debian, Ubuntu, Zorin OS, Linux Mint, Pop!_OS, and most derivatives.

### What gets installed (user mode)

| Source | Destination |
|---|---|
| Release binary `mountie` | `~/.local/bin/mountie` |
| `resources/com.voxelnorth.Mountie.desktop` | `~/.local/share/applications/com.voxelnorth.Mountie.desktop` |
| `resources/icons/com.voxelnorth.Mountie.png` | `~/.local/share/icons/hicolor/256x256/apps/com.voxelnorth.Mountie.png` |

When the script is not run from a source tree, it downloads the desktop file and icon from the `main` branch raw URLs on GitHub.

After copy, it runs (best-effort):

- `update-desktop-database ~/.local/share/applications`
- `gtk-update-icon-cache` on the hicolor theme

If `~/.local/bin` is not on `PATH`, the script prints a hint to add it.

### What gets installed (system / deb mode)

See [Package contents](#package-contents-and-filesystem-layout). Non-Debian `--system` falls back to `/usr/local/bin` plus matching share paths.

### Build-from-source details

On Debian-like systems the script runs approximately:

```bash
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential pkg-config \
  libgtk-4-dev libadwaita-1-dev libglib2.0-dev \
  curl ca-certificates
```

If `cargo` / `rustc` are missing, it installs the stable toolchain via [rustup](https://rustup.rs) non-interactively (`sh -s -- -y`) and sources `~/.cargo/env`.

Outside Debian-like distros, the script **does not** attempt to install system packages; it warns that GTK4 / libadwaita **development** packages must already be present.

### Environment and network requirements

| Need | When |
|---|---|
| Network (HTTPS) | curl install, release download, rustup, git clone, raw resource fetch |
| `sudo` or root | System/deb install; apt build-deps |
| `git` | Clone when not already in a source tree |
| `curl` | Remote install, release API, rustup |
| `cargo` | Source builds (installed automatically if missing) |

### Exit behavior

- Failures call `die` and exit non-zero.
- Missing optional tools (icon cache helpers) are ignored so install can still succeed.
- Temporary clone directories used for remote source builds are cleaned up via `trap` on exit.

---

## Debian package builder (`scripts/build-deb.sh`)

### Purpose

Produce a standards-friendly binary `.deb` without a full Debian source package / `debian/` directory tree. Suitable for:

- GitHub Releases
- Local `apt install ./….deb`
- CI that only needs a single artifact per architecture

### Usage

```bash
./scripts/build-deb.sh [VERSION] [--skip-build] [--help]
```

| Argument | Meaning |
|---|---|
| *(no version)* | Read `version = "…"` from root `Cargo.toml` |
| `VERSION` | Override package version (e.g. `0.2.0` or `0.2.0-1`) |
| `--skip-build` | Do not run `cargo build --release`; require `target/release/mountie` |
| `-h` / `--help` | Print header usage |

### Pipeline

1. Resolve **version** and **Debian architecture** (`amd64`, `arm64`, `armhf`).
2. Unless `--skip-build`: `cargo build --release`.
3. Stage a package tree under `dist/mountie_<version>_<arch>/`:
   - `DEBIAN/control`, `postinst`, `postrm`
   - `usr/bin/mountie`
   - desktop file, icon, `usr/share/doc/mountie/{copyright,changelog.Debian.gz}`
4. Normalize permissions (dirs `755`, binary `755`, data files non-writable).
5. `dpkg-deb --root-owner-group --build` → `dist/mountie_<version>_<arch>.deb`
6. Remove the staging directory; keep the `.deb`.

### Output artifact

```text
dist/mountie_0.1.0_amd64.deb
```

Naming is intentional and **must** stay stable for `install.sh` asset discovery:

```text
mountie_<version>_<debian_arch>.deb
```

`dist/` is gitignored (see `.gitignore`).

### Host requirements to build packages

| Tool | Role |
|---|---|
| `cargo` / Rust | Compile release binary (unless `--skip-build`) |
| GTK4 + libadwaita **dev** packages | Link time |
| `dpkg-deb` | Package assembly (`dpkg-dev` on Debian) |
| `gzip` | Compress `changelog.Debian` |

Example (build machine):

```bash
sudo apt install build-essential pkg-config dpkg-dev gzip \
  libgtk-4-dev libadwaita-1-dev libglib2.0-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
./scripts/build-deb.sh
```

### Control metadata

| Field | Value |
|---|---|
| Package | `mountie` |
| Version | From `Cargo.toml` or override |
| Section | `utils` |
| Priority | `optional` |
| Architecture | Host-mapped Debian arch |
| Depends | `libgtk-4-1, libadwaita-1-0, libglib2.0-0` |
| Maintainer | Voxel North |
| Homepage | https://github.com/maplepreneur/Mountie |

`Installed-Size` is computed from the staged `usr/` tree (KiB).

**Note on `libglib2.0-0`:** On Ubuntu 24.04+ the real package is often `libglib2.0-0t64`, which **Provides** `libglib2.0-0`. Declaring the virtual/legacy name keeps the package installable across older and newer Debian-family releases.

### Maintainer scripts

**`postinst` / `postrm`** (best-effort, never fail the install):

- `update-desktop-database -q /usr/share/applications`
- `gtk-update-icon-cache -q -f -t /usr/share/icons/hicolor`

### Inspecting a built package

```bash
dpkg-deb -I dist/mountie_*.deb    # control metadata
dpkg-deb -c dist/mountie_*.deb    # file list
dpkg-deb -f dist/mountie_*.deb Depends Version Architecture

# Dry-run install
apt-get install -s ./dist/mountie_0.1.0_amd64.deb
```

---

## Package contents and filesystem layout

### Debian package (`apt` / system install)

| Path | Description |
|---|---|
| `/usr/bin/mountie` | Application binary (stripped release build) |
| `/usr/share/applications/com.voxelnorth.Mountie.desktop` | App menu launcher (`Exec=mountie`, icon name) |
| `/usr/share/icons/hicolor/256x256/apps/com.voxelnorth.Mountie.png` | Application icon |
| `/usr/share/doc/mountie/copyright` | MIT license (machine-readable header + full text) |
| `/usr/share/doc/mountie/changelog.Debian.gz` | Per-release packaging changelog |

### User install (`~/.local`)

| Path | Description |
|---|---|
| `~/.local/bin/mountie` | Binary |
| `~/.local/share/applications/com.voxelnorth.Mountie.desktop` | Launcher |
| `~/.local/share/icons/hicolor/256x256/apps/com.voxelnorth.Mountie.png` | Icon |

### Application data (not part of the package)

Created at runtime by Mountie itself — **not** removed by `apt remove` or `install.sh --uninstall` (except optional manual cleanup in INSTALL.md):

| Path | Purpose |
|---|---|
| `~/.local/share/mountie/` | Profiles, icons, app metadata |
| `~/.local/share/applications/webapp-*.desktop` | Per–web-app launchers |
| `~/.local/share/anchor/` | Legacy data from the Anchor product name |
| `~/.local/share/zorin-webapp-manager/` | Legacy data from the v1 project name |

---

## Dependencies

### Runtime (end users — `.deb` Depends)

| Package | Why |
|---|---|
| `libgtk-4-1` | GTK 4 UI |
| `libadwaita-1-0` | libadwaita widgets / styling |
| `libglib2.0-0` | GLib (satisfied by `libglib2.0-0t64` via Provides on newer Ubuntu) |

These are typically already installed on Zorin OS and Ubuntu GNOME desktops. A browser is required to *use* web apps but is **not** a package dependency (user choice: Brave, Firefox, Chrome, etc.).

### Build-time (compiling Mountie)

| Package / tool | Why |
|---|---|
| `build-essential` | C toolchain for native deps |
| `pkg-config` | Discover GTK/libadwaita |
| `libgtk-4-dev` | GTK headers |
| `libadwaita-1-dev` | libadwaita headers |
| `libglib2.0-dev` | GLib headers |
| Rust (`rustc` + `cargo`) | Compile the project |

Release profile (from `Cargo.toml`): LTO, single codegen unit, stripped binary — smaller, faster runtime artifacts for packaging.

---

## Versioning

| Source of truth | Field |
|---|---|
| Application / crate version | `version` in [`Cargo.toml`](../Cargo.toml) |
| Debian package version | Same string by default; optional CLI override in `build-deb.sh` |
| Git tag (recommended) | `v<version>` e.g. `v0.1.0` |
| Release asset name | `mountie_<version>_<arch>.deb` |

### Bumping a release version

1. Edit `Cargo.toml`:

   ```toml
   [package]
   version = "0.2.0"
   ```

2. Commit the version bump (and any feature changes).
3. Tag:

   ```bash
   git tag -a v0.2.0 -m "Mountie 0.2.0"
   git push origin v0.2.0
   ```

4. Build packages for each architecture you support.
5. Create a GitHub Release attached to the tag; upload the `.deb` files.

Keep crate version, git tag, and `.deb` filename version **aligned** so users and `install.sh` stay consistent.

---

## Publishing a GitHub Release

`install.sh` discovers packages only from the **latest** GitHub Release API payload.

### Asset naming (required)

```text
mountie_0.1.0_amd64.deb
mountie_0.1.0_arm64.deb
```

The install script matches:

- package prefix `mountie_`
- architecture suffix `_${arch}.deb` where `arch` is `amd64`, `arm64`, or `armhf`

If the name does not match, the script will not see the asset and will fall back to building from source.

### Suggested release notes template

```markdown
## Mountie 0.1.0

### Install

**Debian / Ubuntu / Zorin**

```bash
curl -fsSL https://raw.githubusercontent.com/maplepreneur/Mountie/main/install.sh | bash
# or
sudo apt install ./mountie_0.1.0_amd64.deb
```

### Changes

- …
```

### Manual upload steps

1. Run `./scripts/build-deb.sh` on a clean tree matching the release commit.
2. GitHub → **Releases** → **Draft a new release**.
3. Choose tag `vX.Y.Z`, title `Mountie X.Y.Z`.
4. Attach `dist/mountie_X.Y.Z_*.deb`.
5. Publish.

### Optional: CLI release

```bash
./scripts/build-deb.sh
gh release create "v0.1.0" \
  --title "Mountie 0.1.0" \
  --notes-file RELEASE_NOTES.md \
  dist/mountie_0.1.0_amd64.deb
```

---

## Supported platforms and architectures

### First-class targets

| Distro family | Install method |
|---|---|
| Zorin OS | `.deb` or install script |
| Ubuntu | `.deb` or install script |
| Debian | `.deb` or install script |
| Other GNOME / XDG desktops | Source → `~/.local` via install script |

### Architectures handled by the scripts

| `uname -m` | Debian arch in package name |
|---|---|
| `x86_64` / `amd64` | `amd64` |
| `aarch64` / `arm64` | `arm64` |
| `armv7l` / `armhf` | `armhf` |

Packages are **not** cross-compiled by default; run `build-deb.sh` on each architecture (or on a matching container/VM) to produce that arch’s `.deb`.

### Explicitly out of scope (for now)

- Windows / macOS
- Flatpak / Snap / AppImage
- Official Debian archive / PPA packaging (could reuse the same binary layout later)

---

## Uninstall

### Install script

```bash
./install.sh --uninstall
# or
curl -fsSL https://raw.githubusercontent.com/maplepreneur/Mountie/main/install.sh | bash -s -- --uninstall
```

This removes:

- User files under `~/.local` (binary, desktop, icon)
- Debian package `mountie` if installed via `dpkg`/`apt`
- `/usr/local` copies if that install path was used

It does **not** delete web apps or `~/.local/share/mountie/` data.

### Package manager

```bash
sudo apt remove mountie
# purge config files owned by the package (none under /etc today):
sudo apt purge mountie
```

### Manual cleanup of web apps (destructive)

See [INSTALL.md](../INSTALL.md#uninstall). Only remove `webapp-*.desktop` files if you are sure Mountie (or the legacy Zorin Web App Manager) created them.

---

## Security notes

1. **`curl | bash`** — Users should prefer cloning the repo and running `./install.sh` if they want to audit the script first. The raw URL always tracks the `main` branch tip of `install.sh`.
2. **Release artifacts** — Prefer installing signed/published GitHub Release `.deb` files from the official `maplepreneur/Mountie` repository. Verify checksums if you publish them alongside the assets (optional enhancement).
3. **sudo** — System installs and `apt` dependency installation require elevated privileges; the default source install path does not.
4. **Binary provenance** — `.deb` packages contain a release-built, stripped ELF linked against system GTK/libadwaita. They do not vendor browsers.
5. **Network during source install** — rustup, crates.io, and git clone traffic occur when building from source.

---

## Troubleshooting

### `install.sh` falls back to source even on Ubuntu

- No GitHub Release exists yet, or the latest release has no matching `mountie_*_<arch>.deb`.
- Architecture not in `{amd64,arm64,armhf}`.
- GitHub API rate limit or network failure (script warns and continues).

**Fix:** Build and publish a correctly named `.deb`, or run `./install.sh --from-source` intentionally.

### `apt install ./mountie_….deb` fails on dependencies

```bash
sudo apt-get install -f
```

Ensure the desktop has GTK4 / libadwaita runtime packages available from the distro.

### `mountie: command not found` after user install

`~/.local/bin` is not on `PATH`:

```bash
echo 'export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc
```

(App menu launch via the `.desktop` file does not require `PATH` if the desktop file uses `Exec=mountie` **and** the session includes `~/.local/bin` — many desktops do; if not, install system-wide with a `.deb` or set `Exec=` to an absolute path.)

### Build fails: missing `gtk4` / `libadwaita-1` pkg-config

Install **dev** packages, not only runtime libraries:

```bash
sudo apt install libgtk-4-dev libadwaita-1-dev libglib2.0-dev pkg-config
pkg-config --modversion gtk4
pkg-config --modversion libadwaita-1
```

### `dpkg-deb: command not found`

```bash
sudo apt install dpkg-dev
```

### Desktop entry or icon missing from the menu

```bash
# User install
update-desktop-database ~/.local/share/applications
gtk-update-icon-cache -f -t ~/.local/share/icons/hicolor

# System install
sudo update-desktop-database /usr/share/applications
sudo gtk-update-icon-cache -f -t /usr/share/icons/hicolor
```

Log out/in or restart the shell/session if the menu is heavily cached.

### Wrong package version in the `.deb`

- Confirm `Cargo.toml` version, or pass an explicit version to `build-deb.sh`.
- Rebuild without relying on a stale staged tree (`dist/` staging is deleted after each successful build; only the `.deb` remains).

---

## Maintainer checklist

Use this when cutting a versioned package release:

- [ ] Version bumped in `Cargo.toml`
- [ ] `cargo test` and `cargo build --release` pass on the release commit
- [ ] `./scripts/build-deb.sh` produces `dist/mountie_<ver>_<arch>.deb`
- [ ] `dpkg-deb -I` and `dpkg-deb -c` look correct
- [ ] `apt-get install -s ./dist/….deb` resolves dependencies
- [ ] Smoke-run: install package, launch `mountie`, create/launch a test web app
- [ ] Git tag `v<ver>` pushed
- [ ] GitHub Release published with correctly named `.deb` asset(s)
- [ ] Spot-check:  
      `curl -fsSL https://raw.githubusercontent.com/maplepreneur/Mountie/main/install.sh | bash -s -- --system`  
      on a clean VM (or document source fallback until the release is live)
- [ ] Update release notes / changelog as needed

---

## Future packaging (out of scope today)

These are reasonable follow-ups but are **not** implemented in the current scripts:

| Format | Notes |
|---|---|
| Flatpak | Sandbox + Flathub; needs runtime/SDK and portal story for launching host browsers |
| Snap | Classic confinement may be needed for browser launching |
| AppImage | Single-file desktop binary; still needs host browsers |
| COPR / AUR / RPM | Same binary + desktop/icon layout as the `.deb` |
| GitHub Actions | CI matrix building `amd64`/`arm64` debs on tag push |
| Checksums / sigstore | `SHA256SUMS` + optional signing next to release assets |
| Full Debian source package | `debian/` with debhelper for archive/PPA uploads |

The filesystem layout under `/usr` (binary name, desktop id, icon name) should stay stable so future formats can reuse the same branding.

---

## Related files

| Path | Role |
|---|---|
| [`install.sh`](../install.sh) | End-user installer |
| [`scripts/build-deb.sh`](../scripts/build-deb.sh) | `.deb` generator |
| [`resources/com.voxelnorth.Mountie.desktop`](../resources/com.voxelnorth.Mountie.desktop) | Desktop entry template |
| [`resources/icons/com.voxelnorth.Mountie.png`](../resources/icons/com.voxelnorth.Mountie.png) | App icon |
| [`Cargo.toml`](../Cargo.toml) | Version + release profile |
| [`INSTALL.md`](../INSTALL.md) | User-facing install documentation |
| [`README.md`](../README.md) | Project overview + install TL;DR |

---

*Last updated for the install-script + Debian packaging work. When behavior changes, update this file in the same PR as the scripts.*
