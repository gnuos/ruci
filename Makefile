# Ruci CI Makefile
# Simple CI/CD workflow for Ruci project

.PHONY: all build check test lint clean run stop status logs help

# Default target
all: build

## Build
build:
	cargo build --release
	@mkdir -p bin
	@cp target/release/rucid bin/ 2>/dev/null || cp target/release/rucid bin/
	@cp target/release/ruci bin/ 2>/dev/null || cp target/release/ruci bin/
	@echo "Build complete: bin/rucid, bin/ruci"

build-dev:
	cargo build
	@mkdir -p bin
	@cp target/debug/rucid bin/ 2>/dev/null || cp target/debug/rucid bin/
	@cp target/debug/ruci bin/ 2>/dev/null || cp target/debug/ruci bin/
	@echo "Dev build complete: bin/rucid, bin/ruci"

## Check (fast type checking)
check:
	cargo check --all

## Test
test:
	cargo test

test-all: test fmt-check clippy

## MySQL Tests (requires MySQL instance)
test-mysql:
	MYSQL_URL="mysql://root:password@localhost/test_ruci" cargo test -p ruci-core -- mysql
	@echo "Note: MySQL tests require a running MySQL instance"

## Lint
fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

clippy:
	cargo clippy --all --all-targets -- -D warnings

## Clean
clean:
	cargo clean
	rm -rf bin/

dev:
	RUST_LOG=debug ./bin/rucid --config contrib/ruci.yaml.example

## Install (system-wide)
install:
	install -m 755 bin/rucid /usr/local/bin/rucid
	install -m 755 bin/ruci /usr/local/bin/ruci
	@echo "Installed to /usr/local/bin/"

## Docker
docker-build:
	docker build -f contrib/docker/Dockerfile -t rucid:latest ..

## Help
help:
	@echo "Ruci CI Makefile"
	@echo ""
	@echo "Targets:"
	@echo "  build         - Release build (output to bin/)"
	@echo "  build-dev     - Development build"
	@echo "  check         - Fast type checking (no binary output)"
	@echo "  test          - Run tests"
	@echo "  test-all      - Run tests, fmt check, clippy"
	@echo "  test-mysql    - Run MySQL tests (requires MySQL instance)"
	@echo "  fmt           - Format code"
	@echo "  fmt-check     - Check code formatting"
	@echo "  clippy        - Run clippy linter"
	@echo "  clean         - Clean build artifacts"
	@echo "  dev           - Run rucid with debug logging"
	@echo "  install       - Install binaries system-wide"
	@echo "  docker-build  - Build Docker image"
