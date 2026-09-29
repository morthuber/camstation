# Camview

Camview is a native Linux application for displaying multiple RTSP cameras in configurable grid layouts. The product and technical requirements are in [`spec.md`](spec.md).

## Development status

Milestone M2 is implemented. The application can display up to ten independent RTSP streams, automatically recover failed or stalled pipelines, select one camera for audio, report decoder diagnostics, and minimize playback latency. Configuration persistence and saved views are milestone M3. Validation with the target Intel VA-API hardware remains deployment-machine dependent.

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

## M2 multi-camera viewer

Start the application and add cameras in the window, or repeat `--rtsp-url` up to ten times:

```sh
cargo run -- \
  --rtsp-url 'rtsp://camera-one.local/stream' \
  --rtsp-url 'rtsp://camera-two.local/stream'
```

Each tile owns an independent GStreamer pipeline. Failed streams retain their tile and reconnect after `1s`, `2s`, `5s`, `10s`, `15s`, then `30s`; the delay resets after 20 seconds of healthy frame delivery. A watchdog reconnects streams that produce no initial frame for 10 seconds or stop delivering frames for 5 seconds. Removing a tile stops its pipeline and releases its timers, bus watch, and frame probe.

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
