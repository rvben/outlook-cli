.PHONY: build test lint fmt check run clean

build:
	cargo build --locked

test:
	cargo test --locked --all-targets

lint:
	cargo fmt -- --check
	cargo clippy --locked --all-targets -- -D warnings

fmt:
	cargo fmt

check: lint test

run:
	cargo run --locked --

clean:
	cargo clean
