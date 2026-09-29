# Camview

Camview is a native Linux application for displaying multiple RTSP cameras in configurable grid layouts. The product and technical requirements are in [`spec.md`](spec.md).

## Development status

Milestone M1 is implemented. The application can render one RTSP stream through GStreamer and `gtk4paintablesink`, report pipeline state and decoder selection, minimize playback latency, and display playback errors. Validation with a real camera and Intel VA-API decoder is still hardware-dependent; multi-camera playback and reconnection are milestone M2.

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

## M1 single-camera viewer

Start the application and enter an RTSP URL in the window, or provide it at startup:

```sh
cargo run -- --rtsp-url 'rtsp://camera.local/stream'
cargo run -- --rtsp-url 'rtsp://user:password@camera.local/stream'
```

The M1 pipeline uses RTSP-over-TCP with its jitter-buffer latency set to zero, RTSP buffering disabled, stale data dropping enabled, and unsynchronized frame presentation. This always favors the lowest practical latency over jitter tolerance and smooth frame pacing. Playback starts with audio muted and reports whether the selected decoder appears to be hardware accelerated. The URL remains visible in the configuration field, but credentials are redacted from routine application logs and pipeline error text.

Use `--kiosk` with a startup URL to hide the M1 connection controls and open fullscreen:

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

VA decoder factories are registered only when libva can initialize a compatible local GPU and driver. Software decoder fallback is expected when a VA decoder is unavailable or does not support a stream's codec profile. The Nix shell includes Intel's media driver for the target deployment hardware.

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
