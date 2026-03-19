#!/usr/bin/env bash
set -euo pipefail

# ── Second Brain — Cron Job Setup ────────────────────────────────────
# Installs scheduled jobs for automatic note ingestion, nightly review,
# and note categorization. Safe to re-run (replaces existing sb entries).

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

# ── Defaults ─────────────────────────────────────────────────────────

INGEST_INTERVAL=30
REVIEW_TIME="22:00"
PUSH_TIME="23:00"
NOTES_DIR=""
ACTION="install"
DRY_RUN=false
CRON_TAG="# second-brain"
LOG_FILE="$HOME/.sb-cron.log"

# ── Usage ────────────────────────────────────────────────────────────

usage() {
    cat <<'USAGE'
Usage: setup-cron.sh [OPTIONS]

Set up scheduled cron jobs for the Second Brain knowledge OS.

Actions (default: install):
  --remove              Remove all second-brain cron jobs
  --list                Show current second-brain cron jobs
  --dry-run             Show what would be added without modifying crontab

Options:
  --ingest-interval <minutes>   Ingest frequency (default: 30)
  --review-time <HH:MM>        Nightly review time (default: 22:00)
  --push-time <HH:MM>          Nightly git push time (default: 23:00)
  --notes-dir <path>            Notes directory (auto-detected from config)
  --help                        Show this help

Examples:
  ./setup-cron.sh                                   # Install with defaults
  ./setup-cron.sh --ingest-interval 15              # Ingest every 15 minutes
  ./setup-cron.sh --review-time 21:30               # Nightly review at 9:30pm
  ./setup-cron.sh --push-time 23:30                 # Nightly push at 11:30pm
  ./setup-cron.sh --list                            # Show installed jobs
  ./setup-cron.sh --remove                          # Remove all sb cron jobs
  ./setup-cron.sh --dry-run                         # Preview without installing
USAGE
}

# ── Argument parsing ─────────────────────────────────────────────────

while [[ $# -gt 0 ]]; do
    case "$1" in
        --ingest-interval) INGEST_INTERVAL="$2"; shift 2 ;;
        --review-time)     REVIEW_TIME="$2"; shift 2 ;;
        --push-time)       PUSH_TIME="$2"; shift 2 ;;
        --notes-dir)       NOTES_DIR="$2"; shift 2 ;;
        --remove)          ACTION="remove"; shift ;;
        --list)            ACTION="list"; shift ;;
        --dry-run)         DRY_RUN=true; shift ;;
        --help|-h)         usage; exit 0 ;;
        *)                 err "Unknown option: $1"; usage; exit 1 ;;
    esac
done

# ── Detect notes directory from config ───────────────────────────────

