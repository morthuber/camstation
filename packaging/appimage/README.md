# AppImage

The AppImage is an additional best-effort artifact. Flatpak and the native Nix
package are the supported Camstation distributions.

Build it on any x86-64 Linux distribution with Rust, GTK 4, GStreamer
development packages (core, base, good, bad, ugly, libav, and gtk4 plugins),
`curl`, `patchelf`, `pkg-config`, and FUSE 2 support:

```sh
make package-appimage
```

The recipe pins and verifies its linuxdeploy tools. It patches the upstream GTK
hook so GTK can use native Wayland rather than forcing X11. GStreamer core and
plugins are isolated inside the image, while hardware-sensitive display, GPU,
and audio-server libraries are supplied by the host.

Software decoding of H.264, H.265, and MJPEG is the acceptance baseline.
Hardware decoding is opportunistic and may vary with host drivers.

On NixOS, use:

```sh
nix-shell -p appimage-run
appimage-run ./dist/Camstation-0.3.0-x86_64.AppImage
```

On systems without working FUSE, the AppImage runtime supports
`--appimage-extract-and-run`, though repeated extraction is slower.

## Building on non-Ubuntu distributions

The build script works on any distribution with the required dependencies.
Tested on Ubuntu 24.04 and Arch Linux. On Arch, install dependencies with:

```sh
pacman -S rust gtk4 gstreamer gst-plugins-base gst-plugins-good \
    gst-plugins-bad gst-plugins-ugly gst-libav gst-plugin-gtk \
    curl patchelf pkgconf fuse2
```

The build collects third-party license files from common system locations
(`/usr/share/doc`, `/usr/share/licenses`) rather than relying on specific
package names.
