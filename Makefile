.PHONY: check clippy fmt fmt-check run test test-media test-ui test-all coverage coverage-html flatpak-sources package-nix package-flatpak bundle-flatpak package-appimage package-all validate-packages validate-flatpak

VERSION := $(shell sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n1)

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

test-media:
	cargo test --all-features media_component -- --test-threads=1

test-ui:
	xvfb-run -a cargo test --all-features ui_integration -- --ignored --test-threads=1

test-all: test test-media test-ui

coverage:
	cargo llvm-cov --all-features --workspace

coverage-html:
	cargo llvm-cov --all-features --workspace --html --open

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
	flatpak build-bundle dist/flatpak-repo dist/Camstation-$(VERSION).flatpak org.camstation.camstation
	sha256sum dist/Camstation-$(VERSION).flatpak > dist/Camstation-$(VERSION).flatpak.sha256

package-appimage:
	./packaging/appimage/build.sh

validate-packages:
	./packaging/validate.sh

validate-flatpak:
	./packaging/validate-flatpak.sh

package-all: package-nix package-flatpak package-appimage
