# Changelog

## Unreleased

- Fixed a stale `Cargo.lock` that made `cargo build --locked` fail, which had broken `make package-appimage` and the Flatpak module on a clean checkout.
- AppImage checksums now record a bare filename instead of the absolute build-host path, so `sha256sum -c` works on a downloaded artifact.
- Added `make package-appimage-container`, a containerized AppImage build pinned to a base that provides `gtk4paintablesink`, so the artifact no longer depends on the host toolchain. Uses podman when available, otherwise docker. The result requires glibc 2.39 or newer.
- Documented that `gtk4paintablesink` is a hard build-host requirement which Debian 12 and Ubuntu 24.04 cannot supply, with a per-distro availability table and the corrected glibc baseline.
- AppImage build no longer requires FUSE; it falls back to the AppImage runtime's extract-and-run mode on hosts without a usable `/dev/fuse` (NixOS, containers, hardened kernels).
- Nix dev shell now provides `gst-plugins-ugly`, so `nix develop && make package-appimage` produces a complete bundle.
- Dropped the stale Ubuntu 24.04 build-host requirement from the AppImage and packaging documentation, and removed a reference to the deleted `package-all` target.

## 0.4.0 — 2026-09-30

- Added About dialog accessible from header bar showing version, MIT license, GitHub link, and description.
- AppImage build now works on any Linux distribution (tested on Ubuntu 24.04 and Arch Linux). Removed Ubuntu 24.04 OS check; third-party license collection uses distro-agnostic paths.

## 0.3.0 — 2026-09-30

- Camera tile status bar (name and audio toggle) now only appears on hover, reducing visual clutter in multi-camera views.
- Added global background color toggle in the header bar with three options: System (follows GTK theme), White, and Black. Setting persists across restarts.
- Added per-camera "Strip URI fragment" option (enabled by default) to control whether the `#fragment` portion of RTSP URLs is removed before sending to GStreamer. This restores compatibility with cameras that require the fragment for stream selection (e.g., some Dahua/Hikvision models using `#media=video`).

## 0.2.1 — 2026-09-29

- Strip client-side URI fragments before sending RTSP URLs to GStreamer, matching VLC behavior and restoring compatibility with cameras that reject fragments in RTSP request targets.

## 0.2.0 — 2026-09-29

- Renamed the application to Camstation and finalized `org.camstation.camstation`.
- Added supported Flatpak and native Nix packaging plus an AppImage recipe.
- Added MIT licensing, desktop integration, package validation, and release metadata.
- Added persistent cameras, named views, startup selection, and atomic configuration backups.
- Added graphical move, resize, snapping, overlap rejection, and direct camera placement.
- Added visible edit-grid guides, automatic grid fitting, and transactional Save/Cancel behavior.
- Added fullscreen kiosk mode, pointer hiding, and expanded-camera viewing.
- Reworked live tiles to maximize video area with compact overlay controls.
- Removed healthy “Live” and decoder text from tiles while retaining decoder diagnostics in logs.
- Added property tests, media lifecycle tests, synthetic GStreamer tests, coverage reporting, and Xvfb GTK workflows.

## 0.1.0 — 2026-09-29

- Initial multi-camera RTSP viewer with independent playback pipelines, reconnect recovery, stall detection, decoder diagnostics, and exclusive audio.
