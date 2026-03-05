-- Phase 4: Add lifecycle classification and provenance tracking to notes

-- Lifecycle classification for notes
ALTER TABLE notes ADD COLUMN lifecycle TEXT NOT NULL DEFAULT 'active';
CREATE INDEX idx_notes_lifecycle ON notes (lifecycle) WHERE deleted = false;

-- Provenance tracking for mirrored project notes
ALTER TABLE notes ADD COLUMN source_project TEXT;
ALTER TABLE notes ADD COLUMN source_path TEXT;
ALTER TABLE notes ADD COLUMN source_branch TEXT;
ALTER TABLE notes ADD COLUMN source_commit TEXT;

CREATE INDEX idx_notes_source_project ON notes (source_project) WHERE source_project IS NOT NULL;

-- Add source_note_id to tasks for linking tasks back to their source note
ALTER TABLE tasks ADD COLUMN source_note_id UUID REFERENCES notes(id) ON DELETE SET NULL;
CREATE INDEX idx_tasks_source_note_id ON tasks (source_note_id) WHERE source_note_id IS NOT NULL;
