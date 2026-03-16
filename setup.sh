#!/usr/bin/env bash
set -euo pipefail

# ── Second Brain — Interactive Setup Script ──────────────────────────
# Helps new users install prerequisites, configure, build, and register
# the Second Brain MCP server. Safe to re-run (idempotent).

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$SCRIPT_DIR"

# ── Color helpers ────────────────────────────────────────────────────

if [[ -t 1 ]] && command -v tput &>/dev/null && [[ "$(tput colors 2>/dev/null || echo 0)" -ge 8 ]]; then
    C_RESET="\033[0m"
    C_BOLD="\033[1m"
    C_GREEN="\033[32m"
    C_YELLOW="\033[33m"
    C_RED="\033[31m"
    C_CYAN="\033[36m"
    C_DIM="\033[2m"
else
    C_RESET="" C_BOLD="" C_GREEN="" C_YELLOW="" C_RED="" C_CYAN="" C_DIM=""
fi

info()    { printf "${C_CYAN}[info]${C_RESET}  %s\n" "$*"; }
ok()      { printf "${C_GREEN}[  ok]${C_RESET}  %s\n" "$*"; }
warn()    { printf "${C_YELLOW}[warn]${C_RESET}  %s\n" "$*"; }
err()     { printf "${C_RED}[ err]${C_RESET}  %s\n" "$*" >&2; }
step()    { printf "\n${C_BOLD}${C_CYAN}── %s${C_RESET}\n" "$*"; }
ask()     { printf "${C_BOLD}%s${C_RESET}" "$*"; }

# ── Defaults ─────────────────────────────────────────────────────────

OPT_NOTES_DIR=""
OPT_DB_PASSWORD=""
OPT_EMBEDDING_PROVIDER=""
OPT_EMBEDDING_MODEL=""
OPT_EMBEDDING_DIMS=""
OPT_MAX_CHUNK_CHARS=""
INTERACTIVE=true

# ── Usage ────────────────────────────────────────────────────────────

usage() {
    cat <<'USAGE'
Usage: setup.sh [OPTIONS]

Interactive setup script for the Second Brain knowledge OS.
Checks prerequisites, configures services, builds, and registers the MCP server.

Options:
  --notes-dir <path>              Notes directory (default: auto-detect or ~/notes)
  --db-password <pass>            PostgreSQL password (default: secondbrain)
  --embedding-provider <name>     Preset: nomic | all-minilm | snowflake | mxbai | qwen3 (default: nomic)
  --embedding-model <model>       Embedding model name
  --embedding-dims <dims>         Embedding dimensions
  --max-chunk-chars <chars>       Max characters per chunk
  --non-interactive               Skip all prompts, use defaults/flags
  --help                          Show this help

Examples:
  ./setup.sh                                         # Interactive mode
  ./setup.sh --notes-dir ~/notes --non-interactive   # Automated
  ./setup.sh --embedding-provider qwen3
USAGE
}

# ── Argument parsing ─────────────────────────────────────────────────

while [[ $# -gt 0 ]]; do
    case "$1" in
        --notes-dir)        OPT_NOTES_DIR="$2"; shift 2 ;;
        --db-password)      OPT_DB_PASSWORD="$2"; shift 2 ;;
        --embedding-provider) OPT_EMBEDDING_PROVIDER="$2"; shift 2 ;;
        --embedding-model)  OPT_EMBEDDING_MODEL="$2"; shift 2 ;;
        --embedding-dims)   OPT_EMBEDDING_DIMS="$2"; shift 2 ;;
        --max-chunk-chars)  OPT_MAX_CHUNK_CHARS="$2"; shift 2 ;;
        --non-interactive)  INTERACTIVE=false; shift ;;
        --help|-h)          usage; exit 0 ;;
        *)                  err "Unknown option: $1"; usage; exit 1 ;;
    esac
done

# ── OS / Package Manager detection ──────────────────────────────────

detect_os() {
    case "$(uname -s)" in
        Linux)  OS="linux" ;;
        Darwin) OS="macos" ;;
        *)      OS="unknown" ;;
    esac

    PKG_MGR=""
    if [[ "$OS" == "macos" ]]; then
        if command -v brew &>/dev/null; then
            PKG_MGR="brew"
        fi
    elif [[ "$OS" == "linux" ]]; then
        if command -v apt-get &>/dev/null; then
            PKG_MGR="apt"
        elif command -v dnf &>/dev/null; then
            PKG_MGR="dnf"
        elif command -v pacman &>/dev/null; then
            PKG_MGR="pacman"
        fi
    fi
}

