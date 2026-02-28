# justfile – project task runner (https://github.com/casey/just)

# Default: list available recipes
default:
    @just --list

# ── Development ────────────────────────────────────────────────────────────────

# Build the project
build:
    cargo build

# Build the project in release mode
build-release:
    cargo build --release

# Run the service locally (requires a running Postgres instance)
run:
    cargo run

# Check the project for errors without producing a binary
check:
    cargo check

# Run clippy lints
lint:
    cargo clippy -- -D warnings

# Format the code
fmt:
    cargo fmt

# Check formatting without modifying files
fmt-check:
    cargo fmt -- --check

# ── Testing ────────────────────────────────────────────────────────────────────

# Run unit tests
test:
    cargo test

# Run end-to-end tests (starts and stops Docker Compose infrastructure)
test-e2e:
    ./scripts/run_e2e_tests.sh

# Run end-to-end tests and leave infrastructure running afterwards
test-e2e-no-teardown:
    ./scripts/run_e2e_tests.sh --no-teardown

# Generate HTML code coverage report (opens in target/llvm-cov/html/index.html)
coverage:
    cargo llvm-cov --html

# ── Infrastructure ─────────────────────────────────────────────────────────────

# Start all infrastructure services (Postgres, Kafka, Schema Registry, Debezium, AKHQ)
infra-up:
    docker compose up -d postgres kafka debezium schema-registry akhq

# Start the full stack including the order service
up:
    docker compose up --build -d

# Stop and remove all containers and volumes
down:
    docker compose down -v --remove-orphans

# Show logs for all services (or pass a service name, e.g. just logs postgres)
logs service="":
    docker compose logs -f {{ service }}

# ── Debezium ───────────────────────────────────────────────────────────────────

# Register the Debezium connector
register-connector:
    curl -X POST http://localhost:8083/connectors \
        -H "Content-Type: application/json" \
        -d @debezium/register-connector.json

# Check the status of the Debezium connector
connector-status:
    curl -s http://localhost:8083/connectors | jq .

# ── Utilities ──────────────────────────────────────────────────────────────────

# Copy .env.example to .env (skips if .env already exists)
env:
    @if [ -f .env ]; then echo ".env already exists, skipping"; else cp .env.example .env && echo ".env created"; fi

# Install git hooks (run once after cloning)
install-hooks:
    git config core.hooksPath .githooks
    @echo "Git hooks installed. Pre-commit hook will run fmt-check, lint, and unit tests."
