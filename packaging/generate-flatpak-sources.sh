#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
generator="${FLATPAK_CARGO_GENERATOR:-}"
if [[ -z "$generator" ]]; then
  generator="/tmp/flatpak-cargo-generator.py"
  curl --fail --location --silent --show-error \
    "https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/41c20aa10819cdb2a4f3ca171758a96d1955c018/cargo/flatpak-cargo-generator.py" \
    --output "$generator"
fi

if ! python3 -c 'import aiohttp, tomlkit' 2>/dev/null; then
  echo "flatpak-cargo-generator requires the Python aiohttp and tomlkit modules." >&2
  echo "Set FLATPAK_CARGO_GENERATOR to another installed generator if preferred." >&2
  exit 1
fi

cd "$root"
python3 "$generator" Cargo.lock -o flatpak/cargo-sources.json