# ── Prompt helper (respects --non-interactive) ───────────────────────

prompt() {
    local varname="$1" prompt_text="$2" default="$3"
    if [[ "$INTERACTIVE" == true ]]; then
        local input
        if [[ -n "$default" ]]; then
            ask "$prompt_text [$default]: "
        else
            ask "$prompt_text: "
        fi
        read -r input
        if [[ -z "$input" ]]; then
            eval "$varname=\"$default\""
        else
            eval "$varname=\"$input\""
        fi
    else
        eval "$varname=\"$default\""
    fi
}

confirm() {
    local prompt_text="$1" default="${2:-y}"
    if [[ "$INTERACTIVE" != true ]]; then
        return 0
    fi
    local yn
    if [[ "$default" == "y" ]]; then
        ask "$prompt_text [Y/n]: "
    else
        ask "$prompt_text [y/N]: "
    fi
    read -r yn
    yn="${yn:-$default}"
    [[ "$yn" =~ ^[Yy] ]]
}

# ── Prerequisite check / install functions ───────────────────────────

check_cmd() {
    command -v "$1" &>/dev/null
}

install_pkg() {
    local pkg="$1"
    case "$PKG_MGR" in
        brew)   brew install "$pkg" ;;
        apt)    sudo apt-get update -qq && sudo apt-get install -y "$pkg" ;;
        dnf)    sudo dnf install -y "$pkg" ;;
        pacman) sudo pacman -S --noconfirm "$pkg" ;;
        *)      return 1 ;;
    esac
}

check_docker() {
    if check_cmd docker; then
        ok "docker found: $(docker --version 2>/dev/null | head -1)"
        return 0
    fi
    warn "docker not found"
    if [[ -z "$PKG_MGR" ]]; then
        err "Cannot auto-install docker. Please install from https://docs.docker.com/get-docker/"
        return 1
    fi
    if confirm "Install Docker?"; then
        if [[ "$OS" == "macos" ]]; then
            brew install --cask docker
            info "Please launch Docker Desktop to finish setup"
        elif [[ "$PKG_MGR" == "apt" ]]; then
            # Use official Docker convenience script
            info "Installing Docker via official install script..."
            curl -fsSL https://get.docker.com | sudo sh
            sudo usermod -aG docker "$USER" 2>/dev/null || true
            info "You may need to log out/in for group membership to take effect"
        elif [[ "$PKG_MGR" == "dnf" ]]; then
            sudo dnf install -y docker docker-compose-plugin
            sudo systemctl enable --now docker
            sudo usermod -aG docker "$USER" 2>/dev/null || true
        elif [[ "$PKG_MGR" == "pacman" ]]; then
            sudo pacman -S --noconfirm docker docker-compose
            sudo systemctl enable --now docker
            sudo usermod -aG docker "$USER" 2>/dev/null || true
        fi
        ok "Docker installed"
    else
        err "Docker is required. Install from https://docs.docker.com/get-docker/"
        return 1
    fi
}

check_docker_compose() {
    if docker compose version &>/dev/null 2>&1; then
        ok "docker compose found: $(docker compose version 2>/dev/null | head -1)"
        return 0
    fi
    if docker-compose version &>/dev/null 2>&1; then
        ok "docker-compose (standalone) found"
        return 0
    fi
    warn "docker compose not found"
    if [[ "$PKG_MGR" == "brew" ]]; then
        info "Docker Compose is included with Docker Desktop on macOS"
    elif [[ "$PKG_MGR" == "apt" ]]; then
        if confirm "Install docker-compose-plugin?"; then
            sudo apt-get install -y docker-compose-plugin
            ok "docker-compose-plugin installed"
        fi
    elif [[ "$PKG_MGR" == "dnf" ]]; then
        if confirm "Install docker-compose-plugin?"; then
            sudo dnf install -y docker-compose-plugin
            ok "docker-compose-plugin installed"
        fi
    elif [[ "$PKG_MGR" == "pacman" ]]; then
        if confirm "Install docker-compose?"; then
            sudo pacman -S --noconfirm docker-compose
            ok "docker-compose installed"
        fi
    else
        err "Please install Docker Compose: https://docs.docker.com/compose/install/"
        return 1
    fi
}

