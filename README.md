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
3. **The embedding pipeline** chunks notes by heading sections, sends chunks to Ollama, and stores vectors in pgvector with an HNSW index.
4. **17 MCP tools** expose everything to your AI assistant — search, read, create, update, link traversal, semantic similarity, project context, edit tracking, multi-user sessions, and composable skills.

The database is a derived index, not the source of truth. If it disappears, re-ingest and you're back.

## Quick start

The entire stack — the app, PostgreSQL, and Ollama — runs in Docker. Nothing besides Docker needs to be installed on the host.

### 1. Deploy

```bash
./deploy.sh              # Interactive setup
./deploy.sh --defaults   # Deploy with sensible defaults (no prompts)
```

This builds the app container, starts PostgreSQL + Ollama, pulls the embedding model, ingests your notes, and runs the initial embedding. The MCP server is exposed at `http://localhost:8080/mcp`.

### 2. Register with Claude Code

```bash
claude mcp add second-brain --type http --url http://localhost:8080/mcp -s user
```

That's it. Claude Code talks to the HTTP endpoint; the server handles database and embedding calls internally.

### 3. Use it

Start a Claude Code session and try:

- *"Search my notes for signal processing"*
- *"What notes are related to my current project?"*
- *"Create a note summarizing today's work"*
- *"Summarize what I worked on this week"*

### Manage the stack

```bash
./deploy.sh status       # Show running services
./deploy.sh logs [svc]   # Tail logs
./deploy.sh ingest       # Re-ingest notes
./deploy.sh embed        # Re-run embeddings
./deploy.sh down         # Stop all services (data volumes preserved)
./deploy.sh down -v      # Stop and delete all data
```

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

### Metadata and sessions

| Tool | Description |
|------|-------------|
| `note_classify` | Set note lifecycle (active/volatile/enduring/archived) |
| `note_stamp` | Stamp edit metadata in frontmatter (editor name + timestamp) |
| `session_init` | Initialize an isolated git worktree for multi-user editing |

### Skills and projects

| Tool | Description |
|------|-------------|
| `run_skill` | Run a composable workflow (see skills below) |
| `project_list` | List projects with note counts |
| `project_context` | Get comprehensive context for a project |

### When to use which search

- **`note_search`** — You know the words. "Find my notes about git CLI."
- **`semantic_search`** — You know the concept. "What have I written about radio frequency interference?" will match notes about signal processing, blind signal detection, and RF analysis even if they don't use that exact phrase.
- **`find_related`** — You have a note and want context. "What else connects to this architecture doc?"
- **`note_graph`** — You want structure. "What links to and from this note?"
- **`file_search`** — DB search missed? Files not ingested yet? Searches the raw filesystem.

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

## Configuration

### Embedding models

All embedding models run locally via Ollama inside the Docker stack. Choose a model during `./deploy.sh` setup or set `EMBEDDING_PRESET` in `.env.prod`:

| Preset | Model | Dimensions | Size | Notes |
|--------|-------|-----------|------|-------|
| `nomic` | nomic-embed-text | 768 | 137 MB | **Default.** Fast, good quality. |
| `all-minilm` | all-minilm | 384 | 46 MB | Fastest, smallest. |
| `snowflake` | snowflake-arctic-embed2 | 768 | 305 MB | Good balance. |
| `mxbai` | mxbai-embed-large | 1024 | 335 MB | High quality, larger vectors. |
| `qwen3` | qwen3-embedding | 1024 | 4.7 GB | Best quality. Slow on CPU. |

### Environment variables (.env.prod)

| Variable | Default | Description |
|----------|---------|-------------|
| `POSTGRES_PASSWORD` | `secondbrain` | Database password |
| `EMBEDDING_PRESET` | `nomic` | Ollama model preset (see table above) |
| `OLLAMA_MODEL` | `nomic-embed-text` | Ollama model to pull (matches preset) |
| `MCP_PORT` | `8080` | Host port for the MCP HTTP endpoint |
| `NOTES_PATH` | `./notes-data` | Host path to your notes directory (bind-mounted into container) |
| `RUST_LOG` | `info` | Log level (debug/info/warn/error) |
| `GIT_USER_NAME` | `secondbrain` | Git identity for note commits (human) |
| `AI_GIT_NAME` | `claude-ai` | Git identity for AI-authored commits |
| `AI_GIT_EMAIL` | `ai@second-brain.local` | Git email for AI-authored commits |
| `ANTHROPIC_API_KEY` | *(none)* | Enables autonomous skill execution (optional) |

PostgreSQL and Ollama run on the internal Docker network only — they are not exposed to the host. The only host-exposed port is the MCP server.

### Notes directory

Your notes are bind-mounted into the container at `/data/notes`. Point `NOTES_PATH` at any directory of markdown files. `deploy.sh` also supports cloning a git repository as the notes source.

