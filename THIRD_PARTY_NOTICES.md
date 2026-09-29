# Third-party components

Camstation is distributed under the MIT license. Its packages dynamically use
third-party components under their own licenses, including:

- GTK and GLib (LGPL-2.1-or-later)
- GStreamer core and the base, good, bad, and libav plugin sets (primarily
  LGPL-2.1-or-later; individual plugin metadata is authoritative)
- `gst-plugin-gtk4` / `gtk4paintablesink` (MPL-2.0)
- FFmpeg through `gst-libav` (LGPL builds are required by Camstation's package
  recipes)
- Rust crates listed in `Cargo.lock`, under the licenses published by their
  respective authors

The Flatpak uses media components supplied and updated by the GNOME and
Freedesktop runtimes. The Nix package composes the corresponding nixpkgs
packages. The beta AppImage bundles copies of its userspace dependencies and
must ship their license texts when it is prepared for release.

H.264 and H.265/HEVC can be subject to patents in some jurisdictions. Software
licenses do not grant patent rights. Anyone redistributing a binary package is
responsible for determining the obligations that apply in their jurisdiction.
