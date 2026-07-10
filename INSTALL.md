# Install Anchor

Complete setup instructions for building and installing **Anchor** on Linux.

## Requirements

| Requirement | Notes |
|---|---|
| Linux desktop | Designed for **Zorin OS**, Ubuntu, and other GNOME-based desktops; works anywhere XDG `.desktop` files work |
| A browser | Brave, Firefox / Firefox Developer Edition, Chrome, Chromium, Edge, Vivaldi, or similar |
| GTK 4 + libadwaita | Runtime libraries (usually preinstalled on Zorin/Ubuntu) |
| Rust toolchain | For building from source (`rustc` / `cargo`) |

## Install build dependencies

### Zorin OS / Ubuntu / Debian

```bash
sudo apt update
sudo apt install build-essential pkg-config \
  libgtk-4-dev libadwaita-1-dev libglib2.0-dev
```

### Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustc --version
```

## Build from source

```bash
git clone https://github.com/maplepreneur/Anchor.git
cd Anchor
cargo build --release
```

The binary is written to:

```text
target/release/anchor
```

Run without installing:

```bash
./target/release/anchor
# or during development:
cargo run
```

## Install for your user

This installs the binary and a desktop launcher so **Anchor** appears in the app menu.

```bash
cargo build --release

mkdir -p ~/.local/bin ~/.local/share/applications

cp target/release/anchor ~/.local/bin/
cp resources/com.voxelnorth.Anchor.desktop ~/.local/share/applications/

# Ensure ~/.local/bin is on your PATH (add to ~/.bashrc if needed)
export PATH="$HOME/.local/bin:$PATH"

update-desktop-database ~/.local/share/applications 2>/dev/null || true
```

Then open **Anchor** from Super search / the application menu, or run:

```bash
anchor
```

### PATH tip

If `anchor` is not found in a new terminal:

```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc
```

## Upgrading from “Zorin Web App Manager” (v1)

Earlier builds used:

- Binary name: `zorin-webapp-manager`
- Data dir: `~/.local/share/zorin-webapp-manager/`
- Desktop tag: `X-WebApp-Manager=zorin-webapp-manager`

Anchor **still lists and repairs** apps created with the old tag. New apps use `anchor` and `~/.local/share/anchor/`.

After installing Anchor:

1. Remove the old binary if you installed it: `rm -f ~/.local/bin/zorin-webapp-manager`
2. Replace the old desktop entry with `com.voxelnorth.Anchor.desktop` (see above)
3. Launch Anchor once so existing apps can refresh dock matching metadata

## Uninstall

```bash
rm -f ~/.local/bin/anchor
rm -f ~/.local/share/applications/com.voxelnorth.Anchor.desktop
update-desktop-database ~/.local/share/applications 2>/dev/null || true
```

Optional — remove Anchor-created web apps and data (destructive):

```bash
# Managed launchers
rm -f ~/.local/share/applications/webapp-*.desktop

# App data (icons + isolated profiles) — new and legacy paths
rm -rf ~/.local/share/anchor
rm -rf ~/.local/share/zorin-webapp-manager
```

Only delete `webapp-*.desktop` files if you are sure they were created by Anchor / the previous Zorin Web App Manager.

## Troubleshooting

### Dock shows the browser icon instead of the web app

On **Wayland**, Chromium-family browsers set a URL-based window id (for example `brave-www.youtube.com__-Default`). Anchor writes that value into `StartupWMClass`.

1. Fully quit the web app
2. Open Anchor once (repairs metadata)
3. Launch the web app again from the menu
4. Unpin any old pin that still points at the browser, then pin the web app again

### Signed out inside the web app

**Isolated** (and **Isolated with extensions**) profiles start clean by design so apps stay independent of your main browser. Sign in once inside each web app.

If you need logins or browser extensions (for example **1Password**) without signing in again, recreate the app with **Shared browser profile**. That uses your browser’s default profile. Chromium-family browsers work best; Firefox may refuse to open a second instance if the default profile is already locked.

**Isolated with extensions** copies extension data from the selected browser into a private profile when the app is created. Cookies and passwords from the main browser are not copied.

### No browsers listed

Install a supported browser and ensure it is on your `PATH`, or set a default browser:

```bash
xdg-settings get default-web-browser
```

### Build fails with missing `gtk4` / `libadwaita`

Install the **development** packages listed under [Install build dependencies](#install-build-dependencies), not only the runtime libraries.

## Packaging (later)

Prebuilt packages (`.deb`, Flatpak, GitHub Releases) are not required to use Anchor today—build from source as above. Release artifacts can be added without changing the install layout described here.
