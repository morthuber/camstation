.PHONY: check clippy fmt fmt-check run test flatpak-sources package-nix package-flatpak bundle-flatpak package-appimage validate-packages validate-flatpak

check:
	cargo check

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

run:
	cargo run --

test:
	cargo test --all-targets --all-features

flatpak-sources:
	./packaging/generate-flatpak-sources.sh

package-nix:
	nix build path:.#camstation

package-flatpak:
	flatpak-builder --user --install-deps-from=flathub --force-clean build-dir flatpak/org.camstation.camstation.yml

bundle-flatpak: package-flatpak
	mkdir -p dist
	flatpak build-export --no-update-summary dist/flatpak-repo build-dir
	flatpak build-update-repo dist/flatpak-repo
	flatpak build-bundle dist/flatpak-repo dist/Camstation.flatpak org.camstation.camstation
	sha256sum dist/Camstation.flatpak > dist/Camstation.flatpak.sha256

package-appimage:
	./packaging/appimage/build.sh

validate-packages:
	./packaging/validate.sh

validate-flatpak:
	./packaging/validate-flatpak.sh