check_rust() {
    if check_cmd rustc && check_cmd cargo; then
        local rust_version
        rust_version="$(rustc --version | awk '{print $2}')"
        ok "Rust found: $rust_version"

        # Check version >= 1.88
        local major minor
        major="$(echo "$rust_version" | cut -d. -f1)"
        minor="$(echo "$rust_version" | cut -d. -f2)"
        if [[ "$major" -lt 1 ]] || { [[ "$major" -eq 1 ]] && [[ "$minor" -lt 88 ]]; }; then
            warn "Rust $rust_version is too old (need >= 1.88 for rmcp/darling dep)"
            if confirm "Update Rust via rustup?"; then
                rustup update stable
                ok "Rust updated to $(rustc --version | awk '{print $2}')"
            else
                err "Rust >= 1.88 is required"
                return 1
            fi
        fi
        return 0
    fi
    warn "Rust/Cargo not found"
    if check_cmd rustup; then
        if confirm "Install Rust stable via rustup?"; then
            rustup install stable
            ok "Rust installed"
        fi
    else
        if confirm "Install Rust via rustup.rs?"; then
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
            # shellcheck source=/dev/null
            source "$HOME/.cargo/env" 2>/dev/null || true
            ok "Rust installed: $(rustc --version 2>/dev/null | awk '{print $2}')"
        else
            err "Rust is required. Install from https://rustup.rs"
            return 1
        fi
    fi
}

check_ripgrep() {
    if check_cmd rg; then
        ok "ripgrep found: $(rg --version | head -1)"
        return 0
    fi
    warn "ripgrep (rg) not found — used for file_search tool"
    if [[ -n "$PKG_MGR" ]] && confirm "Install ripgrep?"; then
        install_pkg ripgrep
        ok "ripgrep installed"
    else
        info "Optional: install ripgrep for file content search (https://github.com/BurntSushi/ripgrep)"
    fi
}

check_fd() {
    if check_cmd fd || check_cmd fdfind; then
        local fdbin
        fdbin="$(command -v fd || command -v fdfind)"
        ok "fd found: $fdbin"
        return 0
    fi
    warn "fd (fd-find) not found — used for file_search tool"
    local fd_pkg="fd-find"
    if [[ "$PKG_MGR" == "pacman" ]] || [[ "$PKG_MGR" == "brew" ]]; then
        fd_pkg="fd"
    fi
    if [[ -n "$PKG_MGR" ]] && confirm "Install fd?"; then
        install_pkg "$fd_pkg"
        ok "fd installed"
    else
        info "Optional: install fd for filename search (https://github.com/sharkdp/fd)"
    fi
}

check_just() {
    if check_cmd just; then
        ok "just found: $(just --version 2>/dev/null)"
        return 0
    fi
    warn "just not found — used for dev commands (justfile)"
    if [[ -n "$PKG_MGR" ]] && confirm "Install just?"; then
        if [[ "$PKG_MGR" == "brew" ]]; then
            brew install just
        elif [[ "$PKG_MGR" == "apt" ]]; then
            # just is not always in default repos; try cargo or snap
            if check_cmd cargo; then
                cargo install just
            else
                sudo snap install just --edge 2>/dev/null || { info "Install just manually: https://github.com/casey/just"; return 0; }
            fi
        elif [[ "$PKG_MGR" == "dnf" ]]; then
            sudo dnf install -y just 2>/dev/null || cargo install just
        elif [[ "$PKG_MGR" == "pacman" ]]; then
            sudo pacman -S --noconfirm just
        fi
        ok "just installed"
    else
        info "Optional: install just for dev commands (https://github.com/casey/just)"
    fi
}

check_curl() {
    if check_cmd curl; then
        ok "curl found"
        return 0
    fi
    warn "curl not found"
    if [[ -n "$PKG_MGR" ]] && confirm "Install curl?"; then
        install_pkg curl
        ok "curl installed"
    else
        err "curl is required for embedding health checks"
        return 1
    fi
}

check_sqlx_cli() {
    if check_cmd sqlx; then
        ok "sqlx-cli found"
        return 0
    fi
    warn "sqlx-cli not found — needed for database migrations"
    if confirm "Install sqlx-cli via cargo?"; then
        info "Installing sqlx-cli (this may take a minute)..."
        cargo install sqlx-cli --no-default-features --features postgres
        ok "sqlx-cli installed"
    else
        info "Migrations can also run via the built binary, but sqlx-cli is recommended"
    fi
}

