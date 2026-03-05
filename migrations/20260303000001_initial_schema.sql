-- Enable required extensions
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "vector";

-- ── Notes ──────────────────────────────────────────────────────

CREATE TABLE notes (
    id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    file_path   TEXT NOT NULL UNIQUE,
    title       TEXT NOT NULL DEFAULT 'Untitled',
    content_hash TEXT NOT NULL,
    raw_content TEXT NOT NULL DEFAULT '',
    frontmatter JSONB,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    synced_at   TIMESTAMPTZ,
    deleted     BOOLEAN NOT NULL DEFAULT FALSE,
    -- Full-text search vector, auto-populated by trigger
    search_vector TSVECTOR
);

-- Full-text search index
CREATE INDEX idx_notes_search ON notes USING GIN (search_vector);
CREATE INDEX idx_notes_file_path ON notes (file_path);
CREATE INDEX idx_notes_updated_at ON notes (updated_at DESC);
CREATE INDEX idx_notes_deleted ON notes (deleted) WHERE deleted = false;

-- Auto-update search_vector on insert/update
CREATE OR REPLACE FUNCTION notes_search_vector_update() RETURNS TRIGGER AS $$
BEGIN
    NEW.search_vector :=
        setweight(to_tsvector('english', COALESCE(NEW.title, '')), 'A') ||
        setweight(to_tsvector('english', COALESCE(NEW.raw_content, '')), 'B');
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER notes_search_vector_trigger
    BEFORE INSERT OR UPDATE OF title, raw_content ON notes
    FOR EACH ROW
    EXECUTE FUNCTION notes_search_vector_update();

-- ── Chunks ─────────────────────────────────────────────────────

CREATE TABLE chunks (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    note_id         UUID NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    chunk_index     INT NOT NULL,
    content         TEXT NOT NULL,
    heading_context TEXT,
    token_count     INT NOT NULL DEFAULT 0,
    UNIQUE (note_id, chunk_index)
);

CREATE INDEX idx_chunks_note_id ON chunks (note_id);

-- ── Embeddings ─────────────────────────────────────────────────

CREATE TABLE embeddings (
    id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    chunk_id    UUID NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
    provider    TEXT NOT NULL,
    model       TEXT NOT NULL,
    vector      vector(768),   -- default dims; will be configurable
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_embeddings_chunk_id ON embeddings (chunk_id);

-- HNSW index for cosine similarity search
CREATE INDEX idx_embeddings_vector ON embeddings
    USING hnsw (vector vector_cosine_ops);

-- ── Tags ───────────────────────────────────────────────────────

CREATE TABLE tags (
    id   UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE note_tags (
    note_id UUID NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    tag_id  UUID NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (note_id, tag_id)
);

CREATE INDEX idx_note_tags_tag_id ON note_tags (tag_id);

-- ── Links ──────────────────────────────────────────────────────

CREATE TABLE links (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    source_note_id  UUID NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    target_note_id  UUID REFERENCES notes(id) ON DELETE SET NULL,
    link_text       TEXT NOT NULL,
    target_path     TEXT NOT NULL,
    context         TEXT
);

CREATE INDEX idx_links_source ON links (source_note_id);
CREATE INDEX idx_links_target ON links (target_note_id);

-- ── Projects ───────────────────────────────────────────────────

CREATE TABLE projects (
    id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name        TEXT NOT NULL UNIQUE,
    root_path   TEXT NOT NULL,
    description TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE note_projects (
    note_id    UUID NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    PRIMARY KEY (note_id, project_id)
);

CREATE INDEX idx_note_projects_project_id ON note_projects (project_id);

-- ── Sync State ─────────────────────────────────────────────────

CREATE TABLE sync_state (
    note_id        UUID PRIMARY KEY REFERENCES notes(id) ON DELETE CASCADE,
    file_hash      TEXT NOT NULL,
    last_synced    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    sync_direction TEXT NOT NULL DEFAULT 'file_to_db'
);

-- ── Skill Runs ─────────────────────────────────────────────────

CREATE TABLE skill_runs (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    skill_name      TEXT NOT NULL,
    started_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at    TIMESTAMPTZ,
    status          TEXT NOT NULL DEFAULT 'running',
    input_params    JSONB,
    output_summary  TEXT
);

CREATE INDEX idx_skill_runs_skill_name ON skill_runs (skill_name);
CREATE INDEX idx_skill_runs_started_at ON skill_runs (started_at DESC);

-- ── Tasks ──────────────────────────────────────────────────────

CREATE TABLE tasks (
    id               UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    title            TEXT NOT NULL,
    status           TEXT NOT NULL DEFAULT 'pending',
    project_id       UUID REFERENCES projects(id) ON DELETE SET NULL,
    due_date         TIMESTAMPTZ,
    created_by_skill TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at     TIMESTAMPTZ
);

CREATE INDEX idx_tasks_status ON tasks (status);
CREATE INDEX idx_tasks_project_id ON tasks (project_id);
