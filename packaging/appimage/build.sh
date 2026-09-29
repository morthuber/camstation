#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

if [[ "${ALLOW_UNSUPPORTED_BUILD_HOST:-0}" != "1" ]]; then
  source /etc/os-release
  if [[ "${ID:-}" != "ubuntu" || "${VERSION_ID:-}" != "24.04" ]]; then
    echo "The beta AppImage must be built on Ubuntu 24.04." >&2
    echo "Set ALLOW_UNSUPPORTED_BUILD_HOST=1 only for packaging experiments." >&2
    exit 1
  fi
fi

required_commands=(cargo curl patchelf pkg-config)
for command_name in "${required_commands[@]}"; do
  command -v "$command_name" >/dev/null || {
    echo "missing required command: $command_name" >&2
    exit 1
  }
done

"$root/packaging/appimage/fetch-tools.sh"

tools_dir="$root/dist/appimage-tools"
appdir="$root/dist/AppDir"
output="$root/dist/Camstation-0.1.0-x86_64.AppImage"
rm -rf "$appdir"
mkdir -p "$appdir" "$root/dist"

cargo build --release --locked

export PATH="$tools_dir:$PATH"
export LINUXDEPLOY="$tools_dir/linuxdeploy-x86_64.AppImage"
export DEPLOY_GTK_VERSION=4
export GSTREAMER_INCLUDE_BAD_PLUGINS=1

"$LINUXDEPLOY" \
  --appdir "$appdir" \
  --executable "$root/target/release/camstation" \
  --desktop-file "$root/resources/org.camstation.camstation.desktop" \
  --icon-file "$root/resources/org.camstation.camstation.svg" \
  --plugin gtk \
  --plugin gstreamer

# The upstream GTK hook currently forces X11. Camstation must allow GTK to
# select native Wayland and retain X11 as a normal fallback.
sed -i '/export GDK_BACKEND=x11/d' "$appdir/apprun-hooks/linuxdeploy-plugin-gtk.sh"

# Do not inject host GTK modules into the privately bundled GTK/GLib stack.
cat > "$appdir/apprun-hooks/00-camstation.sh" <<'EOF'
#!/usr/bin/env bash
unset GTK_MODULES
EOF
chmod +x "$appdir/apprun-hooks/00-camstation.sh"

install -Dm644 LICENSE "$appdir/usr/share/licenses/camstation/LICENSE"
install -Dm644 THIRD_PARTY_NOTICES.md \
  "$appdir/usr/share/doc/camstation/THIRD_PARTY_NOTICES.md"
install -Dm644 resources/org.camstation.camstation.metainfo.xml \
  "$appdir/usr/share/metainfo/org.camstation.camstation.metainfo.xml"

third_party_licenses="$appdir/usr/share/licenses/camstation/third-party"
mkdir -p "$third_party_licenses"
for package_name in \
  libglib2.0-0t64 libgtk-4-1 libgstreamer1.0-0 \
  gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly \
  gstreamer1.0-libav gstreamer1.0-gtk4; do
  copyright_file="/usr/share/doc/$package_name/copyright"
  if [[ -f "$copyright_file" ]]; then
    install -Dm644 "$copyright_file" "$third_party_licenses/$package_name.copyright"
  fi
done

# Graphics, display-server, audio-server, and GPU driver ABIs must come from
# the target host. Bundling these is a common source of Mesa/Wayland failures.
find "$appdir/usr/lib" -maxdepth 1 \( -type f -o -type l \) \
  \( -name 'libEGL.so*' -o -name 'libGL.so*' -o -name 'libGLX.so*' \
     -o -name 'libGLdispatch.so*' -o -name 'libdrm.so*' -o -name 'libgbm.so*' \
     -o -name 'libwayland-*.so*' -o -name 'libpipewire-*.so*' \) -delete

rm -f "$output"
OUTPUT="$output" "$LINUXDEPLOY" --appdir "$appdir" --output appimage
sha256sum "$output" > "$output.sha256"

echo "Created beta artifact: $output"
echo "Validate it on Ubuntu 24.04 and with appimage-run on current NixOS."
