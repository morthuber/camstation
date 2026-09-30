# AppImage

The AppImage is an additional best-effort artifact. Flatpak and the native Nix
package are the supported Camstation distributions.

## Build host requirements

The recipe runs on any x86-64 Linux distribution providing Rust, GTK 4,
GStreamer development packages (core, base, good, bad, ugly, libav, and gtk4
plugins), `curl`, `patchelf`, and `pkg-config`:

```sh
make package-appimage
```

FUSE is optional. The build detects whether FUSE is usable and otherwise runs
`linuxdeploy` through the AppImage runtime's extract-and-run mode, which is
slower but works on NixOS, container hosts, and kernels without `/dev/fuse`.
Set `CAMSTATION_APPIMAGE_EXTRACT_AND_RUN=1` to force that path.

### glibc baseline

The build host does not need to be any particular distribution, but it does
determine **which systems the resulting AppImage can run on**. An AppImage
carries the glibc it was linked against, so it only runs on hosts with an equal
or newer glibc. Building on a rolling-release distribution therefore produces
an artifact that excludes most older systems.

Build on the *oldest* distribution you still need to support:

| Distribution | glibc | Suitable for artifacts running on |
| --- | --- | --- |
| Debian 11 (bullseye) | 2.31 | very old systems, but GTK 4 / GStreamer are too old to build against |
| Ubuntu 20.04 (focal) | 2.31 | as above |
| Ubuntu 22.04 (jammy) | 2.35 | Ubuntu 22.04 and newer |
| Debian 12 (bookworm) | 2.36 | Debian 12 and newer |
| Ubuntu 24.04 (noble) | 2.39 | Ubuntu 24.04 and newer |
| openSUSE Leap 15.6 | 2.40 | openSUSE Leap 15.6 and newer |
| Arch Linux | rolling | recent distributions only |

Debian 12 is the recommended baseline: old enough to cover the widest range of
targets, new enough that GTK 4 and GStreamer are recent. For a reproducible
build on that baseline regardless of host, use the container recipe below.

### Distribution packages

Debian / Ubuntu:

```sh
sudo apt install cargo rustc libgtk-4-dev libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-dev libgstreamer-plugins-bad1.0-dev \
    gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly \
    gstreamer1.0-libav libgstreamer-plugins-gtk4-1.0-0 \
    curl patchelf pkgconf
```

Fedora:

```sh
sudo dnf install cargo rust gtk4-devel gstreamer1-devel \
    gstreamer1-plugins-base-devel gstreamer1-plugins-bad-devel \
    gstreamer1-plugins-good gstreamer1-plugins-bad gstreamer1-plugins-ugly \
    gstreamer1-libav gstreamer1-gtk4 curl patchelf pkgconf-pkg-config
```

openSUSE:

```sh
sudo zypper install cargo rust gtk4-devel gstreamer-devel \
    gstreamer-devel-plugins-base gstreamer-devel-plugins-bad \
    gstreamer-plugins-good gstreamer-plugins-bad gstreamer-plugins-ugly \
    gstreamer-libav gstreamer-plugin-gtk4 curl patchelf pkg-config
```

Arch Linux:

```sh
sudo pacman -S rust gtk4 gstreamer gst-plugins-base gst-plugins-good \
    gst-plugins-bad gst-plugins-ugly gst-libav gst-plugin-gtk \
    curl patchelf pkgconf
```

The Nix dev shell already provides everything the recipe needs:

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
