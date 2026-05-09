.PHONY: build release run scan check fmt lint clean install

build:
	cargo build

release:
	cargo build --release

run:
	cargo run

scan:
	cargo run -- scan

scan-force:
	cargo run -- scan --force

check:
	cargo check

fmt:
	cargo fmt

lint:
	cargo clippy

clean:
	cargo clean

install: release
	cp target/release/wip /usr/local/bin/wip
