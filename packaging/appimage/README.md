# AppImage beta

The AppImage is an additional best-effort artifact. Flatpak and the native Nix
package are the supported Camstation distributions.

Build it in an x86-64 Ubuntu 24.04 environment after installing Rust, GTK 4,
GStreamer development packages, the base/good/bad/ugly/libav plugin packages,
`gstreamer1.0-gtk4`, `curl`, `patchelf`, `pkg-config`, and FUSE 2 support:

```sh
make package-appimage
```

The recipe pins and verifies its linuxdeploy tools. It patches the upstream GTK
hook so GTK can use native Wayland rather than forcing X11. GStreamer core and
plugins are isolated inside the image, while hardware-sensitive display, GPU,
and audio-server libraries are supplied by the host.

Software decoding of H.264, H.265, and MJPEG is the beta acceptance baseline.
Hardware decoding is opportunistic and may vary with host drivers.

On NixOS, use:

```sh
nix-shell -p appimage-run
appimage-run ./dist/Camstation-0.1.0-x86_64.AppImage
```

On systems without working FUSE, the AppImage runtime supports
`--appimage-extract-and-run`, though repeated extraction is slower.
