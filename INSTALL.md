# Install Mountie

Complete setup instructions for installing **Mountie** on Linux.

## Quick install

```bash
curl -fsSL https://raw.githubusercontent.com/maplepreneur/Mountie/main/install.sh | bash
```

| Flag | Behavior |
|---|---|
| *(default)* | On Debian/Ubuntu/Zorin: install latest GitHub Release `.deb` if present; else build from source → `~/.local` |
| `--system` | System-wide install (`.deb` when possible) |
| `--user` | Always install under `~/.local` |
| `--from-source` | Always compile with Cargo |
| `--deb PATH` | Install a local `.deb` |
| `--uninstall` | Remove user and/or package install |

From a git clone:

```bash
git clone https://github.com/maplepreneur/Mountie.git
cd Mountie
./install.sh
```

## Requirements

| Requirement | Notes |
|---|---|
| Linux desktop | Designed for **Zorin OS**, Ubuntu, and other GNOME-based desktops; works anywhere XDG `.desktop` files work |
| A browser | Brave, Firefox / Firefox Developer Edition, Chrome, Chromium, Edge, Vivaldi, or similar |
| GTK 4 + libadwaita | Runtime libraries (usually preinstalled on Zorin/Ubuntu) |
| Rust toolchain | Only needed when building from source (`rustc` / `cargo`) |

## Debian package (`.deb`)

### Install a prebuilt package

After a GitHub Release is published:

```bash
# Via install script (picks the right arch)
./install.sh --system

# Or download the .deb from the release page and:
sudo apt install ./mountie_0.1.0_amd64.deb
```

### Build a package yourself

Version is read from `Cargo.toml` (override with an argument):

```bash
# Build release binary + package
./scripts/build-deb.sh
# → dist/mountie_<version>_<arch>.deb

# Reuse an existing target/release/mountie
./scripts/build-deb.sh --skip-build

# Override version stamp in the package metadata
./scripts/build-deb.sh 0.2.0
```

Package layout:

| Path | Content |
|---|---|
| `/usr/bin/mountie` | Binary |
| `/usr/share/applications/com.voxelnorth.Mountie.desktop` | App menu launcher |
| `/usr/share/icons/hicolor/256x256/apps/com.voxelnorth.Mountie.png` | Icon |

Runtime dependencies declared in the package: `libgtk-4-1`, `libadwaita-1-0`, `libglib2.0-0`.

### Publishing a release

1. Bump `version` in `Cargo.toml`
2. Commit and tag, e.g. `git tag v0.1.0`
3. Run `./scripts/build-deb.sh` (on each arch you support, or cross-build)
4. Create a GitHub Release and upload `dist/mountie_<version>_<arch>.deb`
5. Name the asset exactly like `mountie_0.1.0_amd64.deb` so `install.sh` can find it

## Install build dependencies

### Zorin OS / Ubuntu / Debian

These packages are required to **compile** Mountie (runtime GTK/libadwaita are usually already installed on Zorin/Ubuntu). The install script installs them automatically when building from source.

```bash
sudo apt update
sudo apt install build-essential pkg-config \
  libgtk-4-dev libadwaita-1-dev libglib2.0-dev
```

Verify the toolchain can see GTK:

```bash
pkg-config --modversion gtk4
pkg-config --modversion libadwaita-1
```

### Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustc --version
cargo --version
```

## Build from source

```bash
git clone https://github.com/maplepreneur/Mountie.git
cd Mountie
cargo build --release
```

Optional — confirm unit tests pass:

```bash
cargo test
```

The binary is written to:

```text
target/release/mountie
```

Run without installing:

```bash
./target/release/mountie
# or during development:
cargo run
```

## Install for your user (manual)

This installs the binary, icon, and a desktop launcher so **Mountie** appears in the app menu. Prefer `./install.sh --user` when possible.

```bash
cargo build --release

mkdir -p ~/.local/bin \
  ~/.local/share/applications \
  ~/.local/share/icons/hicolor/256x256/apps

cp target/release/mountie ~/.local/bin/
cp resources/com.voxelnorth.Mountie.desktop ~/.local/share/applications/
cp resources/icons/com.voxelnorth.Mountie.png \
  ~/.local/share/icons/hicolor/256x256/apps/

# Ensure ~/.local/bin is on your PATH (add to ~/.bashrc if needed)
export PATH="$HOME/.local/bin:$PATH"

