# Camstation

Camstation is a native Linux application for displaying multiple RTSP cameras in configurable grid layouts. The product and technical requirements are in [`spec.md`](spec.md).

![Camstation screenshot](screenshot.png)

The demo streams shown in the screenshot use [Fake-RTSP-Stream](https://github.com/insight-platform/Fake-RTSP-Stream/) as an image source.

## Quick Start

Camstation is distributed through three packaging formats:

| Format | Install Command |
|--------|-----------------|
| **Flatpak** (recommended) | `flatpak install flathub org.camstation.camstation` |
| **Nix** | `nix run github:orthuber/camstation` or add `camstation` to your NixOS configuration |
| **AppImage** | Download from [releases](https://github.com/orthuber/camstation/releases) and run directly |

### First Run

1. Launch Camstation
2. Open **Cameras…** to add your RTSP camera URLs
3. Open **Views…** to create a named view and assign cameras
4. Use **Edit layout** to arrange the grid (drag to move, ↘ handle to resize)
5. Press **F11** for kiosk mode, or start with `--kiosk`

## User Guide

### Configuration and Views

- **Cameras…** — Add, edit, test, or remove RTSP cameras. A failed connection test does not prevent saving.
- **Views…** — Create, rename, duplicate, or remove named views. Choose cameras, select the startup view, and configure kiosk-on-start.
- The view selector in the main window changes the active view.

Configuration is stored at `$XDG_CONFIG_HOME/camstation/config.json` (or `$HOME/.config/camstation/config.json`). Writes use atomic replacement and retain the preceding valid file as `config.json.bak`.

Override config or startup view:
```sh
camstation --config ./cameras.json --view Overview
```

Views store logical grid dimensions and each tile's row, column, and spans. Newly assigned cameras are automatically placed into free cells.

### Layout Editing and Kiosk Mode

- **Edit layout** — Arrange the active view while streams continue playing
  - Drag anywhere on a camera image to move it
  - Drag the **↘** handle to resize
  - Use row/column controls to set the logical grid size
  - **Fit grid** trims empty trailing rows/columns
  - Invalid overlaps are rejected and snap back
  - **Save layout** persists changes; **Cancel** restores the original

- **Double-click** video to expand a camera; double-click again or press **Escape** to restore the grid
- **F11** — Toggle kiosk mode (fullscreen, hides controls, hides pointer after 3s inactivity)
- `--kiosk` forces kiosk mode; `--windowed` overrides saved kiosk-on-start

See [Unattended startup](docs/unattended-startup.md) for XDG autostart and systemd user-service examples.

### Temporary Streams

Repeated `--rtsp-url` arguments add temporary, non-persisted streams:
```sh
camstation \
  --rtsp-url 'rtsp://camera-one.local/stream' \
  --rtsp-url 'rtsp://camera-two.local/stream'
```

### Reliable Multi-Camera Playback

- Each tile owns an independent GStreamer pipeline
- Failed streams reconnect with exponential backoff (1s, 2s, 5s, 10s, 15s, 30s; resets after 20s healthy)
- Watchdog reconnects streams with no initial frame for 10s or stalled for 5s
- Changing views or closing stops hidden pipelines and releases resources
- **Audio**: All cameras start muted. Enabling one mutes all others (exclusive selection). Mute intent survives reconnection
- **Latency**: RTSP-over-TCP, zero jitter-buffer, disabled RTSP buffering, stale data dropping, unsynchronized presentation — favors lowest practical latency

Credentials in RTSP URLs are redacted from routine logs and pipeline error text.

### Logging

```sh
# Debug logging
RUST_LOG=camstation=debug GST_DEBUG=2 camstation

# Unattended deployments — daily rotation (7 files retained)
camstation --kiosk --log-file /var/log/camstation
```

Without `--log-file`, logs write to stderr (suitable for systemd journal). RTSP credentials are never written unredacted.

### Inspecting GStreamer Plugins

```sh
gst-inspect-1.0 gtk4paintablesink
vainfo
gst-inspect-1.0 vah264dec
gst-inspect-1.0 vah265dec
```

GStreamer auto-selects among compatible decoders. Camstation recognizes `Hardware` decoder metadata and common VA-API, Intel QSV/MSDK, NVIDIA, V4L2, Vulkan, AMD AMF, and platform decoder factory names. Software fallback activates when no compatible hardware decoder is registered or the profile is unsupported.

## Development Setup

### Prerequisites

**Recommended (Nix flake):**
```sh
nix develop
cargo run
```

**Non-Nix systems:** Install Rust, GTK 4 dev files, GStreamer dev files (core, base, good, bad, ugly, libav, gtk4 plugins), and `pkg-config`. Then:
```sh
cargo run
```

### Common Commands

```sh
# Code quality
make fmt         # Format code
make fmt-check   # Check formatting
make check       # cargo check
make clippy      # Lint with deny-warnings

# Testing
make test        # All unit/integration tests
make test-media  # Media component tests (serial)
make test-ui     # UI integration tests (requires xvfb)
make coverage    # Coverage report
make coverage-html  # HTML coverage report (opens in browser)

# Packaging
make package-nix       # Build Nix package
make package-flatpak   # Build Flatpak
make bundle-flatpak    # Build Flatpak bundle (.flatpak file)
make package-appimage  # Build AppImage (oldest-supported glibc host)
make validate-packages # Run validation checklist
```

See [`docs/testing.md`](docs/testing.md) for the test-suite split, coverage baseline, and graphical test requirements.

### Optional direnv Integration

If `direnv` and `nix-direnv` are installed:
```sh
cp .envrc.example .envrc
direnv allow
```
`.envrc` is not generated automatically — enabling it is a per-user decision.

## Packaging & Distribution

Build commands for maintainers and CI:

```sh
# Nix
nix build path:.#camstation
nix run path:.#camstation -- --help

# Flatpak (requires GNOME Platform/SDK 50 + Freedesktop 25.08 Rust extension)
make bundle-flatpak
flatpak install --user --reinstall dist/Camstation-$(VERSION).flatpak
flatpak run org.camstation.camstation
```
The Flatpak has network, display, GPU, and audio access but no general host home-directory access. See [`flatpak/README.md`](flatpak/README.md) for details.

```sh
# AppImage (build on the oldest glibc host you need to support)
make package-appimage
```
See [`packaging/appimage/README.md`](packaging/appimage/README.md).

Validation checklists:
- [`docs/package-validation.md`](docs/package-validation.md) — Repeatable package checklist
- [`docs/package-validation-results.md`](docs/package-validation-results.md) — Latest completed/pending matrix

## Architecture Overview

| Concern | Decision |
|---------|----------|
| Language | Rust, edition 2024 |
| GUI | GTK 4 via gtk-rs |
| Media | GStreamer 1.x via gstreamer-rs |
| Video presentation | `gtk4paintablesink` |
| Hardware decoding | Available GStreamer hardware decoders, selected through decoder autoplugging |
| Configuration | Versioned JSON via Serde |
| Logging | `tracing` with environment-filter support |
| CLI | `clap` |

## License

Camstation is licensed under the [MIT License](LICENSE). Packaged third-party components retain their own licenses; see [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).