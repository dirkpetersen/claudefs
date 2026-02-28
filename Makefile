.PHONY: build test clippy fmt doc check help all clean

help:
	@echo "ClaudeFS Development Commands"
	@echo "============================="
	@echo "make build        - Build all crates"
	@echo "make test         - Run all tests"
	@echo "make test-storage - Test claudefs-storage crate"
	@echo "make test-meta    - Test claudefs-meta crate"
	@echo "make test-reduce  - Test claudefs-reduce crate"
	@echo "make test-transport - Test claudefs-transport crate"
	@echo "make test-fuse    - Test claudefs-fuse crate"
	@echo "make test-repl    - Test claudefs-repl crate"
	@echo "make test-gateway - Test claudefs-gateway crate"
	@echo "make test-mgmt    - Test claudefs-mgmt crate"
	@echo "make clippy       - Run clippy linter"
	@echo "make fmt          - Check code formatting"
	@echo "make fmt-fix      - Fix code formatting"
	@echo "make doc          - Generate documentation"
	@echo "make check        - Run all checks (build, test, clippy, fmt, doc)"
	@echo "make clean        - Remove build artifacts"

all: check

build:
	cargo build --verbose

test:
	cargo test --verbose

test-storage:
	cargo test --package claudefs-storage --verbose

test-meta:
	cargo test --package claudefs-meta --verbose

test-reduce:
	cargo test --package claudefs-reduce --verbose

test-transport:
	cargo test --package claudefs-transport --verbose

test-fuse:
	cargo test --package claudefs-fuse --verbose

test-repl:
	cargo test --package claudefs-repl --verbose

test-gateway:
	cargo test --package claudefs-gateway --verbose

test-mgmt:
	cargo test --package claudefs-mgmt --verbose

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

fmt:
	cargo fmt --all -- --check

fmt-fix:
	cargo fmt --all

doc:
	cargo doc --no-deps --document-private-items
	@echo "Documentation built in target/doc/claudefs_*/index.html"

check: build test clippy fmt doc
	@echo "All checks passed!"

clean:
	cargo clean
	rm -rf target/