update-desktop-database ~/.local/share/applications 2>/dev/null || true
gtk-update-icon-cache -f -t ~/.local/share/icons/hicolor 2>/dev/null || true
```

Then open **Mountie** from Super search / the application menu, or run:

```bash
mountie
```

### Keyboard shortcuts

In the main window, press **F1** (or the keyboard icon in the header) for the in-app shortcut list. Highlights:

| Action | Shortcut |
|---|---|
| Add web app | Ctrl+N |
| Refresh | Ctrl+R / F5 |
| Launch / Edit / Remove selected | Enter / Ctrl+E / Delete or Ctrl+D |
| Move selection | ↑↓ or J/K |
| Dialog fields | Tab / Shift+Tab · Enter to save · Esc to cancel |

### PATH tip

If `mountie` is not found in a new terminal:

```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc
```
## Upgrading from Anchor or “Zorin Web App Manager”

### From Anchor

Earlier builds used:

- Binary name: `anchor`
- Data dir: `~/.local/share/anchor/`
- Desktop tag: `X-WebApp-Manager=anchor`
- App ID: `com.voxelnorth.Anchor`

Mountie **still lists and repairs** apps created with the Anchor tag. New apps use `mountie` and `~/.local/share/mountie/`.

After installing Mountie:

1. Remove the old binary if present: `rm -f ~/.local/bin/anchor`
2. Replace the old desktop entry with `com.voxelnorth.Mountie.desktop` (see above)
3. Launch Mountie once so existing apps can refresh dock matching metadata

### From “Zorin Web App Manager” (v1)

Earlier builds used:

- Binary name: `zorin-webapp-manager`
- Data dir: `~/.local/share/zorin-webapp-manager/`
- Desktop tag: `X-WebApp-Manager=zorin-webapp-manager`

Mountie **still lists and repairs** apps created with that tag as well.

1. Remove the old binary if you installed it: `rm -f ~/.local/bin/zorin-webapp-manager`
2. Install Mountie and open it once so existing apps refresh dock matching metadata

## Uninstall

```bash
# Preferred
./install.sh --uninstall
# or
curl -fsSL https://raw.githubusercontent.com/maplepreneur/Mountie/main/install.sh | bash -s -- --uninstall
```

Manual user uninstall:

```bash
rm -f ~/.local/bin/mountie
rm -f ~/.local/share/applications/com.voxelnorth.Mountie.desktop
rm -f ~/.local/share/icons/hicolor/256x256/apps/com.voxelnorth.Mountie.png
update-desktop-database ~/.local/share/applications 2>/dev/null || true
gtk-update-icon-cache -f -t ~/.local/share/icons/hicolor 2>/dev/null || true
```

If installed via `.deb`:

```bash
sudo apt remove mountie
```

Optional — remove Mountie-created web apps and data (destructive):

```bash
# Managed launchers
rm -f ~/.local/share/applications/webapp-*.desktop

# App data (icons + isolated profiles) — new and legacy paths
rm -rf ~/.local/share/mountie
rm -rf ~/.local/share/anchor
rm -rf ~/.local/share/zorin-webapp-manager
```

Only delete `webapp-*.desktop` files if you are sure they were created by Mountie, Anchor, or the previous Zorin Web App Manager.

## Troubleshooting

### Dock shows the browser icon instead of the web app

On **Wayland**, Chromium-family browsers set a URL-based window id (for example `brave-www.youtube.com__-Default`). Mountie writes that value into `StartupWMClass`.

1. Fully quit the web app
2. Open Mountie once (repairs metadata)
3. Launch the web app again from the menu
4. Unpin any old pin that still points at the browser, then pin the web app again

### Signed out inside the web app

**Isolated** profiles start clean by design so apps stay independent of your main browser. Sign in once inside each web app.

If you need logins or browser extensions (for example **1Password**) without signing in again, recreate the app with **Shared browser profile**. Shared copies extensions and logins into a **private** profile (close the browser first for best results), so the app stays a separate process with its own dock icon.

### Shared profile dock icons (fixed)

Older Shared apps launched against the browser’s default profile. Chromium uses one process per profile, so the web app and browser shared a dock icon (same bug as Zorin Web App Manager).

**Current behavior:** Shared apps always get their own `--user-data-dir` / Firefox profile. Mountie sets `StartupWMClass` to Chromium’s real Wayland `app_id`, writes an absolute PNG path in `Icon=` (so the list UI shows favicons immediately), and also installs a themed icon named after the window class for the dock.

After upgrading, open **Mountie once** so existing Shared launchers are repaired (icons rewritten to absolute paths; Firefox Shared re-seeds extensions if the XPI folder was empty). Fully quit the web app and browser, then relaunch the web app. Unpin any old pin that still points at the browser if needed.

**Firefox Developer Edition note:** profiles live under `~/.config/mozilla/firefox/` (not only `~/.mozilla/firefox/`). Shared mode picks the real Dev Edition profile (`dev-edition-default` / `[Install]` default), not an empty stub.

**Firefox Shared site logins (WhatsApp, etc.):** Shared mode copies cookies **and** origin storage (`storage/default/https+++…` IndexedDB/localStorage) for the web app’s domain. Apps that only got extensions/cookies before this fix are re-seeded the next time you open Mountie (fully quit the web app and Firefox first so files are not locked).

### No browsers listed

Install a supported browser and ensure it is on your `PATH`, or set a default browser:

```bash
xdg-settings get default-web-browser
```

### Build fails with missing `gtk4` / `libadwaita`

Install the **development** packages listed under [Install build dependencies](#install-build-dependencies), not only the runtime libraries:

```bash
sudo apt install build-essential pkg-config \
  libgtk-4-dev libadwaita-1-dev libglib2.0-dev
```

If `pkg-config --modversion gtk4` still fails, the `-dev` packages are missing or incomplete. Runtime packages such as `libgtk-4-1` alone are not enough to compile.

## Packaging summary

| Method | Command | Install location |
|---|---|---|
| Install script | `./install.sh` or curl one-liner | `.deb` → `/usr` · source → `~/.local` |
| Debian package | `./scripts/build-deb.sh` then `apt install ./dist/…deb` | `/usr/bin`, `/usr/share/…` |
| Manual user | copy binary + desktop + icon (above) | `~/.local` |

Flatpak is not packaged yet; the `.deb` + install script cover Debian-family desktops and source builds elsewhere.

For maintainers (asset naming, control metadata, release checklist, security notes):

**→ [docs/packaging.md](docs/packaging.md)**