check_ollama() {
    if check_cmd ollama; then
        ok "Ollama found: $(ollama --version 2>/dev/null | head -1)"
        return 0
    fi
    warn "Ollama not found — needed for local dev embeddings (Docker deployment includes Ollama)"
    if [[ -n "$PKG_MGR" ]] && confirm "Install Ollama? (optional if using Docker deployment)"; then
        if [[ "$PKG_MGR" == "brew" ]]; then
            brew install ollama
            info "Starting Ollama service..."
            brew services start ollama 2>/dev/null || true
        else
            info "Installing Ollama via official install script..."
            curl -fsSL https://ollama.com/install.sh | sh
        fi
        ok "Ollama installed"
    else
        info "Ollama is optional for local dev — Docker deployment bundles it"
        return 0
    fi
}

# ── Auto-detect notes directories ────────────────────────────────────

detect_notes_dirs() {
    local candidates=()
    # Check common locations
    for dir in \
        "$HOME/notes" \
        "$HOME/Notes" \
        "$HOME/Documents/notes" \
        "$HOME/Documents/Notes" \
        "$HOME/Obsidian" \
        "$HOME/obsidian"; do
        if [[ -d "$dir" ]]; then
            candidates+=("$dir")
        fi
    done

    # Also look 2 levels deep for dirs containing "notes" (case-insensitive)
    while IFS= read -r d; do
        # Avoid duplicates
        local already=false
        for c in "${candidates[@]+"${candidates[@]}"}"; do
            if [[ "$c" == "$d" ]]; then
                already=true
                break
            fi
        done
        if [[ "$already" == false ]]; then
            candidates+=("$d")
        fi
    done < <(find "$HOME" -maxdepth 2 -type d -iname "*notes*" 2>/dev/null | head -10)

    printf '%s\n' "${candidates[@]+"${candidates[@]}"}"
}

# ── Configuration prompts ────────────────────────────────────────────

configure() {
    step "Configuration"

    # Notes directory
    if [[ -n "$OPT_NOTES_DIR" ]]; then
        NOTES_DIR="$OPT_NOTES_DIR"
    else
        local detected
        detected="$(detect_notes_dirs)"
        local default_notes="$HOME/notes"
        if [[ -n "$detected" ]]; then
            local first
            first="$(echo "$detected" | head -1)"
            default_notes="$first"
            if [[ "$INTERACTIVE" == true ]]; then
                info "Detected notes directories:"
                echo "$detected" | while read -r d; do
                    printf "  ${C_DIM}%s${C_RESET}\n" "$d"
                done
            fi
        fi
        prompt NOTES_DIR "Notes directory" "$default_notes"
    fi
    # Expand ~ in notes dir
    NOTES_DIR="${NOTES_DIR/#\~/$HOME}"
    ok "Notes directory: $NOTES_DIR"

    # Database password
    if [[ -n "$OPT_DB_PASSWORD" ]]; then
        DB_PASSWORD="$OPT_DB_PASSWORD"
    else
        prompt DB_PASSWORD "Database password" "secondbrain"
    fi

    # Embedding preset (all Ollama-based — runs inside Docker or on host)
    if [[ "$INTERACTIVE" == true ]]; then
        info "Ollama embedding presets (sets model, dimensions, chunk size automatically):"
        printf "  ${C_BOLD}1)${C_RESET} nomic       — nomic-embed-text (137M, 768d, fast, recommended)\n"
        printf "  ${C_BOLD}2)${C_RESET} all-minilm  — all-minilm (33M, 384d, fastest)\n"
        printf "  ${C_BOLD}3)${C_RESET} snowflake   — snowflake-arctic-embed2 (305M, 768d, good balance)\n"
        printf "  ${C_BOLD}4)${C_RESET} mxbai       — mxbai-embed-large (335M, 1024d, high quality)\n"
        printf "  ${C_BOLD}5)${C_RESET} qwen3       — qwen3-embedding (4.7G, 1024d, best quality, slow on CPU)\n"
        local choice
        prompt choice "Choose preset (1-5)" "1"
        case "$choice" in
            1|nomic)      EMBED_PRESET="nomic" ;;
            2|all-minilm) EMBED_PRESET="all-minilm" ;;
            3|snowflake)  EMBED_PRESET="snowflake" ;;
            4|mxbai)      EMBED_PRESET="mxbai" ;;
            5|qwen3)      EMBED_PRESET="qwen3" ;;
            *)            EMBED_PRESET="nomic" ;;
        esac
    else
        EMBED_PRESET="${OPT_EMBEDDING_PROVIDER:-nomic}"
    fi
    ok "Embedding preset: $EMBED_PRESET"

    # Resolve preset defaults (all Ollama)
    EMBED_PROVIDER="ollama"
    EMBED_URL="http://localhost:11434"
    case "$EMBED_PRESET" in
        nomic)      EMBED_MODEL="nomic-embed-text"; EMBED_DIMS="768"; MAX_CHUNK="2400" ;;
        all-minilm) EMBED_MODEL="all-minilm"; EMBED_DIMS="384"; MAX_CHUNK="1000" ;;
        snowflake)  EMBED_MODEL="snowflake-arctic-embed2"; EMBED_DIMS="768"; MAX_CHUNK="2400" ;;
        mxbai)      EMBED_MODEL="mxbai-embed-large"; EMBED_DIMS="1024"; MAX_CHUNK="1200" ;;
        qwen3)      EMBED_MODEL="qwen3-embedding"; EMBED_DIMS="1024"; MAX_CHUNK="3000" ;;
        *)          EMBED_MODEL="nomic-embed-text"; EMBED_DIMS="768"; MAX_CHUNK="2400" ;;
    esac

    # Allow overriding individual fields
    if [[ -n "$OPT_EMBEDDING_MODEL" ]]; then EMBED_MODEL="$OPT_EMBEDDING_MODEL"; fi
    if [[ -n "$OPT_EMBEDDING_DIMS" ]]; then EMBED_DIMS="$OPT_EMBEDDING_DIMS"; fi
    if [[ -n "$OPT_MAX_CHUNK_CHARS" ]]; then MAX_CHUNK="$OPT_MAX_CHUNK_CHARS"; fi

    # Ollama model pull (for local dev — Docker deployment pulls inside the container)
    if check_cmd ollama; then
        if confirm "Pull Ollama model '$EMBED_MODEL' now? (optional — Docker deployment handles this)"; then
            info "Pulling $EMBED_MODEL (this may take a while)..."
            ollama pull "$EMBED_MODEL" || warn "Failed to pull model — you can do this later with: ollama pull $EMBED_MODEL"
        fi
    fi

    # Summary
    step "Configuration Summary"
    printf "  Notes directory:      %s\n" "$NOTES_DIR"
    printf "  Database password:    %s\n" "$DB_PASSWORD"
    printf "  Embedding preset:     %s\n" "$EMBED_PRESET"
    printf "  Embedding model:      %s\n" "$EMBED_MODEL"
    printf "  Embedding dimensions: %s\n" "$EMBED_DIMS"
    printf "  Max chunk chars:      %s\n" "$MAX_CHUNK"
    echo

    if [[ "$INTERACTIVE" == true ]]; then
        if ! confirm "Proceed with these settings?"; then
            info "Aborted. Re-run to change settings."
            exit 0
        fi
    fi
}

