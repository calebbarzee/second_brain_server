# Architecture

Detailed internals for contributors and the curious. For setup and usage, see the [README](../README.md).

## The core loop

```
You write markdown  -->  Watcher detects change  -->  Parser extracts structure
                                                            |
You ask a question  <--  MCP returns results  <--  DB stores notes + vectors
```

1. **You write** plain markdown files. No special syntax required. Frontmatter is parsed if present but never mandatory.
2. **The file watcher** detects changes within 300ms, reads the file, parses it, and upserts it into PostgreSQL. Links (`[text](path.md)` and `[[wikilinks]]`) are extracted and stored in a graph.
3. **The embedding pipeline** chunks each note by heading sections, sends chunks to Ollama, and stores vectors in pgvector with an HNSW index.
4. **MCP tools** expose everything to your AI assistant: full-text search, semantic search, link graph traversal, note creation, and updates.

## Database schema

The database is a **derived index**, not the source of truth. Your files on disk are canonical. If the database disappears, re-ingest and you're back.

| Table | Purpose |
|-------|---------|
| `notes` | Content, metadata, full-text search vector |
| `chunks` | Note sections with heading context |
| `embeddings` | Vectors (HNSW indexed, dimensions set at runtime) |
| `links` | Directed graph between notes |
| `tags`, `note_tags` | Tagging system |
| `projects`, `note_projects` | Project associations and detection |
| `sync_state` | Tracks what's been synced and when |
| `skill_runs` | Skill execution history and tracking |
| `tasks` | Tasks extracted from note checkboxes |
| `settings` | Runtime configuration (tracked branch, etc.) |

## Data flow

```
 Markdown files on disk
        |
        +--- [file watcher]  auto-detects changes (300ms debounce)
        |         |
        |         v
        +--- [ingest]  parse markdown, extract title/links/frontmatter
        |         |     compute content hash, skip if unchanged
        |         |
        |         v
        +--- [DB upsert]  notes table + links table + sync_state
        |         |
        |         v
        +--- [embed]  chunk by headings -> batch to Ollama -> store vectors
                                                |
                                                v
                                    pgvector HNSW index
                                                |
                                    +-----------+-----------+
                                    |                       |
                              semantic_search          find_related
                              (query -> vector -> top-k) (note -> avg vector -> top-k)
```

## Embedding strategy

Notes are chunked by heading structure (`## Section`) with a configurable max chunk size (default ~2400 characters for nomic). Each chunk preserves its heading context, so search results point you to the right section, not just the right file.

All embedding models run locally via Ollama. The default is nomic-embed-text (768 dimensions). The pipeline is provider-agnostic via the `EmbeddingProvider` trait — the codebase also supports TEI and OpenAI providers, but Docker deployment standardizes on Ollama.

Available presets: `nomic`, `all-minilm`, `snowflake`, `mxbai`, `qwen3`. Set via `EMBEDDING_PRESET` env var or `embedding.preset` in the config file.

## Link resolution

When you write `[[some-note]]` or `[text](./path.md)`, the ingestion pipeline:

1. Extracts the link target
2. Resolves it against known notes (exact path match, then filename suffix)
3. Stores the link with a foreign key to the target note (if found)
4. When a new note is ingested, previously-unresolved links that match its filename are retroactively connected

The link graph builds itself incrementally — you don't need to ingest everything at once.

## Crate structure

```
second-brain (workspace)
+-- sb-core     Shared foundation: models, DB queries, markdown parser,
|               ingestion pipeline, file search. Everything depends on this.
|
+-- sb-embed    Embedding pipeline: pluggable providers (Ollama, TEI, OpenAI),
|               markdown chunker, batch processor.
|
+-- sb-sync     File watcher (notify v7) + sync processor. Detects file
|               changes, ingests, embeds. Runs as a background tokio task.
|
+-- sb-server   MCP server binary. 17 tools via rmcp. Supports stdio
|               (local) and HTTP (network) transports.
|
+-- sb-skills   Skill engine: composable workflows for summarize,
|               reflect, continue-work, connect-ideas, contextualize.
|
+-- sb-cli      CLI for ingestion, search, embedding, skills, and
                project management.
```

## Transport modes

| Mode | Use case | How it works |
|------|----------|-------------|
| `stdio` | Local MCP client (Claude Code direct, Neovim) | Binary is spawned as child process, JSON-RPC over stdin/stdout |
| `http` | Docker deployment, network access, multi-device | Axum server at `/mcp`, stateful sessions via `Mcp-Session-Id` header |

Docker deployment uses HTTP transport. The entrypoint auto-configures the server.

## Multi-user editing

HTTP transport supports multiple concurrent users through git worktrees. Each session gets an isolated branch via the `session_init` tool. AI commits use `--author` override so human vs AI changes are distinguishable in `git log`. Protected branches (main/master/staging/dev) are refused for direct AI writes.

## Note lifecycle

Notes can be classified into lifecycle stages: `active` (default), `volatile`, `enduring`, or `archived`. Search tools accept lifecycle filters so you can scope results (e.g., exclude archived notes). The `contextualize` skill can auto-classify based on note content and age.

## Project detection

Projects are auto-detected from file paths (e.g., `projects/<name>/`), project directory file parsing, fuzzy substring matching against known projects, and configured mappings. The `project_context` tool aggregates recent notes, open tasks, and lifecycle breakdown for a given project.

## Docker deployment internals

The production stack (`docker-compose.prod.yml`) runs three services on an internal Docker network:

- **db** — PostgreSQL 16 + pgvector. No host port exposure.
- **ollama** — Ollama embedding server. No host port exposure.
- **app** — The Second Brain MCP server. Exposed on the configured MCP port (default 8080).

The app container's entrypoint (`docker-entrypoint.sh`):
1. Waits for PostgreSQL readiness
2. Configures git safe directories and initializes the notes repo
3. Generates `second-brain.toml` from environment variables
4. Starts the MCP server in HTTP mode

The Dockerfile uses a multi-stage build: Rust 1.93 builder with dependency caching, then a slim Debian runtime with git, ripgrep, and fd.
