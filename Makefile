.PHONY: check clippy fmt fmt-check run test

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
