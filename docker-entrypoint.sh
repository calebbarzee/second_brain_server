#!/usr/bin/env bash
set -euo pipefail

# ── Wait for PostgreSQL (with timeout) ───────────────────
echo "Waiting for PostgreSQL..."
timeout=60
elapsed=0
until pg_isready -d "$DATABASE_URL" > /dev/null 2>&1; do
    sleep 1
    elapsed=$((elapsed + 1))
    if [ $elapsed -ge $timeout ]; then
        echo "ERROR: PostgreSQL not ready after ${timeout}s" >&2
        exit 1
    fi
done
echo "PostgreSQL is ready."

# ── Git safe directory ────────────────────────────────────
# Bind-mounted notes dir may have different ownership
git config --global --add safe.directory "${WATCH_PATHS}" 2>/dev/null || true
# Configure the repo owner identity (the human user).
# AI commits use --author override, so user.name is always the human.
git config --global user.name "${GIT_USER_NAME:-secondbrain}" 2>/dev/null || true
git config --global user.email "${GIT_USER_EMAIL:-user@second-brain.local}" 2>/dev/null || true
# Init git repo in notes dir if not already one
if [ -d "${WATCH_PATHS}" ] && ! git -C "${WATCH_PATHS}" rev-parse --git-dir >/dev/null 2>&1; then
    echo "Initializing git repo in ${WATCH_PATHS}..."
    git init "${WATCH_PATHS}"
    git -C "${WATCH_PATHS}" add -A
    git -C "${WATCH_PATHS}" commit -m "initial notes import" --allow-empty
fi
# Checkout the tracked branch (for the shared DB index)
tracked_branch="${TRACKED_BRANCH:-main}"
current_branch=$(git -C "${WATCH_PATHS}" rev-parse --abbrev-ref HEAD 2>/dev/null || echo "")
if [ -n "$current_branch" ] && [ "$current_branch" != "$tracked_branch" ]; then
    echo "Checking out tracked branch: ${tracked_branch}"
    git -C "${WATCH_PATHS}" checkout "$tracked_branch" 2>/dev/null || \
        git -C "${WATCH_PATHS}" checkout -b "$tracked_branch" 2>/dev/null || true
fi
# Create worktree directory
mkdir -p "${WORKTREE_DIR:-/data/worktrees}"
# Mark worktree dir as safe for git
git config --global --add safe.directory "*" 2>/dev/null || true

# ── Generate config from environment ─────────────────────
# Always regenerate so config stays in sync with env vars.
# The Rust binaries handle migrations via sqlx::migrate!().
cat > /app/second-brain.toml <<TOML
[database]
url = "${DATABASE_URL}"

[notes]
paths = ["${WATCH_PATHS}"]
tracked_branch = "${TRACKED_BRANCH:-main}"
worktree_dir = "${WORKTREE_DIR:-/data/worktrees}"

[embedding]
preset = "${EMBEDDING_PRESET:-nomic}"
batch_size = ${EMBEDDING_BATCH_SIZE:-16}
TOML

# Append provider overrides only if explicitly set
if [ -n "${EMBEDDING_PROVIDER:-}" ]; then
    echo "provider = \"${EMBEDDING_PROVIDER}\"" >> /app/second-brain.toml
fi
if [ -n "${EMBEDDING_URL:-}" ]; then
    echo "url = \"${EMBEDDING_URL}\"" >> /app/second-brain.toml
fi
if [ -n "${EMBEDDING_MODEL:-}" ]; then
    echo "model = \"${EMBEDDING_MODEL}\"" >> /app/second-brain.toml
fi
if [ -n "${EMBEDDING_DIMS:-}" ]; then
    echo "dimensions = ${EMBEDDING_DIMS}" >> /app/second-brain.toml
fi

echo "Config written to /app/second-brain.toml"

# ── Dispatch command ──────────────────────────────────────
case "${1:-server}" in
    server)
        transport="${MCP_TRANSPORT:-http}"
        host="${MCP_HOST:-0.0.0.0}"
        port="${MCP_PORT:-8080}"
        echo "Starting MCP server (${transport} transport)..."
        exec second-brain --transport "$transport" --host "$host" --port "$port"
        ;;
    cli)
        shift
        exec sb "$@"
        ;;
    ingest)
        echo "Ingesting notes from ${WATCH_PATHS}..."
        exec sb ingest "${WATCH_PATHS}"
        ;;
    embed)
        echo "Running embedding pipeline..."
        exec sb embed
        ;;
    shell)
        exec /bin/bash
        ;;
    *)
        exec "$@"
        ;;
esac
