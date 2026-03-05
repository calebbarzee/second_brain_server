# Second Brain — development commands

set dotenv-load

# Default: show available commands
default:
    @just --list

# ── Database ────────────────────────────────────────────────

# Start PostgreSQL (Docker)
db-up:
    docker compose up -d db
    @echo "Waiting for PostgreSQL to be ready..."
    @until docker compose exec db pg_isready -U secondbrain > /dev/null 2>&1; do sleep 1; done
    @echo "PostgreSQL is ready."

# Stop PostgreSQL
db-down:
    docker compose down

# Connect to PostgreSQL via psql
db-shell:
    docker compose exec db psql -U secondbrain secondbrain

# Reset database (destroy and recreate)
db-reset:
    docker compose down -v
    just db-up
    just migrate

# ── Migrations ──────────────────────────────────────────────

# Run database migrations
migrate:
    cargo sqlx migrate run

# Create a new migration file
migration name:
    cargo sqlx migrate add {{name}}

# ── Build & Test ────────────────────────────────────────────

# Check all crates compile
check:
    cargo check --workspace

# Run all tests
test:
    cargo test --workspace

# Run clippy lints
lint:
    cargo clippy --workspace -- -D warnings

# Format code
fmt:
    cargo fmt --all

# Check formatting without modifying
fmt-check:
    cargo fmt --all -- --check

# Full CI check: fmt + lint + test
ci: fmt-check lint test

# ── Run ─────────────────────────────────────────────────────

# Run the MCP server
server:
    cargo run --bin second-brain

# Run the CLI with arguments
cli *args:
    cargo run --bin sb -- {{args}}

# Ingest notes from a path
ingest path:
    cargo run --bin sb -- ingest {{path}}

# Search notes
search query:
    cargo run --bin sb -- search "{{query}}"

# ── MCP ──────────────────────────────────────────────────────

# Build release binary and show path
build:
    cargo build --release --bin second-brain
    @echo "Binary: target/release/second-brain"
