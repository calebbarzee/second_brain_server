use sb_core::models::CreateChunk;
use uuid::Uuid;

/// Configuration for the chunker.
pub struct ChunkerConfig {
    /// Maximum number of characters per chunk (approximate).
    pub max_chunk_chars: usize,
    /// Overlap in characters between consecutive sliding-window chunks.
    pub overlap_chars: usize,
}

impl Default for ChunkerConfig {
    fn default() -> Self {
        Self {
            max_chunk_chars: 1500,  // ~375 tokens at ~4 chars/token
            overlap_chars: 200,
        }
    }
}

pub struct Chunker {
    config: ChunkerConfig,
}

impl Chunker {
    pub fn new(config: ChunkerConfig) -> Self {
        Self { config }
    }

    /// Split a note's content into chunks suitable for embedding.
    /// Strategy:
    /// 1. Split by ## headings (level 2+)
    /// 2. If a section is too long, use sliding window
    /// 3. Preserve heading context for each chunk
    pub fn chunk(&self, note_id: Uuid, content: &str) -> Vec<CreateChunk> {
        let sections = split_by_headings(content);
        let mut chunks = Vec::new();
        let mut chunk_index = 0i32;

        for section in sections {
            if section.text.trim().is_empty() {
                continue;
            }

            if section.text.len() <= self.config.max_chunk_chars {
                // Section fits in one chunk
                chunks.push(CreateChunk {
                    note_id,
                    chunk_index,
                    content: section.text.clone(),
                    heading_context: section.heading.clone(),
                    token_count: estimate_tokens(&section.text),
                });
                chunk_index += 1;
            } else {
                // Section too long — sliding window
                let sub_chunks =
                    sliding_window(&section.text, self.config.max_chunk_chars, self.config.overlap_chars);
                for sub in sub_chunks {
                    chunks.push(CreateChunk {
                        note_id,
                        chunk_index,
                        content: sub,
                        heading_context: section.heading.clone(),
                        token_count: 0, // will be recalculated below
                    });
                    chunk_index += 1;
                }
            }
        }

        // Recalculate token counts
        for chunk in &mut chunks {
            chunk.token_count = estimate_tokens(&chunk.content);
        }

        chunks
    }
}

impl Default for Chunker {
    fn default() -> Self {
        Self::new(ChunkerConfig::default())
    }
}

/// A section of a markdown document.
struct Section {
    heading: Option<String>,
    text: String,
}

/// Split markdown content by headings (## and above).
/// Each section includes the heading line and all content until the next heading.
fn split_by_headings(content: &str) -> Vec<Section> {
    let mut sections = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut current_text = String::new();

    for line in content.lines() {
        if is_heading(line) {
            // Save current section
            if !current_text.is_empty() || current_heading.is_some() {
                sections.push(Section {
                    heading: current_heading.take(),
                    text: std::mem::take(&mut current_text),
                });
            }
            current_heading = Some(extract_heading_text(line));
            current_text.push_str(line);
            current_text.push('\n');
        } else {
            current_text.push_str(line);
            current_text.push('\n');
        }
    }

    // Don't forget the last section
    if !current_text.is_empty() || current_heading.is_some() {
        sections.push(Section {
            heading: current_heading,
            text: current_text,
        });
    }

    sections
}

fn is_heading(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("# ") || trimmed.starts_with("## ") || trimmed.starts_with("### ")
}

fn extract_heading_text(line: &str) -> String {
    line.trim_start_matches('#').trim().to_string()
}

/// Sliding window split for content that exceeds max_chars.
/// Tries to break at paragraph boundaries (double newline).
fn sliding_window(text: &str, max_chars: usize, overlap: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut start = 0;

    while start < text.len() {
        let end = (start + max_chars).min(text.len());

        // Try to find a paragraph break near the end
        let actual_end = if end < text.len() {
            find_break_point(text, start, end)
        } else {
            end
        };

        let chunk = text[start..actual_end].trim().to_string();
        if !chunk.is_empty() {
            chunks.push(chunk);
        }

        if actual_end >= text.len() {
            break;
        }

        // Advance with overlap
        start = if actual_end > overlap {
            actual_end - overlap
        } else {
            actual_end
        };
    }

    chunks
}

