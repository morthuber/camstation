#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tools_dir="$root/dist/appimage-tools"
mkdir -p "$tools_dir"

fetch() {
  local url="$1"
  local output="$2"
  local expected="$3"
  if [[ ! -f "$output" ]]; then
    curl --fail --location --silent --show-error \
      --header "Accept: application/octet-stream" "$url" --output "$output"
  fi
  echo "$expected  $output" | sha256sum --check --status || {
    echo "checksum verification failed for $output" >&2
    exit 1
  }
  chmod +x "$output"
}

fetch \
  "https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20251107-1/linuxdeploy-x86_64.AppImage" \
  "$tools_dir/linuxdeploy-x86_64.AppImage" \
  "c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d"
fetch \
  "https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/7a3fbc31a9e5075073ff8790f26effbac5f84453/linuxdeploy-plugin-gtk.sh" \
  "$tools_dir/linuxdeploy-plugin-gtk.sh" \
  "b0f4cbc684a0103a9651f0955b635eaea0096b3a66c0f5a2c2aa337960375171"
fetch \
  "https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gstreamer/2a2e67491c32995a3f279ad0ecbe77abd512b42a/linuxdeploy-plugin-gstreamer.sh" \
  "$tools_dir/linuxdeploy-plugin-gstreamer.sh" \
  "c107b49d84edbffc6ab226ed1007e0626a4f7aa2c3a36b7782bef62351d49e94"

echo "AppImage tools are available in $tools_dir"
