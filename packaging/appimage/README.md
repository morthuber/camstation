# AppImage

The AppImage is an additional best-effort artifact. Flatpak and the native Nix
package are the supported Camstation distributions.

## Build host requirements

The recipe runs on any x86-64 Linux distribution providing Rust, GTK 4,
GStreamer development packages (core, base, good, bad, ugly, libav, **and the
gtk4 plugin**), `curl`, `patchelf`, and `pkg-config`:

```sh
make package-appimage
```

The gtk4 plugin is the hard requirement; read the section below before choosing
a build host, because several otherwise-suitable distributions do not provide
it. If your host cannot supply it, use the container recipe instead of building
directly.

FUSE is optional. The build detects whether FUSE is usable and otherwise runs
`linuxdeploy` through the AppImage runtime's extract-and-run mode, which is
slower but works on NixOS, container hosts, and kernels without `/dev/fuse`.
Set `CAMSTATION_APPIMAGE_EXTRACT_AND_RUN=1` to force that path.

### The `gtk4paintablesink` requirement

Camstation will not start a pipeline without the GStreamer element
`gtk4paintablesink` (`libgstgtk4.so`); `src/media/controller.rs` rejects the
element explicitly. The build host must therefore *have that plugin installed*
so that `linuxdeploy` bundles it into the image.

This matters because the plugin is a comparatively new Rust-based GStreamer
component, and distributions package it late. It is **not** available on the
older distributions you would otherwise prefer for a low glibc floor:

| Distribution | glibc | `gtk4paintablesink` available |
| --- | --- | --- |
| Ubuntu 22.04 (jammy) | 2.35 | no |
| Debian 12 (bookworm) | 2.36 | no — only `gstreamer1.0-gtk3` exists |
| Ubuntu 24.04 (noble) | 2.39 | no — no GTK 4 GStreamer package at all |
| openSUSE Leap 15.6 | 2.40 | yes |
| Fedora 41 | 2.40 | yes (`rust-gst-plugin-gtk4`) |
| Debian 13 (trixie) | 2.41 | yes (`gstreamer1.0-gtk4`) |
| Arch Linux | rolling | yes (`gst-plugin-gtk4`) |
| Nix dev shell | nixpkgs | yes (`gst-plugins-rs`) |

Because the plugin is a runtime dependency, a build host without it still
*compiles* successfully and produces an AppImage that installs cleanly and
then fails at startup. Always confirm before building:

```sh
gst-inspect-1.0 gtk4paintablesink
```

### glibc baseline

The build host does not need to be any particular distribution, but it does
determine **which systems the resulting AppImage can run on**. An AppImage
carries the glibc it was linked against, so it only runs on hosts with an equal
or newer glibc. Building on a rolling-release distribution therefore produces
an artifact that excludes most older systems.

The floor is set by the highest `GLIBC_x.y` symbol referenced by any bundled
library, not by the build host's own glibc. The container build below uses
Debian 13 (glibc 2.41) but the resulting artifact only requires **glibc 2.39**,
because that is what its bundled libraries actually reference. It therefore runs
on Ubuntu 24.04 LTS and newer, but not on Ubuntu 22.04 or Debian 12.

Check a target machine with:

```sh
getconf GNU_LIBC_VERSION
```

### Containerized build

Because the plugin requirement and the glibc floor pull in opposite directions,
the container recipe is the most reliable way to build. It pins a base that is
known to provide the plugin, so the result does not depend on the host:

```sh
make package-appimage-container
```

This uses podman when available and otherwise docker, and writes the artifact
plus its checksum to `dist/appimage-container/`. Artifacts land in
`dist/appimage-container/` rather than `dist/` so a container build never
clobbers a host build.

Override the base image if you want a different toolchain. It must still ship
`gtk4paintablesink`:

```sh
BASE_IMAGE=rust:1.98.1-bookworm make package-appimage-container
```

The build fails immediately if the base image cannot supply
`gtk4paintablesink`, so a base that would produce a broken image is rejected
before any compilation happens. Lowering the glibc floor is not possible by
substituting an older base, because those bases cannot supply the plugin at
all.

### Distribution packages

Debian 13 / Ubuntu 25.04 and newer:

```sh
sudo apt install cargo rustc libgtk-4-dev libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-dev libgstreamer-plugins-bad1.0-dev \
    gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly \
    gstreamer1.0-libav gstreamer1.0-gtk4 \
    curl patchelf pkgconf
```

Fedora:

```sh
sudo dnf install cargo rust gtk4-devel gstreamer1-devel \
    gstreamer1-plugins-base-devel gstreamer1-plugins-bad-devel \
    gstreamer1-plugins-good gstreamer1-plugins-bad gstreamer1-plugins-ugly \
    gstreamer1-libav "rust-gst-plugin-gtk4+default" \
    curl patchelf pkgconf-pkg-config
```

openSUSE Leap 15.6 and newer:

```sh
sudo zypper install cargo rust gtk4-devel gstreamer-devel \
    gstreamer-devel-plugins-base gstreamer-devel-plugins-bad \
    gstreamer-plugins-good gstreamer-plugins-bad gstreamer-plugins-ugly \
    gstreamer-libav gstreamer-plugin-gtk4 curl patchelf pkg-config
```

Arch Linux:

```sh
sudo pacman -S rust gtk4 gstreamer gst-plugins-base gst-plugins-good \
    gst-plugins-bad gst-plugins-ugly gst-libav gst-plugin-gtk4 \
    curl patchelf pkgconf
```

The Nix dev shell already provides everything the recipe needs, including
`gst-plugins-rs`:

```sh
nix develop
make package-appimage
```

## What the recipe does

It pins and verifies its `linuxdeploy` tools by checksum. It patches the
upstream GTK hook so GTK can use native Wayland rather than forcing X11.
GStreamer core and plugins are isolated inside the image, while
hardware-sensitive display, GPU, and audio-server libraries are supplied by the
host.

The build collects third-party license files from common system locations
(`/usr/share/doc`, `/usr/share/licenses`) rather than relying on distribution
package names.

Software decoding of H.264, H.265, and MJPEG is the acceptance baseline.
Hardware decoding is opportunistic and may vary with host drivers.

## Running the AppImage

On NixOS, use:

```sh
nix-shell -p appimage-run
appimage-run ./dist/Camstation-*-x86_64.AppImage
```

On systems without working FUSE, the AppImage runtime supports
`--appimage-extract-and-run`, though repeated extraction is slower.
