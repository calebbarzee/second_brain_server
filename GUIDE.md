# Second Brain — User Guide

A personal knowledge OS that turns your markdown notes into a searchable,
connected, AI-accessible knowledge base. Built in Rust, backed by PostgreSQL,
and exposed via MCP so any compatible AI assistant can read, search, and
write your notes.

---

## Quick Start

### Prerequisites

- **Rust 1.88+** (1.93+ recommended; run `rustup update stable`)
- **Docker & Docker Compose** (for PostgreSQL and the embedding server)

### 1. Start the infrastructure

```bash
docker compose up -d
```

This launches:
- **PostgreSQL 16 + pgvector** on port 5432
- **TEI embedding server** (BAAI/bge-base-en-v1.5) on port 8090

The embedding model downloads on first start (~500 MB). Give it a minute.

### 2. Build the server

```bash
cargo build --release -p sb-server
```

The binary lands at `target/release/second-brain`.

### 3. Register with Claude Code

```bash
claude mcp add second-brain \
  -e DATABASE_URL=postgresql://secondbrain:secondbrain@localhost:5432/secondbrain \
  -e EMBEDDING_URL=http://localhost:8090 \
  -e EMBEDDING_MODEL=BAAI/bge-base-en-v1.5 \
  -e EMBEDDING_DIMS=768 \
  -e WATCH_PATHS=$HOME/notes \
  -- $(pwd)/target/release/second-brain
```

### 4. Use it

Start a Claude Code session. Your notes are now available as MCP tools. Try:

- *"Search my notes for signal processing"*
- *"What notes are related to my current project?"*
- *"Create a note summarizing today's work"*

### Dev commands (justfile)

```bash
just db-up        # start postgres + embeddings
just db-down      # stop services
just db-reset     # wipe and recreate the database
just test         # run all tests
just ci           # fmt + clippy + test
just server       # run MCP server in dev mode
```

---

## How It Works

### The core loop

```
You write markdown  →  Watcher detects change  →  Parser extracts structure
                                                        ↓
You ask a question  ←  MCP returns results  ←  DB stores notes + vectors
```

1. **You write** plain markdown files. No special syntax required. Frontmatter
   is parsed if present but never mandatory.

2. **The file watcher** detects changes within 300ms, reads the file, parses
   it, and upserts it into PostgreSQL. Links (`[text](path.md)` and
   `[[wikilinks]]`) are extracted and stored in a graph.

3. **The embedding pipeline** chunks each note by heading sections, sends
   chunks to the embedding provider, and stores vectors in pgvector with
   an HNSW index. Vector dimensions are configurable per preset.

4. **MCP tools** expose everything to your AI assistant: full-text search,
   semantic search, link graph traversal, note creation, and updates.

### What's in the database

The database is a **derived index**, not the source of truth. Your files
on disk are canonical. If the database disappears, re-ingest and you're back.

| Table | Purpose |
|-------|---------|
| `notes` | Content, metadata, full-text search vector |
| `chunks` | Note sections with heading context |
| `embeddings` | 768-dim vectors (HNSW indexed) |
| `links` | Directed graph between notes |
| `tags`, `note_tags` | Tagging system |
| `projects`, `note_projects` | Project associations and detection |
| `sync_state` | Tracks what's been synced and when |
| `skill_runs` | Skill execution history and tracking |
| `tasks` | Tasks extracted from note checkboxes |
| `settings` | Runtime configuration (tracked branch, etc.) |

---

## The 17 MCP Tools

### Search & Read

| Tool | What it does |
|------|-------------|
| `note_search` | Full-text search (PostgreSQL tsvector). Fast keyword matching. |
| `semantic_search` | Vector similarity search. Finds conceptually related content even without exact keyword matches. |
| `find_related` | Given a note, find other notes that are semantically similar. |
| `note_read` | Read a note's full content, frontmatter, and metadata. |
| `note_list` | Browse all notes, most recently updated first. |
| `note_graph` | Show a note's outbound links and backlinks. |
| `file_search` | Search files directly on disk via ripgrep (content) or fd (filenames). Works without DB — useful for non-ingested files. |