# ── Step 1: Generate .env ────────────────────────────────────────────

generate_env() {
    step "Step 1/9: Generate .env"

    local env_file="$PROJECT_DIR/.env"
    local db_url="postgresql://secondbrain:${DB_PASSWORD}@localhost:5432/secondbrain"

    if [[ -f "$env_file" ]]; then
        local existing_url
        existing_url="$(grep -oP '(?<=^DATABASE_URL=).*' "$env_file" 2>/dev/null || true)"
        if [[ "$existing_url" == "$db_url" ]]; then
            ok ".env already up to date"
            return 0
        fi
        warn ".env exists but DATABASE_URL differs, updating"
    fi

    cat > "$env_file" <<EOF
DATABASE_URL=${db_url}
EMBEDDING_PRESET=${EMBED_PRESET}
WATCH_PATHS=${NOTES_DIR}
EOF

    ok "Generated $env_file"
}

# ── Step 2: Generate second-brain.toml ───────────────────────────────

generate_toml() {
    step "Step 2/9: Generate second-brain.toml"

    local toml_file="$PROJECT_DIR/second-brain.toml"

    if [[ -f "$toml_file" ]]; then
        info "second-brain.toml already exists, backing up to second-brain.toml.bak"
        cp "$toml_file" "${toml_file}.bak"
    fi

    local db_url="postgresql://secondbrain:${DB_PASSWORD}@localhost:5432/secondbrain"

    cat > "$toml_file" <<EOF
[database]
url = "${db_url}"

[notes]
paths = [
    "${NOTES_DIR}",
]

[embedding]
# Available presets: nomic, all-minilm, snowflake, mxbai, qwen3
preset = "${EMBED_PRESET}"
batch_size = 16
EOF

    ok "Generated $toml_file"
}

# ── Step 3: Docker compose up ────────────────────────────────────────

