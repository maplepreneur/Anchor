# Contributing to Anchor

Thanks for helping make Anchor better.

## Development setup

See [INSTALL.md](INSTALL.md) for full build dependencies. Short version:

```bash
# System packages (Debian/Ubuntu/Zorin)
sudo apt install build-essential pkg-config \
  libgtk-4-dev libadwaita-1-dev libglib2.0-dev

# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

git clone https://github.com/maplepreneur/Anchor.git
cd Anchor
cargo build
cargo test
cargo run
```

## Pull requests

1. Fork and branch from `main`
2. Keep changes focused and documented
3. Run `cargo test` and `cargo build --release` before opening a PR
4. Describe **what** changed and **why**

## Issues

Bug reports are welcome. Please include:

- Distro and desktop (e.g. Zorin OS 18, GNOME/Wayland)
- Browser used for the web app
- Steps to reproduce and expected vs actual behavior