### Write & Ingest

| Tool | What it does |
|------|-------------|
| `note_create` | Write a new markdown file and auto-ingest it. |
| `note_update` | Overwrite a note's content and re-index. |
| `note_ingest` | Manually ingest a file or directory into the database. |
| `embed_notes` | Trigger embedding for any notes missing vectors. |

### Metadata & Sessions

| Tool | What it does |
|------|-------------|
| `note_classify` | Set a note's lifecycle: active, volatile, enduring, or archived. |
| `note_stamp` | Stamp edit metadata in frontmatter — records editor name and timestamp. |
| `session_init` | Initialize an isolated git worktree for multi-user editing sessions. |

### Skills & Projects

| Tool | What it does |
|------|-------------|
| `run_skill` | Run a composable workflow (summarize, reflect, continue-work, connect-ideas, contextualize). |
| `project_list` | List all detected projects with note counts. |
| `project_context` | Get comprehensive context for a project (recent notes, open tasks, lifecycle breakdown). |

### When to use which search

- **`note_search`** — You know the words. "Find my notes about the useful and frequently used git CLI."
- **`semantic_search`** — You know the concept. "What have I written about radio frequency interference?" will match notes about signal processing, blind signal detection, and RF analysis even if they don't use that exact phrase.
- **`find_related`** — You have a note and want context. "What else connects to this architecture doc?"
- **`note_graph`** — You want structure. "What links to and from this note?"
- **`file_search`** — DB search missed? Files not ingested yet? This searches the raw filesystem via ripgrep (content) or fd (filenames). No database or embeddings required.

---

## Guidelines for Use

### Note organization

The system is deliberately unopinionated about how you organize files. It
works with whatever structure you already have:

```
~/notes/
  architecture.md               # project docs
  test_03-09-2025.md            # test reports
  TODO_02_03_2026.md            # task lists
  signal_processing.md          # reference material
  daily/                        # daily logs
```

No folder hierarchy is required. No naming convention is enforced. The
semantic search and link graph handle discoverability — you don't need to
get the filing right upfront.

**What does help:**

- **Use headings.** The chunker splits on `## ` boundaries. Well-structured
  notes produce better, more targeted search results.
- **Link between notes.** Use `[[wikilinks]]` or `[text](./path.md)` to
  create connections. The link graph makes these traversable.
- **Write naturally.** The embedding model understands meaning. A note
  titled "turn interpolation" will surface when you ask about "smoothing
  aircraft trajectories" because the vectors capture semantic proximity.

### What the system is good at today

- **Instant recall.** "What were the test results from the September SIL
  test?" — semantic search finds it even if you don't remember exact dates.
- **Cross-project context.** Notes from different projects live in the same index. Ask about a concept and get
  results that span domains.
- **AI-assisted writing.** Use `note_create` to have Claude draft notes
  directly into your knowledge base — meeting summaries, architecture
  decisions, research findings.
- **Automatic sync.** Edit in Neovim, save, and the index updates. No
  manual import step.

---

## Architecture

### Crate structure

```
second-brain (workspace)
├── sb-core     Shared foundation: models, DB queries, markdown parser,
│               ingestion pipeline. Everything depends on this.
│
├── sb-embed    Embedding pipeline: pluggable providers (TEI, OpenAI,
│               Ollama), markdown chunker, batch processor.
│
├── sb-sync     File watcher (notify v7) + sync processor. Detects file
│               changes, ingests, embeds. Runs as a background tokio task.
│
├── sb-server   MCP server binary. 17 tools via rmcp. Supports stdio
│               (local) and HTTP (network) transports.
│
├── sb-skills   Skill engine: composable workflows for summarize,
│               reflect, continue-work, connect-ideas, contextualize.
│
└── sb-cli      CLI for ingestion, search, embedding, skills, and
                project management.
```

### Data flow

