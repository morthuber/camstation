#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

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
version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n1)"
output="$root/dist/Camstation-${version}-x86_64.AppImage"
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

# Collect third-party license files from common locations
# Try distribution package locations first, then common library paths
license_dirs=(
  "/usr/share/doc"
  "/usr/share/licenses"
)
for license_dir in "${license_dirs[@]}"; do
  if [[ -d "$license_dir" ]]; then
    find "$license_dir" -maxdepth 2 -name "copyright" -o -name "COPYING" -o -name "LICENSE*" 2>/dev/null | while IFS= read -r copyright_file; do
      if [[ -f "$copyright_file" ]]; then
        rel_path="${copyright_file#$license_dir/}"
        dest_name="${rel_path//\//-}"
        install -Dm644 "$copyright_file" "$third_party_licenses/$dest_name" 2>/dev/null || true
      fi
    done
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
