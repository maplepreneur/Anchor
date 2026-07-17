# Anchor

**Turn any website into a real desktop app.**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform: Linux](https://img.shields.io/badge/platform-Linux-brightgreen.svg)](#install)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg)](https://www.rust-lang.org/)
[![UI: GTK4](https://img.shields.io/badge/UI-GTK4%20%2F%20libadwaita-purple.svg)](https://www.gtk.org/)

Anchor is a free, open-source **web app manager** for Linux. Give it a name and a URL—it fetches an icon, launches the site in its own window, and pins cleanly to your dock with a separate process from your everyday browser.

Built for people on **Zorin OS**, Ubuntu, and GNOME who want the “install this site as an app” experience without fighting browser menus or distro-only tooling.

<p align="center">
  <em>Native GTK app · Profile modes · Wayland-aware dock icons · Multi-browser</em>
</p>

---

## Why Anchor?

| You want… | Anchor gives you… |
|---|---|
| Sites that feel like apps | Frameless Chromium app windows / frameless Firefox profiles (no title bar) |
| Your own icons on the dock | Launchers + Wayland `StartupWMClass` matching that actually works |
| Independence from the browser window | Per-app isolated profiles—or share the browser profile for extensions like 1Password |
| Choice of browser | Brave, Firefox, Firefox Developer Edition, Chrome, Chromium, Edge, Vivaldi, Flatpaks, plus **system default** |
| Something simple and free | MIT-licensed, no account, no telemetry, no Electron wrapper |

Web tools are where a huge amount of real work happens—email, docs, chat, dashboards, video. Anchor makes those sites first-class citizens on a Linux desktop.

---

## Who it’s for

- **Zorin OS / Ubuntu / GNOME** users who want a polished GUI, not a shell script
- People coming from **Omarchy**-style web apps who want the same idea on a stock desktop
- Anyone tired of **browser PWAs** that group under Chrome/Brave or break after updates
- Users who tried **Linux Mint WebApp Manager** and want a lightweight, modern alternative focused on isolation and dock icons

---

## Features

- **Create / edit** web apps with name + URL, profile mode, and **show/hide title bar** (Firefox frameless by default)
- **Auto-fetch favicons** (HTML icons → `/favicon.ico` → Google favicon API)
- **Custom icon upload** when a site has no usable favicon
- **Browser picker**, including **Default browser** and **Firefox Developer Edition**
- **Profile modes** per app:
  - **Isolated** (default) — private empty profile (own dock icon)
  - **Shared** — private profile **seeded** from your browser (logins & extensions like 1Password) while keeping a **separate dock icon**
- **List, launch, edit, and remove** managed apps from one window
- **Keyboard-first UI** — Tab through the create form, shortcuts for add/launch/edit/delete, **F1** shortcut reference
- **Automatic repair** of dock-matching metadata on startup (important on Wayland)

---

## How Anchor compares

Respectful comparison—different tools optimize for different environments.

| | **Anchor** | **Omarchy web apps** | **Linux Mint WebApp Manager** | **Chrome / Brave “Install app” (PWA)** |
|---|---|---|---|---|
| **UI** | Native GTK4 / libadwaita GUI | TUI menu inside Omarchy | GTK GUI | Inside the browser |
| **Best fit** | Zorin / Ubuntu / GNOME stock desktops | Full Omarchy (Hyprland) setup | Linux Mint / Cinnamon (runs elsewhere too) | Single browser ecosystem |
| **Isolation** | Isolated by default; optional shared browser profile | Typically shares browser profile | Optional isolated profiles | PWA / app profile |
| **Dock icons on Wayland** | Sets `StartupWMClass` to Chromium’s real `app_id` | Window rules / Hyprland-centric | Can need manual class fixes | Often good for official PWAs |
| **Browser choice** | Yes (default + many browsers) | Usually Chromium-family default | Yes | That browser only |
| **Electron bloat** | No—uses the browser you already have | No | No | No |
| **Cost / license** | Free · MIT | Free · part of Omarchy | Free · Mint project | Free |

**In short:** Omarchy is a full opinionated desktop; Mint’s tool is the classic Linux GUI for web apps; browser PWAs are convenient but browser-bound. **Anchor** is a focused, modern manager you can drop onto Zorin or Ubuntu and share as a standalone open-source project.

---

## Screenshots

Screenshots will live in [`docs/screenshots/`](docs/screenshots/).  
*(Add PNGs here when publishing marketing materials.)*

---

## Install

### One-liner (recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/maplepreneur/Anchor/main/install.sh | bash
```

On **Debian / Ubuntu / Zorin**, the script installs the latest GitHub Release `.deb` when one is available. Otherwise it builds from source and installs to `~/.local`.

```bash
# System-wide (.deb on Debian-like distros)
curl -fsSL https://raw.githubusercontent.com/maplepreneur/Anchor/main/install.sh | bash -s -- --system

# Always build from source (user install)
curl -fsSL https://raw.githubusercontent.com/maplepreneur/Anchor/main/install.sh | bash -s -- --from-source

# Uninstall
curl -fsSL https://raw.githubusercontent.com/maplepreneur/Anchor/main/install.sh | bash -s -- --uninstall
```

### From a clone

```bash
git clone https://github.com/maplepreneur/Anchor.git
cd Anchor
./install.sh                 # smart default
./install.sh --user          # ~/.local
./install.sh --system        # .deb / system-wide
./install.sh --from-source   # force cargo build
```

### Debian package (`.deb`)

Build a versioned package (version comes from `Cargo.toml`):

```bash
./scripts/build-deb.sh
# → dist/anchor_0.1.0_amd64.deb

sudo apt install ./dist/anchor_*.deb
```

Attach the `.deb` to a [GitHub Release](https://github.com/maplepreneur/Anchor/releases) named like `anchor_0.1.0_amd64.deb` so `install.sh` can download it.

Full requirements, PATH setup, upgrade from the old name, uninstall, and troubleshooting:

**→ [INSTALL.md](INSTALL.md)**

Maintainer deep-dive (install modes, `.deb` layout, release checklist):

**→ [docs/packaging.md](docs/packaging.md)**

---

## Usage

1. Open **Anchor**
2. Click **+** (Add Web App) or press **Ctrl+N**
3. Enter **Name** and **URL** (use **Tab** / **Shift+Tab** to move between fields)
4. Choose a **Browser** (or Default browser)
5. Choose a **Profile** mode (Isolated or Shared)
6. **Fetch icon** or **Choose image…**
7. Click **Create** or press **Enter**
8. Launch from Super search, the app menu, the ▶ button, or **Enter** on a selected row
9. Use the pencil button (or **Ctrl+E**) to **edit**; trash (or **Delete** / **Ctrl+D**) to remove with confirmation

Press **F1** or click the keyboard icon in the header for a full shortcut list.

### Keyboard shortcuts

| Action | Shortcut |
|---|---|
| Add web app | **Ctrl+N** |
| Refresh list | **Ctrl+R** or **F5** |
| Show shortcuts | **F1** or **Ctrl+?** |
| Move selection | **↑** / **↓** or **K** / **J** (list focused) |
| Launch selected | **Enter** (list focused) or **Ctrl+Enter** |
| Edit selected | **Ctrl+E** (or **E** when list focused) |
| Remove selected | **Ctrl+D** or **Delete** (confirmation dialog) |
| Next / previous field (dialog) | **Tab** / **Shift+Tab** |
| Create / Save (dialog) | **Enter** in Name or URL |
| Cancel dialog | **Esc** |

**Tips**

- **Isolated** apps start signed out. Sign in once inside each web app.
- Use **Shared browser profile** when you need password managers (e.g. 1Password) or existing logins. Anchor copies data into a **private** profile so the web app is a separate process with its **own dock icon** (unlike joining the browser’s profile, which groups under the browser—Zorin Web App Manager’s bug). Close the browser before creating/updating Shared apps for a more complete copy.
- Dock matching: `StartupWMClass` equals Chromium’s Wayland `app_id`; the list uses an absolute PNG path for favicons.

---

## How it works (brief)

| Browser family | Launch style | Profile location |
|---|---|---|
| Chromium / Chrome / Brave / Edge / Vivaldi | `--app=URL` + `--user-data-dir` (always) | `~/.local/share/anchor/profiles/<id>/` (Shared is seeded from the browser) |
| Firefox / Developer Edition / LibreWolf | Dedicated profile + `--no-remote` | `~/.local/share/anchor/firefox/<id>/` (Shared is seeded from the browser) |

Launchers: `~/.local/share/applications/webapp-*.desktop`  
On Wayland, Chromium ignores `--class` and uses a URL-based window id (e.g. `brave-www.youtube.com__-Default`). Anchor writes that into `StartupWMClass` so the dock shows the right icon.

---

## Project layout

```text
src/
  main.rs           # Application entry
  browser.rs        # Detect browsers + launch commands
  desktop.rs        # .desktop file I/O
  favicon.rs        # Icon download / normalize
  paths.rs          # XDG paths
  webapp.rs         # Create / list / delete / repair
  ui/               # GTK4 + libadwaita UI
resources/          # Desktop launcher + icon for Anchor itself
scripts/build-deb.sh  # Versioned .deb package builder
install.sh          # One-shot installer (deb or source)
INSTALL.md          # Detailed setup
docs/packaging.md   # Packaging & release documentation
```

---

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md).

```bash
cargo test
cargo build --release
```

---

## Credits

- Inspired by the simplicity of **Omarchy** web apps and the completeness of **Linux Mint WebApp Manager**
- Built with **Rust**, **GTK4**, and **libadwaita**

---

## License

[MIT](LICENSE) © Voxel North

**Free to use. Free to share. Free to improve.**
