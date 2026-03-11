# Second Brain

A personal knowledge OS built in Rust. It indexes your plain markdown notes into a searchable, semantically connected knowledge base — backed by PostgreSQL + pgvector and exposed via [MCP](https://modelcontextprotocol.io/) so any AI assistant can read, search, and write your notes.

## Why

You already take notes. They live in scattered directories, across projects, in various stages of completeness. The problem isn't writing — it's *finding* what you wrote six months ago, seeing connections between ideas across domains, and giving your AI tools the context they need to actually help.

Second Brain solves this by treating your markdown files as the source of truth and building a derived index that supports full-text search, semantic (vector) search, link-graph traversal, and composable AI workflows — all accessible from Claude Code, Claude Desktop, Neovim, or any MCP-compatible client.

Your files stay on disk, in whatever structure you already use. No lock-in, no special syntax, no migration required.

## How it works

```
You write markdown  -->  Watcher detects change  -->  Parser extracts structure
                                                            |
You ask a question  <--  MCP returns results  <--  DB stores notes + vectors
```

1. **You write** plain markdown files. No required frontmatter or naming convention.
2. **The file watcher** detects changes within 300ms, parses the file, and upserts it into PostgreSQL. Links (`[[wikilinks]]` and `[markdown](links)`) are extracted into a traversable graph.
3. **The embedding pipeline** chunks notes by heading sections, sends chunks to your embedding provider, and stores vectors in pgvector with an HNSW index.
4. **15 MCP tools** expose everything to your AI assistant — search, read, create, update, link traversal, semantic similarity, project context, and composable skills.

The database is a derived index, not the source of truth. If it disappears, re-ingest and you're back.

## Features

- **Full-text + semantic search** — keyword matching via PostgreSQL tsvector, conceptual search via vector similarity. A note about "turn interpolation" surfaces when you ask about "smoothing aircraft trajectories."
- **Auto-sync** — file watcher detects changes and re-indexes/re-embeds automatically. Edit in your editor, save, done.
- **Link graph** — extracts `[[wikilinks]]` and `[markdown links]()`, resolves targets, traverses connections in both directions.
- **Skill engine** — composable workflows: summarize your week, reflect on patterns, find cross-project connections, auto-tag and classify notes.
- **Note lifecycle** — classify notes as active, volatile, enduring, or archived. Filter search results by lifecycle stage.
- **Project awareness** — auto-detect projects from file paths, fuzzy match against known projects, get comprehensive project context.
- **Filesystem search** — search files directly via ripgrep/fd, even without the database or for non-ingested files.
- **Pluggable embeddings** — local models via Docker (TEI) or Ollama, cloud via OpenAI. Swap providers without touching application code.

## Quick start

The easiest way to get running is the interactive setup script:

```bash
./setup.sh
```

This walks you through prerequisites, configuration, building, initial ingestion, and MCP registration. It's idempotent — safe to re-run.

### Manual setup

If you prefer to do it step by step:

```bash
# 1. Start PostgreSQL + embedding server
docker compose up -d

# 2. Build
cargo build --release

# 3. Register with Claude Code
claude mcp add second-brain \
  -e DATABASE_URL=postgresql://secondbrain:secondbrain@localhost:5432/secondbrain \
  -e EMBEDDING_URL=http://localhost:8090 \
  -e EMBEDDING_MODEL=BAAI/bge-base-en-v1.5 \
  -e EMBEDDING_DIMS=768 \
  -e WATCH_PATHS=$HOME/notes \
  -- $(pwd)/target/release/second-brain

# 4. Ingest your notes
./target/release/sb ingest ~/notes
```

Then start a Claude Code session and try: *"Search my notes for signal processing"*

## MCP tools

### Search and read

| Tool | Description |
|------|-------------|
| `note_search` | Full-text search with lifecycle/project filters |
| `semantic_search` | Vector similarity search — finds conceptually related content |
| `find_related` | Given a note, find semantically similar notes |
| `note_read` | Read a note's content and metadata |
| `note_list` | List notes with pagination and filters |
| `note_graph` | Show a note's outbound links and backlinks |
| `file_search` | Search files on disk via ripgrep/fd (no DB needed) |

### Write and ingest

| Tool | Description |
|------|-------------|
| `note_create` | Create a new markdown file and auto-ingest it |
| `note_update` | Update an existing note and re-index |
| `note_ingest` | Ingest files/directories with auto-embedding |
| `embed_notes` | Batch-embed notes missing vectors |

### Skills and projects

| Tool | Description |
|------|-------------|
| `run_skill` | Run a composable workflow (see skills below) |
| `project_list` | List projects with note counts |
| `project_context` | Get comprehensive context for a project |
| `note_classify` | Set note lifecycle (active/volatile/enduring/archived) |

## Skills

Skills are composable workflows that operate over your knowledge base:

| Skill | Permission | What it does |
|-------|-----------|-------------|
| `summarize` | Read-only | Activity summary — notes created/modified, tasks, grouped by project |
| `continue-work` | Read-only | Resume context for a specific project — recent changes, open tasks, related notes |
| `reflect` | Read-only | Compare planned vs completed work, surface patterns and gaps |
| `connect-ideas` | Read-only | Find semantic connections across projects and domains |
| `contextualize` | Destructive | Auto-tag, auto-link, and classify note lifecycle (previews changes first) |

Skills work interactively through your MCP client (no API key needed) or autonomously with an `ANTHROPIC_API_KEY`.

## Architecture

```
second-brain (Cargo workspace)
|
|-- sb-core       Shared foundation: models, DB queries, markdown parser,
|                 ingestion pipeline, file search. Everything depends on this.
|
|-- sb-embed      Embedding pipeline: pluggable providers (TEI, OpenAI/Ollama),
|                 markdown chunker, batch processor.
|
|-- sb-sync       File watcher (notify v7) + sync processor. Detects file
|                 changes, ingests, embeds. Runs as a background tokio task.
|
|-- sb-skills     Skill engine: composable workflows for summarize, reflect,
|                 continue-work, connect-ideas, contextualize.
|
|-- sb-server     MCP server binary. 15 tools via rmcp. Starts the watcher,
|                 serves on stdio.
|
|-- sb-cli        CLI for manual ingestion, search, embedding, and skills.
```

### Embedding providers

The embedding pipeline is provider-agnostic. Configure via `second-brain.toml` or environment variables:

| Provider | Model | Dimensions | Notes |
|----------|-------|-----------|-------|
| `tei` (default) | BAAI/bge-base-en-v1.5 | 768 | Runs locally via Docker, no API key |
| `ollama` | qwen3-embedding | 1024 | Local via Ollama, matryoshka dimensions |
| `openai` | text-embedding-3-small | 1536 | Cloud, requires API key |

You can also point at any TEI-compatible or OpenAI-compatible remote endpoint — Docker is optional if you have a remote embedding server.

## Configuration

Configuration is read from `second-brain.toml` (generated by `setup.sh`) or environment variables. Environment variables take precedence.

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | `postgresql://secondbrain:secondbrain@localhost:5432/secondbrain` | PostgreSQL connection string |
| `EMBEDDING_URL` | `http://localhost:8090` | Embedding server endpoint |
| `EMBEDDING_MODEL` | `BAAI/bge-base-en-v1.5` | Model identifier |
| `EMBEDDING_DIMS` | `768` | Vector dimensions |
| `EMBEDDING_PROVIDER` | `tei` | Provider: `tei`, `ollama`, or `openai` |
| `WATCH_PATHS` | *(auto-detect)* | Comma-separated directories to watch |
| `ANTHROPIC_API_KEY` | *(none)* | Enables autonomous skill execution |

If no watch paths are configured, the server searches `$HOME` up to 2 directories deep for any directory named `notes`.

## Prerequisites

- **Rust 1.88+** — `rustup update stable`
- **Docker & Docker Compose** — for local PostgreSQL + embeddings (optional with remote servers)
- **ripgrep** (`rg`) — for `file_search` content search (recommended; falls back to grep)
- **fd** — for `file_search` filename search (optional; falls back to find)

## CLI

The CLI binary is called `sb`. You can either install it to your PATH or run it directly from the build output:

```bash
# Option A: Install to PATH
cargo install --path crates/sb-cli

# Option B: Run from build output
cargo build --release -p sb-cli
./target/release/sb <command>
```

```bash
# Search notes
sb search "signal processing"

# Semantic search
sb semantic "radio frequency interference"

# Ingest a directory
sb ingest ~/notes

# Embed unembedded notes
sb embed

# Run a skill
sb skill summarize --period this-week

# List projects
sb projects

# Classify a note
sb classify ~/notes/TODO.md volatile
```

## Further reading

- **[GUIDE.md](GUIDE.md)** — detailed usage guide with architecture diagrams, search strategy advice, and configuration reference
- **[ROADMAP.md](ROADMAP.md)** — future plans: Neovim integration, hosting/multi-device, extensibility

## License

MIT
