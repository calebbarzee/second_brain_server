-- Settings table for runtime configuration.
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Default tracked branch for the shared DB index.
INSERT INTO settings (key, value) VALUES ('tracked_branch', 'main')
ON CONFLICT (key) DO NOTHING;
