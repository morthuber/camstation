# Flatpak packaging

Camstation targets GNOME Platform 50, whose GStreamer stack includes
`gtk4paintablesink`, LGPL gst-libav H.264/H.265 decoders, and MJPEG decoding.

Install the SDK and Rust extension, then build a local bundle:

```sh
flatpak install --user flathub org.gnome.Platform//50 org.gnome.Sdk//50 \
  org.freedesktop.Sdk.Extension.rust-stable//25.08
make bundle-flatpak
flatpak install --user --reinstall dist/Camstation-0.2.0.flatpak
flatpak run org.camstation.camstation
```

`cargo-sources.json` is generated from `Cargo.lock`. Regenerate it with
`make flatpak-sources` whenever Rust dependencies change.

The manifest grants network, display, GPU, and audio access. It does not grant
general access to the host home directory. Configuration is stored in the
Flatpak application's private XDG configuration directory.