start_services() {
    step "Step 3/9: Start Docker services"

    cd "$PROJECT_DIR"

    # Start just the database (Ollama runs on host or via Docker deployment)
    local services=("db")

    # Update docker-compose.yml password if non-default
    if [[ "$DB_PASSWORD" != "secondbrain" ]]; then
        info "Note: Update docker-compose.yml POSTGRES_PASSWORD to match your chosen password"
    fi

    # Check if services are already running
    local running
    running="$(docker compose ps --format '{{.Name}}' 2>/dev/null || true)"
    local all_running=true
    for svc in "${services[@]}"; do
        if ! echo "$running" | grep -q "secondbrain-${svc}"; then
            all_running=false
            break
        fi
    done

    if [[ "$all_running" == true ]]; then
        ok "Services already running"
        return 0
    fi

    info "Starting services: ${services[*]}"
    docker compose up -d "${services[@]}"
    ok "Docker services started"
}

# ── Step 4: Wait for healthy ─────────────────────────────────────────

wait_healthy() {
    step "Step 4/9: Wait for services to be healthy"

    info "Waiting for PostgreSQL..."
    local retries=30
    while ! docker compose exec -T db pg_isready -U secondbrain &>/dev/null; do
        retries=$((retries - 1))
        if [[ $retries -le 0 ]]; then
            err "PostgreSQL did not become ready in time"
            exit 1
        fi
        sleep 2
    done
    ok "PostgreSQL is ready"

    if check_cmd ollama; then
        info "Checking Ollama is serving..."
        retries=15
        while ! curl -sf http://localhost:11434/api/tags &>/dev/null; do
            retries=$((retries - 1))
            if [[ $retries -le 0 ]]; then
                warn "Ollama is not responding — start it with: ollama serve (or brew services start ollama)"
                return 0
            fi
            sleep 2
        done
        ok "Ollama is ready"

        # Ensure the model is pulled
        if ! ollama list 2>/dev/null | grep -q "$EMBED_MODEL"; then
            info "Pulling $EMBED_MODEL (this may take a while on first run)..."
            ollama pull "$EMBED_MODEL" || warn "Failed to pull model — run: ollama pull $EMBED_MODEL"
        else
            ok "Model $EMBED_MODEL is available"
        fi
    fi
}

# ── Step 5: Run migrations ───────────────────────────────────────────

