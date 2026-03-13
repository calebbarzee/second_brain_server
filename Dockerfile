# ── Stage 1: Build ────────────────────────────────────────
FROM rust:1.93-bookworm AS builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev && \
    rm -rf /var/lib/apt/lists/*

# Copy manifests first for layer caching
COPY Cargo.toml Cargo.lock ./
COPY crates/sb-core/Cargo.toml crates/sb-core/Cargo.toml
COPY crates/sb-server/Cargo.toml crates/sb-server/Cargo.toml
COPY crates/sb-cli/Cargo.toml crates/sb-cli/Cargo.toml
COPY crates/sb-embed/Cargo.toml crates/sb-embed/Cargo.toml
COPY crates/sb-sync/Cargo.toml crates/sb-sync/Cargo.toml
COPY crates/sb-skills/Cargo.toml crates/sb-skills/Cargo.toml

# Create stub lib.rs / main.rs so cargo can resolve the workspace
RUN mkdir -p crates/sb-core/src && echo "pub fn _stub(){}" > crates/sb-core/src/lib.rs && \
    mkdir -p crates/sb-embed/src && echo "pub fn _stub(){}" > crates/sb-embed/src/lib.rs && \
    mkdir -p crates/sb-sync/src && echo "pub fn _stub(){}" > crates/sb-sync/src/lib.rs && \
    mkdir -p crates/sb-skills/src && echo "pub fn _stub(){}" > crates/sb-skills/src/lib.rs && \
    mkdir -p crates/sb-server/src && echo "fn main(){}" > crates/sb-server/src/main.rs && \
    mkdir -p crates/sb-cli/src && echo "fn main(){}" > crates/sb-cli/src/main.rs

# Pre-build dependencies (cached unless Cargo.toml/lock changes)
# Stubs won't compile fully — we just want dependency downloads + partial compilation
ENV SQLX_OFFLINE=true
RUN cargo build --release --workspace || true

# Copy real source and migrations
COPY crates/ crates/
COPY migrations/ migrations/

# Touch source files so cargo knows they changed
RUN find crates -name "*.rs" -exec touch {} +

# Build release binaries
RUN cargo build --release --bin second-brain --bin sb

# ── Stage 2: Runtime ──────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 curl git ripgrep fd-find postgresql-client && \
    rm -rf /var/lib/apt/lists/* && \
    # fd-find installs as fdfind on Debian — symlink to fd
    ln -sf /usr/bin/fdfind /usr/bin/fd

WORKDIR /app

# Copy binaries from builder
COPY --from=builder /build/target/release/second-brain /usr/local/bin/second-brain
COPY --from=builder /build/target/release/sb /usr/local/bin/sb

# Copy migrations (used by sqlx::migrate! at runtime)
COPY migrations/ /app/migrations/

# Copy config templates
COPY second-brain.toml.example /app/second-brain.toml.example

# Default notes mount point
RUN mkdir -p /data/notes

# Default environment
ENV DATABASE_URL=postgresql://secondbrain:secondbrain@db:5432/secondbrain \
    WATCH_PATHS=/data/notes \
    RUST_LOG=info \
    MCP_TRANSPORT=http \
    MCP_HOST=0.0.0.0 \
    MCP_PORT=8080

EXPOSE 8080

# Entrypoint waits for DB, generates config, then dispatches
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

ENTRYPOINT ["docker-entrypoint.sh"]
CMD ["server"]
