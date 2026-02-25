# Ludolph MCP Server - Development Commands

.PHONY: fmt lint test check install-hooks

# Format code with black and ruff
fmt:
	python3 -m black --line-length=100 .
	python3 -m ruff check --fix .

# Check formatting and linting without fixing
lint:
	python3 -m black --check --line-length=100 .
	python3 -m ruff check .

# Run tests
test:
	python3 -m unittest discover -s tests -v

# Run all checks (format + lint + test)
check: fmt test

# Install pre-commit hooks (run once after cloning)
install-hooks:
	pip3 install --user pre-commit black ruff
	cd ../.. && pre-commit install
	@echo "Pre-commit hooks installed. Formatting will run automatically on commit."

# Install dev dependencies
install-dev:
	pip3 install --user black ruff pytest flask
