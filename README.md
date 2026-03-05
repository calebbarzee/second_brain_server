# Second Brain

A personal knowledge OS built in Rust. Turns your markdown notes into a searchable, connected, AI-accessible knowledge base — backed by PostgreSQL + pgvector, exposed via MCP.

## What it does

- **Full-text + semantic search** over your markdown notes
- **Filesystem search** via ripgrep/fd — works even without the database
- **Auto-sync**: file watcher detects changes, re-indexes and re-embeds automatically
- **Link graph**: extracts `[[wikilinks]]` and `[markdown links]()`, traverses connections
- **Skill engine**: composable workflows — summarize your week, reflect on patterns, find cross-project connections
- **Note lifecycle**: classify notes as active/volatile/enduring/archived
- **Project awareness**: auto-detect projects from paths and filenames, fuzzy match against known projects
- **Symlink-based project sync**: mirror project docs into your KB via symlinks (no duplication)
- **15 MCP tools** for any MCP-compatible client (Claude Code, Claude Desktop, Neovim, etc.)

## Quick Start

```bash
# 1. Start PostgreSQL + embedding server
docker compose up -d

# 2. Build
cargo build --release -p sb-server

# 3. Register with Claude Code
claude mcp add second-brain \
  -e DATABASE_URL=postgresql://secondbrain:secondbrain@localhost:5432/secondbrain \
  -e EMBEDDING_URL=http://localhost:8090 \
  -e EMBEDDING_MODEL=BAAI/bge-base-en-v1.5 \
  -e EMBEDDING_DIMS=768 \
  -e WATCH_PATHS=$HOME/notes \
  -- $(pwd)/target/release/second-brain

# 4. Ingest your notes (from Claude Code or CLI)
sb-cli ingest ~/notes
```

### Using a remote embedding server

You don't need to run Docker locally — point at any TEI-compatible or OpenAI-compatible embedding endpoint:

```toml
# second-brain.toml
[embedding]
provider = "tei"
url = "https://my-embeddings-server.example.com"
model = "BAAI/bge-base-en-v1.5"
dimensions = 768
```

Or via environment variable: `EMBEDDING_URL=https://my-server.example.com` (overrides config file).

## CLI

```bash
# Build the CLI
cargo build --release -p sb-cli

# Search notes
sb-cli search "signal processing"

# Ingest a directory
sb-cli ingest ~/notes

# Run a skill
sb-cli skill summarize --period this-week

# List projects
sb-cli projects

# Classify a note
sb-cli classify ~/notes/TODO.md volatile
```

## MCP Tools (15)

| Tool | Description |
|------|-------------|
| `note_search` | Full-text search with lifecycle/project filters |
| `note_read` | Read a note's content and metadata |
| `note_list` | List notes with pagination and filters |
| `note_ingest` | Ingest files/directories with auto-embedding |
| `semantic_search` | Vector similarity search |
| `find_related` | Find semantically related notes |
| `embed_notes` | Batch-embed unembedded notes |
| `note_create` | Create a new note on disk + DB |
| `note_update` | Update an existing note |
| `note_graph` | Show link graph (outbound + inbound) |
| `file_search` | Search files on disk via ripgrep/fd (no DB needed) |
| `run_skill` | Run a skill: summarize, reflect, continue-work, connect-ideas, contextualize |
| `project_list` | List projects with note counts |
| `project_context` | Get comprehensive project context |
| `note_classify` | Set note lifecycle (active/volatile/enduring/archived) |

## Skills

| Skill | Permission | What it does |
|-------|-----------|-------------|
| `summarize` | ReadOnly | Activity summary: notes, tasks, grouped by project |
| `continue-work` | ReadOnly | Resume context for a specific project |
| `reflect` | ReadOnly | Compare planned vs completed, find patterns |
| `connect-ideas` | ReadOnly | Cross-project semantic connections |
| `contextualize` | Destructive | Auto-tag, auto-link, classify lifecycle (preview first) |

## Architecture

```
Markdown files (source of truth)
       │
       ▼  [file watcher / ingest]
PostgreSQL + pgvector (derived index)
  ├── notes, chunks, embeddings
  ├── tags, links, projects
  └── skill_runs, tasks
       │
       ▼  [MCP server - stdio]
Claude Code / Claude Desktop / CLI / Neovim
```

**Crates**: `sb-core` (models, DB, ingest, file search) · `sb-embed` (chunking, embeddings) · `sb-sync` (file watcher) · `sb-skills` (skill engine) · `sb-server` (MCP server) · `sb-cli` (command line)

## Prerequisites

- Rust 1.88+
- Docker & Docker Compose (for local PostgreSQL + embeddings, optional if using remote servers)
- ripgrep (`rg`) for filesystem search (recommended, falls back to grep)
- fd/fdfind for filename search (optional, falls back to find)

## Configuration

Copy `second-brain.toml.example` to `second-brain.toml` and customize. Or use environment variables — see GUIDE.md for details.

### Notes directory discovery

If no `notes.paths` are configured, the server automatically searches up to 2 subdirectories of `$HOME` for directories named `notes` (e.g., `$HOME/notes`, `$HOME/2_resources/notes`, `$HOME/projects/myproject/notes`).

## Docs

- [GUIDE.md](GUIDE.md) — detailed usage guide with examples
- [ROADMAP.md](ROADMAP.md) — development plan and architecture

## License
MIT
