# Zorin Web App Manager

A native GTK4 / libadwaita app for **Zorin OS** that turns any website into a launchable desktop application — similar in spirit to Omarchy’s web apps, with **isolated browser profiles** so each web app has its own dock icon and process lifetime.

## Features

- Enter a **name** and **URL** to create a web app
- **Auto-fetch favicon** as the desktop icon (HTML icons → `/favicon.ico` → Google favicon API)
- If favicon fetch fails, **choose a local image**
- Pick which **browser** launches the app (Brave, Firefox, Chrome, Chromium, Edge, Vivaldi, Flatpaks, …)
- Apps appear in the Zorin app menu / Super search
- **Isolated profiles**: closing your main browser leaves web apps open, and vice versa
- List, launch, and remove managed web apps

## How isolation works

| Browser family | Launch mode | Profile |
|---|---|---|
| Chromium / Chrome / Brave / Edge / Vivaldi | `--app=URL` + `--user-data-dir` | `~/.local/share/zorin-webapp-manager/profiles/<code>/` |
| Firefox / LibreWolf / similar | dedicated `--profile` + `--no-remote` + `--class` | `~/.local/share/zorin-webapp-manager/firefox/<code>/` |

Desktop files live in `~/.local/share/applications/webapp-*.desktop`.

### Dock icons (Wayland)

On **Wayland**, Chromium-family browsers **ignore `--class`** and set the window `app_id` from the site URL, e.g.:

`brave-www.youtube.com__-Default`

The manager sets `StartupWMClass` to that value so GNOME/Zorin can match the open window to the launcher and show the correct dock icon. Icons are also installed into the hicolor theme under that class name.

Existing apps are repaired automatically on startup. After updating, fully close a web app and reopen it (unpin/re-pin if an old Brave pin remains).

**Note:** Isolated profiles start signed out. Sign in once inside each web app (or use a non-isolated approach later if you prefer shared cookies).

## Build requirements

```bash
# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# GTK4 / libadwaita development packages (Ubuntu / Zorin)
sudo apt install build-essential pkg-config \
  libgtk-4-dev libadwaita-1-dev libglib2.0-dev
```

## Build & run

```bash
cargo build --release
./target/release/zorin-webapp-manager
```

Or during development:

```bash
cargo run
```

## Install launcher (optional)

```bash
cargo build --release
mkdir -p ~/.local/share/applications ~/.local/bin
cp target/release/zorin-webapp-manager ~/.local/bin/
# Edit Exec= if you install elsewhere
cp resources/com.voxelnorth.ZorinWebAppManager.desktop \
  ~/.local/share/applications/
update-desktop-database ~/.local/share/applications 2>/dev/null || true
```

Then open **Web App Manager** from the Zorin app menu.

## Usage

1. Click **+** (Add Web App)
2. Enter **Name** and **URL**
3. Choose a **Browser**
4. Click **Fetch icon** (or **Choose image…** if fetch fails)
5. Click **Create**
6. Launch from Super search, the app menu, or the **▶** button in the list

## Project layout

```
src/
  main.rs          # Application entry
  browser.rs       # Detect browsers + build Exec lines
  desktop.rs       # .desktop file I/O
  favicon.rs       # Favicon download / normalize
  paths.rs         # XDG paths
  webapp.rs        # Create / list / delete
  ui/              # GTK4 + libadwaita UI
```

## License

MIT
