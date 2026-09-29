# Camview

Camview is a native Linux application for displaying multiple RTSP cameras in configurable grid layouts. The product and technical requirements are in [`spec.md`](spec.md).

## Development status

The project is at milestone M0. The current application is a GTK/GStreamer initialization smoke test; camera playback and layout editing are the next milestones.

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
gst-inspect-1.0 vah264dec
gst-inspect-1.0 vah265dec
```

Software decoder fallback is expected when a VA decoder is unavailable or does not support a stream's codec profile.

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
