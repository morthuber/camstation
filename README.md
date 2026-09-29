# Camview

Camview is a native Linux application for displaying multiple RTSP cameras in configurable grid layouts. The product and technical requirements are in [`spec.md`](spec.md).

## Development status

Milestone M3 is implemented. Camview persists cameras and named views, restores a selected startup view, renders saved grid positions and spans, and provides camera and view managers. The application can display up to ten independent RTSP streams, automatically recover failed or stalled pipelines, select one camera for audio, report decoder diagnostics, and minimize playback latency. Graphical drag-and-resize layout editing remains milestone M4. Validation with the target Intel VA-API hardware remains deployment-machine dependent.

## Prerequisites

The recommended environment is the included Nix flake:

```sh
nix develop
cargo run
```

On a non-Nix development system, install:

- Rust and Cargo
- GTK 4 development files
- GStreamer development files
- GStreamer base, good, bad, ugly, libav, and Rust plugins; with GStreamer 1.28, modern VA decoders are provided by the bad plugin set
- `pkg-config`

Then run:

```sh
cargo run
```

## M3 configuration and views

Use **Cameras…** to add, edit, test, or remove RTSP cameras. A failed connection test does not prevent saving an unavailable camera. Use **Views…** to create, rename, duplicate, or remove named views, choose their cameras, select the startup view, and configure kiosk-on-start. The view selector in the main window changes the active view.

Configuration is stored at `$XDG_CONFIG_HOME/camview/config.json`, or at `$HOME/.config/camview/config.json` when `XDG_CONFIG_HOME` is unset. Writes use an atomic replacement, restrict newly created files to the current user on Unix, and retain the preceding valid file as `config.json.bak`. An invalid existing file is reported and is never silently replaced.

Use another configuration file or override the startup view by UUID or case-insensitive name:

```sh
cargo run -- --config ./cameras.json --view Overview
```

Views store logical grid dimensions and each tile's row, column, and spans. Camview renders that geometry, while the M3 view editor automatically places newly assigned cameras into free cells. Direct manipulation by dragging and resizing tiles is planned for M4.

Repeated `--rtsp-url` arguments remain available for temporary, non-persisted streams. They are placed in free cells in the selected view:

```sh
cargo run -- \
  --rtsp-url 'rtsp://camera-one.local/stream' \
  --rtsp-url 'rtsp://camera-two.local/stream'
```

## Reliable multi-camera playback

Each tile owns an independent GStreamer pipeline. Failed streams retain their tile and reconnect after `1s`, `2s`, `5s`, `10s`, `15s`, then `30s`; the delay resets after 20 seconds of healthy frame delivery. A watchdog reconnects streams that produce no initial frame for 10 seconds or stop delivering frames for 5 seconds. Changing views or closing the application stops hidden pipelines and releases their timers, bus watches, and frame probes.

All cameras start muted. Enabling a tile's **Audio** toggle first mutes every other camera, so at most one stream is audible. Mute intent survives pipeline reconnection.

Pipelines use RTSP-over-TCP with jitter-buffer latency set to zero, RTSP buffering disabled, stale data dropping enabled, and unsynchronized frame presentation. This favors the lowest practical latency over jitter tolerance and smooth frame pacing. The URL is visible in the add-camera field, but credentials are redacted from routine application logs and pipeline error text.

A working local development stream is available for repeatable playback tests:

```sh
cargo run -- --rtsp-url 'rtsp://127.0.0.1:8554/city-traffic' --log camview=debug
```

At the time it was added, this H.264 stream reached `Playing` and selected `avdec_h264` on the AMD development workstation.

Use `--kiosk` with startup URLs to hide camera-management controls and open fullscreen:

```sh
cargo run -- --kiosk --rtsp-url 'rtsp://camera.local/stream'
```

## Common commands

```sh
make fmt
make check
make clippy
make test
```

To inspect relevant GStreamer plugins:

```sh
gst-inspect-1.0 gtk4paintablesink
vainfo
gst-inspect-1.0 vah264dec
gst-inspect-1.0 vah265dec
```

GStreamer automatically selects among compatible installed decoders. Camview recognizes GStreamer's `Hardware` decoder metadata and common VA-API, Intel QSV/MSDK, NVIDIA, V4L2, Vulkan, AMD AMF, and platform decoder factory names. Software fallback is expected when no compatible hardware decoder is registered or when the hardware does not support the stream's codec profile. The Nix shell includes Intel's media driver because Intel is the initial deployment target; other vendors require their corresponding system driver and GStreamer plugin.

## Logging

Camview uses `tracing`. Set `RUST_LOG` for application logs and `GST_DEBUG` for GStreamer diagnostics:

```sh
RUST_LOG=camview=debug GST_DEBUG=2 cargo run
```

RTSP URLs may contain credentials and must not be written unredacted to routine logs.

## Optional direnv integration

If `direnv` and `nix-direnv` are installed:

```sh
cp .envrc.example .envrc
direnv allow
```

`.envrc` is intentionally not generated automatically because enabling it is a per-user decision.
