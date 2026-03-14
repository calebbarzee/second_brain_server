# Second Brain — Roadmap

A personal knowledge OS: a Rust-based MCP server backed by PostgreSQL + pgvector,
syncing plain markdown notes with semantic search, and surfacing context to any AI
assistant you choose to work with.

---

## Guiding Principles

- **Rust core** — type-safe, fast, and practically functional. The server and sync
  engine are Rust. Ancillary tooling (scripts, Neovim plugin) uses the right tool
  for the job (shell, Lua).
- **Plain markdown is the source of truth** — notes are files on disk. The database
  is a derived, enrichable index. You never *need* the database to read your notes.
- **Pluggable embeddings** — local models (Ollama), OpenAI, or any provider behind
  a trait boundary. Swap without touching application code.
- **MCP-native** — every capability is exposed as MCP tools/resources so any
  MCP-compatible client (Claude Code, Claude Desktop, Zed, Neovim, etc.) can use it.
---

## Future Work
TBD

## Reference Projects & Resources

These informed the architecture and are worth studying:

| Project | Relevance |
|---------|-----------|
| [Official Rust MCP SDK (rmcp)](https://github.com/modelcontextprotocol/rust-sdk) | **The** SDK we'll build on. v0.16+, tokio-based, supports tools/resources/prompts. |
| [MCP Spec 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25) | Latest spec. Adds tasks, elicitation, OAuth 2.1. |
| [pgvector](https://github.com/pgvector/pgvector) | Vector similarity search for Postgres. HNSW indexes, cosine/L2/inner product. |
| [pgvectorscale](https://github.com/timescale/pgvectorscale) | Rust-based DiskANN index for pgvector at scale. |
| [pgai Vectorizer](https://github.com/timescale/pgai) | Auto-sync embeddings when source data changes. Pattern reference. |
| [gnosis-mcp](https://github.com/nicholasglazer/gnosis-mcp) | Closest existing project: MD → PG + pgvector MCP server (Python). |
| [basic-memory](https://github.com/basicmachines-co/basic-memory) | MD-based semantic knowledge graph over MCP. Obsidian-compatible. |
| [little-kb](https://github.com/Coder-Upsilon/little-kb) | Vector KB with auto MCP server generation. UI reference. |
| [ATLAS MCP Server](https://github.com/cyanheads/atlas-mcp-server) | Three-tier architecture (Projects, Tasks, Knowledge). |
| [Pi (oh-my-pi)](https://github.com/can1357/oh-my-pi) | Model-agnostic, extension-first agent design. Extensibility reference. |
| [SimpleScraper MCP Guide](https://simplescraper.io/blog/how-to-mcp) | Transport patterns, auth flow, session management. |
| [Shuttle Rust MCP Tutorial](https://www.shuttle.dev/blog/2025/07/18/how-to-build-a-stdio-mcp-server-in-rust) | Step-by-step Rust MCP server guide. |

---

*This is a living document. Update it as the project evolves.*