If you use git-backed notes, the container auto-initializes the repo and tracks the configured branch. Push/pull via `./deploy.sh push` and `./deploy.sh pull`.

### Config file (second-brain.toml)

The Docker entrypoint auto-generates this from environment variables. For local development, you can write one manually:

```toml
[database]
url = "postgresql://secondbrain:secondbrain@localhost:5432/secondbrain"

[notes]
paths = ["~/notes"]

[embedding]
preset = "nomic"
# Individual fields override the preset:
# url = "http://localhost:11434"
# model = "nomic-embed-text"
# dimensions = 768
# batch_size = 32
# max_chunk_chars = 2400
```

## Multi-user sessions

When accessed over HTTP, the server supports concurrent editing through git worktrees. Each user session gets an isolated working copy.

### Branch naming

Default: `<username>/<YYYY-MM-DD>/working` (e.g., `alice/2026-03-18/working`)

Custom: pass a `branch` parameter to `session_init` (e.g., `alice/2026-03-18/notes_on_bees`)

### How it works

1. **`session_init`** creates a git worktree on a date-stamped branch
2. **`note_create`/`note_update`** write to the worktree and auto-commit with AI author identity
3. Notes are immediately searchable in the DB (ingested on write)
4. When the session ends, the worktree is cleaned up but the **branch persists**
5. Merging to `main` is a deliberate step (PR or manual merge)

Protected branches (`main`, `master`, `staging`, `dev`) are never written to directly. AI commits use `--author` so they're distinguishable from human commits in `git log`.

Read-only tools (search, list, read) don't require a session.

## Architecture

```
second-brain (Cargo workspace)
|
|-- sb-core       Shared foundation: models, DB queries, markdown parser,
|                 ingestion pipeline, file search. Everything depends on this.
|
|-- sb-embed      Embedding pipeline: pluggable providers, markdown chunker,
|                 batch processor.
|
|-- sb-sync       File watcher (notify v7) + sync processor. Detects file
|                 changes, ingests, embeds. Runs as a background tokio task.
|
|-- sb-skills     Skill engine: composable workflows for summarize, reflect,
|                 continue-work, connect-ideas, contextualize.
|
|-- sb-server     MCP server binary. 17 tools via rmcp. Supports stdio
|                 (local) and HTTP (network) transports.
|
|-- sb-cli        CLI for manual ingestion, search, embedding, and skills.
```

For deeper architecture details (data flow, embedding strategy, link resolution, database schema), see [docs/architecture.md](docs/architecture.md).

## Development

For local development without Docker (building from source):

```bash
# Start just the database
docker compose up -d db

# Start Ollama on the host (or use docker compose up -d ollama from the dev compose)
ollama serve &
ollama pull nomic-embed-text

# Build
cargo build --release

# Run the MCP server (stdio mode for Claude Code)
./target/release/second-brain

# Or HTTP mode
./target/release/second-brain --transport http --port 8080
```

Dev commands via justfile:

```bash
just db-up        # start postgres
just db-down      # stop services
just db-reset     # wipe and recreate the database
just test         # run all tests
just ci           # fmt + clippy + test
just server       # run MCP server in dev mode
```

The interactive `setup.sh` script can also walk through local setup, but Docker deployment via `deploy.sh` is the primary supported path.

## CLI

The `sb` CLI is available inside the container via `./deploy.sh cli`, or built locally with `cargo build --release -p sb-cli`:

```bash
# Via deploy.sh (runs inside container)
./deploy.sh cli search "signal processing"
./deploy.sh cli semantic "radio frequency"
./deploy.sh cli skill summarize --period this-week

# Or locally
sb search "signal processing"
sb semantic "radio frequency interference"
sb ingest ~/notes
sb embed
sb skill summarize --period this-week
sb projects
sb classify ~/notes/TODO.md volatile
```

## Testing

```bash
cargo test --workspace          # 95 unit + integration tests
cargo clippy --workspace        # lint checks
python3 test-mcp-http.py        # 51 HTTP integration tests (requires running server)
```

### Test coverage by crate

| Crate | Tests | Coverage |
|-------|-------|----------|
| sb-core | 62 | config, ingest, lifecycle, markdown, path_map, project_detect, project_sync, worktree |
| sb-embed | 10 | chunker, TEI provider |
| sb-skills | 16 | git_ops (branch validation, commit, snapshot), time_period |
| sb-sync | 2 | watcher (file change detection) |
| sb-core (integration) | 5 | DB connection, CRUD, full-text search, pgvector |

DB-dependent modules (db/\*) are covered by the integration test suite (`test-mcp-http.py`) and the `tests/db_integration.rs` tests.

## Further reading

- **[docs/architecture.md](docs/architecture.md)** — Database schema, data flow, embedding strategy, link resolution
- **[ROADMAP.md](ROADMAP.md)** — Future plans and reference projects

## License

MIT