detect_notes_dir() {
    # 1. Already provided via flag
    if [[ -n "$NOTES_DIR" ]]; then
        NOTES_DIR="${NOTES_DIR/#\~/$HOME}"
        return 0
    fi

    # 2. From second-brain.toml
    local toml_file="$PROJECT_DIR/second-brain.toml"
    if [[ -f "$toml_file" ]]; then
        local parsed
        # Extract first path from paths = ["..."]
        parsed="$(grep -oP 'paths\s*=\s*\[\s*"?\K[^",\]]+' "$toml_file" 2>/dev/null | head -1 || true)"
        if [[ -n "$parsed" ]]; then
            NOTES_DIR="${parsed/#\~/$HOME}"
            NOTES_DIR="${NOTES_DIR%\"}"
            info "Detected notes directory from second-brain.toml: $NOTES_DIR"
            return 0
        fi
    fi

    # 3. From .env WATCH_PATHS
    local env_file="$PROJECT_DIR/.env"
    if [[ -f "$env_file" ]]; then
        local watch
        watch="$(grep -oP '(?<=^WATCH_PATHS=).*' "$env_file" 2>/dev/null || true)"
        if [[ -n "$watch" ]]; then
            # Take first comma-separated path
            NOTES_DIR="${watch%%,*}"
            NOTES_DIR="${NOTES_DIR/#\~/$HOME}"
            info "Detected notes directory from .env WATCH_PATHS: $NOTES_DIR"
            return 0
        fi
    fi

    # 4. Default
    NOTES_DIR="$HOME/notes"
    warn "Could not detect notes directory, using default: $NOTES_DIR"
}

# ── Validate prerequisites ───────────────────────────────────────────

validate() {
    local cli_bin="$PROJECT_DIR/target/release/sb"

    if [[ ! -f "$cli_bin" ]]; then
        err "CLI binary not found: $cli_bin"
        err "Run ./setup.sh first to build the project"
        exit 1
    fi

    if ! command -v crontab &>/dev/null; then
        err "crontab command not found — cron is required"
        exit 1
    fi

    ok "CLI binary found: $cli_bin"
}

# ── Parse review time ────────────────────────────────────────────────

parse_review_time() {
    if ! [[ "$REVIEW_TIME" =~ ^[0-9]{1,2}:[0-9]{2}$ ]]; then
        err "Invalid review time format: $REVIEW_TIME (expected HH:MM)"
        exit 1
    fi

    REVIEW_HOUR="${REVIEW_TIME%%:*}"
    REVIEW_MINUTE="${REVIEW_TIME##*:}"

    # Remove leading zeros for cron (but keep valid)
    REVIEW_HOUR="$((10#$REVIEW_HOUR))"
    REVIEW_MINUTE="$((10#$REVIEW_MINUTE))"

    # Calculate categorize time (5 minutes after review)
    CATEGORIZE_MINUTE=$((REVIEW_MINUTE + 5))
    CATEGORIZE_HOUR=$REVIEW_HOUR
    if [[ $CATEGORIZE_MINUTE -ge 60 ]]; then
        CATEGORIZE_MINUTE=$((CATEGORIZE_MINUTE - 60))
        CATEGORIZE_HOUR=$(((CATEGORIZE_HOUR + 1) % 24))
    fi

    # Parse push time
    if ! [[ "$PUSH_TIME" =~ ^[0-9]{1,2}:[0-9]{2}$ ]]; then
        err "Invalid push time format: $PUSH_TIME (expected HH:MM)"
        exit 1
    fi
    PUSH_HOUR="${PUSH_TIME%%:*}"
    PUSH_MINUTE="${PUSH_TIME##*:}"
    PUSH_HOUR="$((10#$PUSH_HOUR))"
    PUSH_MINUTE="$((10#$PUSH_MINUTE))"
}

# ── Build cron entries ───────────────────────────────────────────────

build_cron_entries() {
    local cli_bin="$PROJECT_DIR/target/release/sb"
    # Helper to ensure Ollama is serving (no-op if not using Ollama or already running)
    local ensure_ollama="(grep -q ollama ${PROJECT_DIR}/.env 2>/dev/null && { pgrep -f 'ollama serve' >/dev/null 2>&1 || ollama serve >/dev/null 2>&1 & sleep 3; } || true)"

    CRON_ENTRIES=""

    # Ingest job
    CRON_ENTRIES+="$CRON_TAG — periodic ingest + embed\n"
    CRON_ENTRIES+="*/${INGEST_INTERVAL} * * * * cd ${PROJECT_DIR} && set -a && . ./.env && set +a && ${ensure_ollama} && ${cli_bin} ingest ${NOTES_DIR} 2>>${LOG_FILE}\n"

    # Nightly review — summarize
    CRON_ENTRIES+="\n$CRON_TAG — nightly summarize\n"
    CRON_ENTRIES+="${REVIEW_MINUTE} ${REVIEW_HOUR} * * * cd ${PROJECT_DIR} && set -a && . ./.env && set +a && ${ensure_ollama} && ${cli_bin} skill summarize --period today 2>>${LOG_FILE}\n"

    # Nightly review — connect-ideas
    CRON_ENTRIES+="$CRON_TAG — nightly connect-ideas\n"
    CRON_ENTRIES+="${REVIEW_MINUTE} ${REVIEW_HOUR} * * * cd ${PROJECT_DIR} && set -a && . ./.env && set +a && ${cli_bin} skill connect-ideas --period today 2>>${LOG_FILE}\n"

    # Nightly categorize (5 min after review)
    CRON_ENTRIES+="\n$CRON_TAG — nightly categorize\n"
    CRON_ENTRIES+="${CATEGORIZE_MINUTE} ${CATEGORIZE_HOUR} * * * cd ${PROJECT_DIR} && set -a && . ./.env && set +a && ${cli_bin} skill contextualize --period today --allow-writes 2>>${LOG_FILE}\n"

    # Nightly push all branches
    CRON_ENTRIES+="\n$CRON_TAG — nightly push all branches\n"
    CRON_ENTRIES+="${PUSH_MINUTE} ${PUSH_HOUR} * * * cd ${NOTES_DIR} && git push --all 2>>${LOG_FILE}\n"
}

# ── List existing cron jobs ──────────────────────────────────────────

list_cron() {
    step "Current Second Brain cron jobs"

    local existing
    existing="$(crontab -l 2>/dev/null || true)"

    if [[ -z "$existing" ]]; then
        info "No crontab entries found"
        return 0
    fi

    local sb_entries
    sb_entries="$(echo "$existing" | grep -A1 "second-brain" || true)"

    if [[ -z "$sb_entries" ]]; then
        info "No second-brain cron jobs installed"
    else
        printf "\n%s\n\n" "$sb_entries"
    fi
}

# ── Remove existing cron jobs ────────────────────────────────────────

remove_cron() {
    step "Removing Second Brain cron jobs"

    local existing
    existing="$(crontab -l 2>/dev/null || true)"

    if [[ -z "$existing" ]]; then
        info "No crontab entries found"
        return 0
    fi

    # Filter out second-brain entries:
    # Remove lines containing the tag, and the command line immediately after a tag line
    local filtered
    filtered="$(echo "$existing" | awk '
        /# second-brain/ { skip=1; next }
        skip { skip=0; next }
        { print }
    ')"

    # Also remove any blank lines that were separating sections (clean up doubles)
    filtered="$(echo "$filtered" | cat -s)"

    if [[ "$filtered" == "$existing" ]]; then
        info "No second-brain cron jobs found to remove"
        return 0
    fi

    if [[ "$DRY_RUN" == true ]]; then
        info "[dry-run] Would remove second-brain entries from crontab"
        info "[dry-run] Resulting crontab:"
        printf "%s\n" "$filtered"
        return 0
    fi

    echo "$filtered" | crontab -
    ok "Second Brain cron jobs removed"
}

# ── Install cron jobs ────────────────────────────────────────────────

install_cron() {
    step "Installing Second Brain cron jobs"

    parse_review_time
    build_cron_entries

    # Show what will be installed
    info "Jobs to install:"
    printf "\n"
    printf "  ${C_BOLD}Ingest + embed:${C_RESET}  every %s minutes\n" "$INGEST_INTERVAL"
    printf "  ${C_BOLD}Summarize:${C_RESET}       %02d:%02d daily\n" "$REVIEW_HOUR" "$REVIEW_MINUTE"
    printf "  ${C_BOLD}Connect-ideas:${C_RESET}   %02d:%02d daily\n" "$REVIEW_HOUR" "$REVIEW_MINUTE"
    printf "  ${C_BOLD}Categorize:${C_RESET}      %02d:%02d daily\n" "$CATEGORIZE_HOUR" "$CATEGORIZE_MINUTE"
    printf "  ${C_BOLD}Push branches:${C_RESET}   %02d:%02d daily\n" "$PUSH_HOUR" "$PUSH_MINUTE"
    printf "  ${C_BOLD}Log file:${C_RESET}        %s\n" "$LOG_FILE"
    printf "\n"

    info "Cron entries:"
    printf "${C_DIM}"
    printf '%b' "$CRON_ENTRIES"
    printf "${C_RESET}\n"

    if [[ "$DRY_RUN" == true ]]; then
        info "[dry-run] No changes made"
        return 0
    fi

    # Read existing crontab, strip old second-brain entries
    local existing
    existing="$(crontab -l 2>/dev/null || true)"

    local cleaned
    cleaned="$(echo "$existing" | awk '
        /# second-brain/ { skip=1; next }
        skip { skip=0; next }
        { print }
    ')"
    # Remove trailing blank lines and collapse multiple blanks
    cleaned="$(echo "$cleaned" | cat -s | sed -e '/^$/N;/^\n$/d')"

    # Append new entries
    local new_crontab
    if [[ -n "$cleaned" ]]; then
        new_crontab="${cleaned}\n\n"
    else
        new_crontab=""
    fi
    new_crontab+="$(printf '%b' "$CRON_ENTRIES")"

    printf '%b\n' "$new_crontab" | crontab -
    ok "Cron jobs installed"

    # Log rotation hint
    printf "\n"
    info "Log rotation hint:"
    printf "  Logs go to ${C_DIM}%s${C_RESET}\n" "$LOG_FILE"
    printf "  To set up automatic rotation, create ${C_DIM}/etc/logrotate.d/second-brain${C_RESET}:\n"
    printf "\n"
    printf "    ${C_DIM}%s {${C_RESET}\n" "$LOG_FILE"
    printf "    ${C_DIM}    weekly${C_RESET}\n"
    printf "    ${C_DIM}    rotate 4${C_RESET}\n"
    printf "    ${C_DIM}    compress${C_RESET}\n"
    printf "    ${C_DIM}    missingok${C_RESET}\n"
    printf "    ${C_DIM}    notifempty${C_RESET}\n"
    printf "    ${C_DIM}}${C_RESET}\n"
    printf "\n"
    printf "  Or add a simple size check to your shell profile:\n"
    printf "    ${C_DIM}[[ -f %s ]] && [[ \$(stat -c%%s %s 2>/dev/null || stat -f%%z %s 2>/dev/null) -gt 10485760 ]] && : > %s${C_RESET}\n" \
        "$LOG_FILE" "$LOG_FILE" "$LOG_FILE" "$LOG_FILE"
    printf "\n"
}

# ── Main ─────────────────────────────────────────────────────────────

main() {
    printf "\n${C_BOLD}${C_CYAN}Second Brain — Cron Setup${C_RESET}\n\n"

    case "$ACTION" in
        list)
            list_cron
            ;;
        remove)
            remove_cron
            ;;
        install)
            detect_notes_dir
            validate
            install_cron
            ;;
    esac
}

main
