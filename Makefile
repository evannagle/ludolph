.PHONY: build install clean test lint fmt

# Build release binary
build:
	cargo build --release

# Build and install to ~/.local/bin
install: build
	@mkdir -p ~/.local/bin
	@rm -f ~/.local/bin/lu
	cp target/release/lu ~/.local/bin/lu
	@echo "Installed: ~/.local/bin/lu"

# Run tests
test:
	cargo test

# Run clippy
lint:
	cargo clippy

# Format code
fmt:
	cargo fmt

# Clean build artifacts
clean:
	cargo clean