```
 Markdown files on disk
        │
        ├─── [file watcher]  auto-detects changes (300ms debounce)
        │         │
        │         ▼
        ├─── [ingest]  parse markdown, extract title/links/frontmatter
        │         │     compute content hash, skip if unchanged
        │         │
        │         ▼
        ├─── [DB upsert]  notes table + links table + sync_state
        │         │
        │         ▼
        └─── [embed]  chunk by headings → batch to TEI → store vectors
                                                │
                                                ▼
                                    pgvector HNSW index
                                                │
                                    ┌───────────┴───────────┐
                                    │                       │
                              semantic_search          find_related
                              (query → vector → top-k)  (note → avg vector → top-k)
```

### Embedding strategy

Notes are chunked by heading structure (`## Section`) with a configurable
max chunk size (default ~2400 characters for nomic). Each chunk preserves
its heading context, so search results can point you to the right section,
not just the right file.

The default model is nomic-embed-text (768 dimensions), running locally
via Ollama. No API keys needed. Nine embedding presets are available —
switch between Ollama, TEI, and OpenAI models by changing the preset in
config. The pipeline is provider-agnostic via the `EmbeddingProvider` trait.

### Link resolution

When you write `[[some-note]]` or `[text](./path.md)`, the ingestion
pipeline:

1. Extracts the link target
2. Resolves it against known notes (exact path match, then filename suffix)
3. Stores the link with a foreign key to the target note (if found)
4. When a new note is ingested, previously-unresolved links that match its
   filename are retroactively connected

This means the link graph builds itself incrementally — you don't need to
ingest everything at once for links to work.

---


## Configuration Reference

### Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | `postgresql://secondbrain:secondbrain@localhost:5432/secondbrain` | PostgreSQL connection string |
| `EMBEDDING_URL` | from config or `http://localhost:8090` | Embedding server endpoint (overrides config) |
| `EMBEDDING_MODEL` | *(from preset)* | Model identifier (overrides config) |
| `EMBEDDING_DIMS` | from config or `768` | Vector dimensions (overrides config) |
| `EMBEDDING_PRESET` | `nomic` | Preset name (nomic, tei, qwen3, openai-small, etc.) |
| `WATCH_PATHS` | *(none)* | Comma-separated directories to watch |
| `ANTHROPIC_API_KEY` | *(none)* | Enables autonomous skill execution (optional) |

### Config file (second-brain.toml)

```toml
[database]
url = "postgresql://secondbrain:secondbrain@localhost:5432/secondbrain"

[notes]
paths = ["~/notes"]

[embedding]
preset = "nomic"              # 9 presets: nomic, tei, qwen3, openai-small, etc.
# Individual fields override the preset:
# provider = "openai"         # "tei" or "openai" (Ollama uses openai protocol)
# url = "http://localhost:11434"
# model = "nomic-embed-text"
# dimensions = 768
# batch_size = 32
# max_chunk_chars = 2400
```

**Remote embedding servers** — set `embedding.url` to any TEI-compatible or OpenAI-compatible endpoint. Docker is optional if you have a remote server.

### Notes directory auto-discovery

If `notes.paths` is empty and `WATCH_PATHS` is not set, the server searches `$HOME` up to 2 directories deep for any directory named `notes`. This means `$HOME/notes`, `$HOME/2_resources/notes`, etc. are found automatically.

### CLI flags

```
second-brain [OPTIONS]

Options:
  -c, --config <PATH>    Path to config file
      --watch <PATHS>    Directories to watch (comma-separated, overrides config)
  -h, --help             Print help
```

---

## Project Status

| Phase | Status | What it delivers |
|-------|--------|-----------------|
| 0 — Foundation | Done | Workspace, schema, Docker, migrations |
| 1 — Core MCP Server | Done | 4 tools: search, read, list, ingest |
| 2 — Embedding Pipeline | Done | Semantic search, related notes, embeddings |
| 3 — Sync Engine | Done | File watcher, auto-sync, links, create/update |
| 4 — Skills & Intelligence | Done | 5 skills, lifecycle, projects, file_search |
| 5 — Deployment & Multi-user | Done | Docker, HTTP transport, git worktrees, edit stamping, 17 tools |
| Next — Neovim Integration | Planned | Editor-native search, sync, linking |
| Next — Extensibility | Planned | Plugin system, importers, webhooks |
