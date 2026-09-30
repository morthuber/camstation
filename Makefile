.PHONY: check clippy fmt fmt-check run test test-media test-ui test-all coverage coverage-html flatpak-sources package-nix package-flatpak bundle-flatpak package-appimage package-appimage-container validate-packages validate-flatpak

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
	cd dist && sha256sum Camstation-$(VERSION).flatpak > Camstation-$(VERSION).flatpak.sha256

package-appimage:
	./packaging/appimage/build.sh

# Containerized AppImage build. Use this when the host toolchain is missing
# gtk4paintablesink or when a reproducible glibc floor is needed.
# See packaging/appimage/README.md for the baseline rationale.
CONTAINER_ENGINE ?= $(shell command -v podman >/dev/null 2>&1 && echo podman || echo docker)
APPIMAGE_BASE_IMAGE ?= rust:1.98.1-trixie
APPIMAGE_BUILDER_IMAGE ?= camstation-appimage-builder

package-appimage-container:
	$(CONTAINER_ENGINE) build \
		--file packaging/appimage/Containerfile \
		--build-arg BASE_IMAGE=$(APPIMAGE_BASE_IMAGE) \
		--tag $(APPIMAGE_BUILDER_IMAGE) \
		.
	@rm -rf dist/appimage-container
	@mkdir -p dist/appimage-container
	@cid=$$($(CONTAINER_ENGINE) create $(APPIMAGE_BUILDER_IMAGE)); \
		trap '$(CONTAINER_ENGINE) rm -f $$cid >/dev/null' EXIT; \
		$(CONTAINER_ENGINE) cp $$cid:/out/. dist/appimage-container/
	@echo "AppImage artifacts in dist/appimage-container/"

validate-packages:
	./packaging/validate.sh

validate-flatpak:
	./packaging/validate-flatpak.sh
