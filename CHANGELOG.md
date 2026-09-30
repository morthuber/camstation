# Changelog

## 0.3.0 — 2026-09-30

- Camera tile status bar (name and audio toggle) now only appears on hover, reducing visual clutter in multi-camera views.
- Added global background color toggle in the header bar with three options: System (follows GTK theme), White, and Black. Setting persists across restarts.
- Added per-camera "Strip URI fragment" option (enabled by default) to control whether the `#fragment` portion of RTSP URLs is removed before sending to GStreamer. This restores compatibility with cameras that require the fragment for stream selection (e.g., some Dahua/Hikvision models using `#media=video`).

## 0.2.1 — 2026-09-29

- Strip client-side URI fragments before sending RTSP URLs to GStreamer, matching VLC behavior and restoring compatibility with cameras that reject fragments in RTSP request targets.

## 0.2.0 — 2026-09-29

- Renamed the application to Camstation and finalized `org.camstation.camstation`.
- Added supported Flatpak and native Nix packaging plus an AppImage beta recipe.
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