run_migrations() {
    step "Step 5/9: Run database migrations"

    cd "$PROJECT_DIR"

    # Source .env for DATABASE_URL
    set -a
    # shellcheck source=/dev/null
    source "$PROJECT_DIR/.env"
    set +a

    # Check if migrations are already applied
    local tables
    tables="$(docker compose exec -T db psql -U secondbrain -d secondbrain -t -c \
        "SELECT count(*) FROM information_schema.tables WHERE table_schema='public';" 2>/dev/null || echo "0")"
    tables="$(echo "$tables" | tr -d ' ')"

    if [[ "$tables" -gt 5 ]]; then
        ok "Database schema already exists ($tables tables)"
        # Still run migrations in case there are new ones
        if check_cmd sqlx; then
            sqlx migrate run --source "$PROJECT_DIR/migrations" 2>/dev/null || true
        fi
        return 0
    fi

    if check_cmd sqlx; then
        info "Running migrations via sqlx-cli..."
        sqlx migrate run --source "$PROJECT_DIR/migrations"
    else
        info "sqlx-cli not found, running migrations via raw SQL..."
        for migration in "$PROJECT_DIR"/migrations/*.sql; do
            if [[ -f "$migration" ]]; then
                info "  Applying $(basename "$migration")..."
                docker compose exec -T db psql -U secondbrain -d secondbrain < "$migration"
            fi
        done
    fi

    ok "Migrations applied"
}

# ── Step 6: Build release binaries ───────────────────────────────────

build_release() {
    step "Step 6/9: Build release binaries"

    cd "$PROJECT_DIR"

    local server_bin="$PROJECT_DIR/target/release/second-brain"
    local cli_bin="$PROJECT_DIR/target/release/sb"

    # Check if binaries exist and are newer than source
    if [[ -f "$server_bin" ]] && [[ -f "$cli_bin" ]]; then
        local newest_src
        newest_src="$(find "$PROJECT_DIR/crates" -name '*.rs' -newer "$server_bin" 2>/dev/null | head -1)"
        if [[ -z "$newest_src" ]]; then
            ok "Release binaries are up to date"
            return 0
        fi
        info "Source files changed since last build, rebuilding..."
    fi

    info "Building release binaries (this may take a few minutes on first run)..."

    # Set SQLX_OFFLINE so we don't need a live DB at compile time
    SQLX_OFFLINE=true cargo build --release 2>&1 | tail -5

    if [[ -f "$server_bin" ]]; then
        ok "Built: $server_bin"
    else
        err "Build failed — second-brain binary not found"
        exit 1
    fi

    if [[ -f "$cli_bin" ]]; then
        ok "Built: $cli_bin"
    fi
}

# ── Step 7: Initial ingest ───────────────────────────────────────────

initial_ingest() {
    step "Step 7/9: Ingest notes directory"

    if [[ ! -d "$NOTES_DIR" ]]; then
        warn "Notes directory does not exist: $NOTES_DIR"
        if confirm "Create it?" "y"; then
            mkdir -p "$NOTES_DIR"
            ok "Created $NOTES_DIR"
            info "Add some markdown files and re-run this step, or let the file watcher handle it"
            return 0
        else
            info "Skipping ingest — directory does not exist"
            return 0
        fi
    fi

    local note_count
    note_count="$(find "$NOTES_DIR" -name '*.md' -type f 2>/dev/null | wc -l)"
    if [[ "$note_count" -eq 0 ]]; then
        info "No markdown files found in $NOTES_DIR — skipping ingest"
        return 0
    fi

    info "Found $note_count markdown files in $NOTES_DIR"

    cd "$PROJECT_DIR"
    set -a
    # shellcheck source=/dev/null
    source "$PROJECT_DIR/.env"
    set +a

    local cli_bin="$PROJECT_DIR/target/release/sb"
    if [[ -f "$cli_bin" ]]; then
        "$cli_bin" ingest "$NOTES_DIR" --no-embed || {
            warn "Ingest returned an error — check your notes directory and database connection"
            return 0
        }
        ok "Notes ingested"
    else
        warn "CLI binary not found, skipping ingest"
    fi
}

# ── Step 8: Run embedding ────────────────────────────────────────────

run_embedding() {
    step "Step 8/9: Embed notes"

    # Check if Ollama is reachable
    if ! curl -sf http://localhost:11434/api/tags &>/dev/null; then
        warn "Ollama not reachable — skipping embedding"
        info "Start Ollama and run: ./target/release/sb embed"
        info "Or use Docker deployment (./deploy.sh) which bundles Ollama"
        return 0
    fi

    cd "$PROJECT_DIR"
    set -a
    # shellcheck source=/dev/null
    source "$PROJECT_DIR/.env"
    set +a

    local cli_bin="$PROJECT_DIR/target/release/sb"
    if [[ -f "$cli_bin" ]]; then
        "$cli_bin" embed || {
            warn "Embedding returned an error — the embedding server may still be loading"
            info "Run later with: ./target/release/sb embed"
            return 0
        }
        ok "Notes embedded"
    else
        warn "CLI binary not found, skipping embedding"
    fi
}

# ── Step 9: Register MCP server ──────────────────────────────────────

register_mcp() {
    step "Step 9/9: Register MCP server with Claude Code"

    local server_bin="$PROJECT_DIR/target/release/second-brain"

    if ! check_cmd claude; then
        warn "Claude Code CLI not found — skipping MCP registration"
        info "Install Claude Code, then register manually."
        info ""
        info "For Docker deployment (recommended):"
        printf "\n  claude mcp add second-brain --type http --url http://localhost:8080/mcp -s user\n\n"
        info "For local stdio mode (dev):"
        printf "\n  claude mcp add second-brain -s user \\\\\n"
        printf "    -e DATABASE_URL=postgresql://secondbrain:${DB_PASSWORD}@localhost:5432/secondbrain \\\\\n"
        printf "    -e EMBEDDING_PRESET=${EMBED_PRESET} \\\\\n"
        printf "    -e WATCH_PATHS=${NOTES_DIR} \\\\\n"
        printf "    -- %s\n\n" "$server_bin"
        return 0
    fi

    # Check if already registered
    local existing
    existing="$(claude mcp list 2>/dev/null || true)"
    if echo "$existing" | grep -q "second-brain"; then
        if confirm "second-brain is already registered. Re-register with new settings?" "n"; then
            claude mcp remove second-brain -s user 2>/dev/null || true
        else
            ok "MCP server already registered"
            return 0
        fi
    fi

    # Determine registration mode
    local use_http=false
    if [[ "$INTERACTIVE" == true ]]; then
        info "MCP transport mode:"
        printf "  ${C_BOLD}1)${C_RESET} stdio — Claude Code spawns the server directly (local dev)\n"
        printf "  ${C_BOLD}2)${C_RESET} http  — Connect to a running server (Docker deployment)\n"
        local transport_choice
        prompt transport_choice "Choose transport" "1"
        if [[ "$transport_choice" == "2" || "$transport_choice" == "http" ]]; then
            use_http=true
        fi
    fi

    info "Registering MCP server..."

    if [[ "$use_http" == true ]]; then
        local mcp_url="http://localhost:8080/mcp"
        prompt mcp_url "MCP server URL" "$mcp_url"
        claude mcp add second-brain --type http --url "$mcp_url" -s user
    else
        local db_url="postgresql://secondbrain:${DB_PASSWORD}@localhost:5432/secondbrain"
        local env_args=(
            -e "DATABASE_URL=${db_url}"
            -e "EMBEDDING_PRESET=${EMBED_PRESET}"
            -e "WATCH_PATHS=${NOTES_DIR}"
        )
        claude mcp add second-brain -s user "${env_args[@]}" -- "$server_bin"
    fi

    ok "MCP server registered with Claude Code"
}

# ── Print summary ────────────────────────────────────────────────────

print_summary() {
    local db_url="postgresql://secondbrain:${DB_PASSWORD}@localhost:5432/secondbrain"
    local server_bin="$PROJECT_DIR/target/release/second-brain"
    local cli_bin="$PROJECT_DIR/target/release/sb"

    printf "\n"
    printf "${C_BOLD}${C_GREEN}════════════════════════════════════════════════════${C_RESET}\n"
    printf "${C_BOLD}${C_GREEN}  Second Brain — Setup Complete${C_RESET}\n"
    printf "${C_BOLD}${C_GREEN}════════════════════════════════════════════════════${C_RESET}\n"
    printf "\n"
    printf "  ${C_BOLD}Notes directory:${C_RESET}   %s\n" "$NOTES_DIR"
    printf "  ${C_BOLD}Database URL:${C_RESET}      %s\n" "$db_url"
    printf "  ${C_BOLD}Embedding:${C_RESET}         %s (%s, %s dims)\n" "$EMBED_PROVIDER" "$EMBED_MODEL" "$EMBED_DIMS"
    printf "  ${C_BOLD}MCP server:${C_RESET}        %s\n" "$server_bin"
    printf "  ${C_BOLD}CLI:${C_RESET}               %s\n" "$cli_bin"
    printf "\n"
    printf "  ${C_BOLD}Quick commands:${C_RESET}\n"
    printf "    ${C_DIM}sb search \"query\"${C_RESET}          Full-text search\n"
    printf "    ${C_DIM}sb semantic \"concept\"${C_RESET}      Semantic search\n"
    printf "    ${C_DIM}sb ingest /path/to/dir${C_RESET}     Ingest notes\n"
    printf "    ${C_DIM}sb embed${C_RESET}                    Embed unembedded notes\n"
    printf "    ${C_DIM}sb stats${C_RESET}                    Show knowledge base stats\n"
    printf "    ${C_DIM}sb skill summarize${C_RESET}          Summarize recent notes\n"
    printf "\n"
    printf "  ${C_BOLD}Next steps:${C_RESET}\n"
    printf "    1. Start a Claude Code session — your notes are available via MCP\n"
    printf "    2. Try: \"Search my notes for ...\"\n"
    printf "    3. Set up cron jobs: ${C_DIM}./setup-cron.sh${C_RESET}\n"
    printf "\n"
}

# ── Main ─────────────────────────────────────────────────────────────

main() {
    printf "\n${C_BOLD}${C_CYAN}Second Brain — Setup${C_RESET}\n"
    printf "${C_DIM}Personal knowledge OS: Rust + PostgreSQL/pgvector + MCP${C_RESET}\n\n"

    detect_os
    info "Detected OS: $OS (package manager: ${PKG_MGR:-none})"

    # Prerequisites
    step "Prerequisites"

    check_docker || exit 1
    check_docker_compose || exit 1
    check_rust || exit 1
    check_curl || exit 1
    check_ollama
    check_ripgrep
    check_fd
    check_just
    check_sqlx_cli

    # Configuration
    configure

    # Setup steps
    generate_env
    generate_toml
    start_services
    wait_healthy
    run_migrations
    build_release
    initial_ingest
    run_embedding
    register_mcp

    # Done
    print_summary
}

main
