# Package validation results

Date: 2026-09-29

Build host: Arch Linux x86-64 with Nix 2.35.2, Flatpak, Wayland, and GNOME
Platform 50 installed.

## Completed

- Rust formatting, check, strict Clippy, 50 default tests, and the serialized
  Xvfb GTK workflow test passed.
- Desktop entry and AppStream metadata validated.
- `nix flake check path:.` passed.
- `nix build path:.#camstation` built the application and ran all tests in
  the Nix build sandbox.
- The Nix result launched an H.264 RTSP stream with `avdec_h264` software
  decoding.
- The Flatpak manifest built and exported an installable bundle.
- The bundle installed and exposed only network, Wayland/X11, PulseAudio, and
  DRI permissions.
- The installed Flatpak contained `gtk4paintablesink`, `avdec_h264`,
  `avdec_h265`, and `jpegdec`.
- One Flatpak H.264 stream selected `vah264dec` hardware decoding.
- Eight concurrent Flatpak H.264 pipelines ran with `vah264dec` explicitly
  disabled and each selected `avdec_h264`.
- Pinned AppImage packaging tools downloaded and passed checksum validation.

## Still requiring the selected target environments

- Build and run the Flatpak on Ubuntu 24.04 under Wayland and X11.
- Build and run the Nix package on current NixOS.
- Build the AppImage on the oldest supported glibc baseline and smoke-test it
  directly, then again on a current distribution.
- Run that AppImage through `appimage-run` on current NixOS.
- Exercise actual H.265 and MJPEG RTSP streams; their required decoder elements
  are present, but no corresponding live fixtures were available here.
- Verify camera audio in each package.

Intel VA-API certification, representative latency measurement, and the
24-hour deployment soak remain separate deployment signoff tasks.
