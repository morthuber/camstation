#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

cargo fmt --all -- --check
cargo check --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
desktop-file-validate resources/org.camstation.camstation.desktop
appstreamcli validate --no-net --override=url-homepage-missing=pedantic \
  resources/org.camstation.camstation.metainfo.xml
bash -n packaging/appimage/build.sh packaging/appimage/fetch-tools.sh

grep -q '^app-id: org.camstation.camstation$' flatpak/org.camstation.camstation.yml
grep -q 'name = "camstation"' Cargo.toml

echo "Source and package metadata validation passed."