/// Find a good break point near `target` (paragraph break > sentence end > word boundary).
fn find_break_point(text: &str, start: usize, target: usize) -> usize {
    let search_start = if target > 200 { target - 200 } else { start };
    let region = &text[search_start..target];

    // Prefer paragraph break
    if let Some(pos) = region.rfind("\n\n") {
        return search_start + pos + 2;
    }
    // Then sentence end
    if let Some(pos) = region.rfind(". ") {
        return search_start + pos + 2;
    }
    // Then any newline
    if let Some(pos) = region.rfind('\n') {
        return search_start + pos + 1;
    }
    // Fallback: word boundary
    if let Some(pos) = region.rfind(' ') {
        return search_start + pos + 1;
    }
    target
}

/// Rough token count estimate (~4 chars per token for English).
fn estimate_tokens(text: &str) -> i32 {
    (text.len() as f64 / 4.0).ceil() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_simple_note() {
        let chunker = Chunker::default();
        let note_id = Uuid::new_v4();
        let content = "# My Note\n\nSome intro text.\n\n## Section One\n\nContent for section one.\n\n## Section Two\n\nContent for section two.\n";

        let chunks = chunker.chunk(note_id, content);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].heading_context.as_deref(), Some("My Note"));
        assert!(chunks[0].content.contains("My Note"));
        assert_eq!(chunks[1].heading_context.as_deref(), Some("Section One"));
        assert!(chunks[1].content.contains("Content for section one"));
        assert_eq!(chunks[2].heading_context.as_deref(), Some("Section Two"));
    }

    #[test]
    fn test_chunk_no_headings() {
        let chunker = Chunker::default();
        let note_id = Uuid::new_v4();
        let content = "Just some plain text without any headings.\nAnother line.\n";

        let chunks = chunker.chunk(note_id, content);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].heading_context.is_none());
    }

    #[test]
    fn test_chunk_long_section_splits() {
        let chunker = Chunker::new(ChunkerConfig {
            max_chunk_chars: 100,
            overlap_chars: 20,
        });
        let note_id = Uuid::new_v4();
        // Create content longer than 100 chars
        let long_text = "word ".repeat(50); // 250 chars
        let content = format!("# Title\n\n{long_text}");

        let chunks = chunker.chunk(note_id, &content);
        assert!(chunks.len() > 1, "Long section should be split into multiple chunks");
        for chunk in &chunks {
            assert!(chunk.content.len() <= 120, "Chunks should respect max size (with some tolerance)");
        }
    }

    #[test]
    fn test_token_estimate() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("hello world"), 3); // 11 chars / 4 = 2.75 → 3
        // 100 chars → 25 tokens
        let hundred = "a".repeat(100);
        assert_eq!(estimate_tokens(&hundred), 25);
    }

    #[test]
    fn test_chunk_empty_content() {
        let chunker = Chunker::default();
        let note_id = Uuid::new_v4();
        let chunks = chunker.chunk(note_id, "");
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_chunk_preserves_all_note_id() {
        let chunker = Chunker::default();
        let note_id = Uuid::new_v4();
        let content = "# A\n\nText.\n\n## B\n\nMore.\n";
        let chunks = chunker.chunk(note_id, content);
        for chunk in &chunks {
            assert_eq!(chunk.note_id, note_id);
        }
    }

    #[test]
    fn test_chunk_indices_sequential() {
        let chunker = Chunker::default();
        let note_id = Uuid::new_v4();
        let content = "# A\n\nText.\n\n## B\n\nMore.\n\n## C\n\nEven more.\n";
        let chunks = chunker.chunk(note_id, content);
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.chunk_index, i as i32);
        }
    }
}
