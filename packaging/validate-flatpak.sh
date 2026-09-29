#!/usr/bin/env bash
set -euo pipefail

app_id="org.camstation.camstation"
flatpak info --user "$app_id" >/dev/null || {
  echo "Install dist/Camstation.flatpak for the current user first." >&2
  exit 1
}

for element in gtk4paintablesink avdec_h264 avdec_h265 jpegdec; do
  flatpak run --command=gst-inspect-1.0 "$app_id" "$element" >/dev/null || {
    echo "missing required Flatpak GStreamer element: $element" >&2
    exit 1
  }
done

permissions="$(flatpak info --user --show-permissions "$app_id")"
for permission in 'shared=network' 'sockets=fallback-x11;pulseaudio;wayland' 'devices=dri'; do
  grep -Fq "$permission" <<<"$permissions" || {
    echo "missing expected Flatpak permission: $permission" >&2
    exit 1
  }
done

flatpak run "$app_id" --help >/dev/null
echo "Flatpak runtime, codecs, permissions, and command validation passed."
