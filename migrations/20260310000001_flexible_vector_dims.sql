-- Allow flexible embedding dimensions (support multiple models)

-- Drop dimension-specific HNSW index
DROP INDEX IF EXISTS idx_embeddings_vector;

-- Remove fixed 768 dimension constraint from vector column
-- This allows storing embeddings of any dimensionality (768, 1024, 1536, etc.)
ALTER TABLE embeddings ALTER COLUMN vector TYPE vector USING vector::vector;

-- NOTE: HNSW index requires explicit dimensions. It will be created by the
-- setup script or on first embed for the configured dimension size.
-- Example for 768 dims:  CREATE INDEX idx_embeddings_vector ON embeddings USING hnsw ((vector::vector(768)) vector_cosine_ops);
-- Example for 1024 dims: CREATE INDEX idx_embeddings_vector ON embeddings USING hnsw ((vector::vector(1024)) vector_cosine_ops);
