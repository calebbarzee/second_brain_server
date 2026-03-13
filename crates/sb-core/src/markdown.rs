use crate::models::{Heading, ParsedLink, ParsedNote, ParsedTask};
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

/// Parse a markdown file's raw content into structured data.
pub fn parse_markdown(raw: &str) -> ParsedNote {
    let (frontmatter, body) = extract_frontmatter(raw);
    let title = extract_title(body);
    let headings = extract_headings(body);
    let links = extract_links(body);
    let tasks = extract_tasks(body);

    ParsedNote {
        title,
        frontmatter,
        content: body.to_string(),
        headings,
        links,
        tasks,
    }
}

/// Extract YAML frontmatter delimited by `---` fences.
fn extract_frontmatter(raw: &str) -> (Option<serde_json::Value>, &str) {
    let trimmed = raw.trim_start();
    if !trimmed.starts_with("---") {
        return (None, raw);
    }

    // Find the closing ---
    let after_open = &trimmed[3..];
    if let Some(close_pos) = after_open.find("\n---") {
        let yaml_str = &after_open[..close_pos].trim();
        let body_start = 3 + close_pos + 4; // skip past closing ---\n
        let body = trimmed[body_start..].trim_start_matches('\n');

        // Try to parse as YAML → JSON value
        match serde_json::from_str::<serde_json::Value>(
            &serde_yaml_frontmatter(yaml_str),
        ) {
            Ok(val) => (Some(val), body),
            Err(_) => (None, raw),
        }
    } else {
        (None, raw)
    }
}

/// Minimal YAML-to-JSON conversion for simple frontmatter.
/// Handles `key: value` pairs and `key: [list]` syntax.
fn serde_yaml_frontmatter(yaml: &str) -> String {
    let mut map = serde_json::Map::new();
    for line in yaml.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, val)) = line.split_once(':') {
            let key = key.trim().to_string();
            let val = val.trim();
            if val.starts_with('[') && val.ends_with(']') {
                // Simple array: [a, b, c]
                let items: Vec<serde_json::Value> = val[1..val.len() - 1]
                    .split(',')
                    .map(|s| serde_json::Value::String(s.trim().to_string()))
                    .collect();
                map.insert(key, serde_json::Value::Array(items));
            } else {
                map.insert(key, serde_json::Value::String(val.to_string()));
            }
        }
    }
    serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string())
}

/// Extract the title: first `# Heading` or first line of content.
fn extract_title(body: &str) -> String {
    let parser = Parser::new(body);
    let mut in_h1 = false;
    let mut title = String::new();

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level: HeadingLevel::H1, .. }) => {
                in_h1 = true;
            }
            Event::Text(text) if in_h1 => {
                title.push_str(&text);
            }
            Event::End(TagEnd::Heading(HeadingLevel::H1)) => {
                if !title.is_empty() {
                    return title;
                }
                in_h1 = false;
            }
            _ => {}
        }
    }

    // Fallback: first non-empty line
    body.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("Untitled")
        .trim_start_matches('#')
        .trim()
        .to_string()
}

/// Extract all headings with their levels.
fn extract_headings(body: &str) -> Vec<Heading> {
    let parser = Parser::new(body);
    let mut headings = Vec::new();
    let mut current_level: Option<u8> = None;
    let mut current_text = String::new();

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                current_level = Some(heading_level_to_u8(level));
                current_text.clear();
            }
            Event::Text(text) if current_level.is_some() => {
                current_text.push_str(&text);
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(level) = current_level.take() {
                    headings.push(Heading {
                        level,
                        text: current_text.clone(),
                    });
                }
            }
            _ => {}
        }
    }
    headings
}

fn heading_level_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Extract markdown links `[text](target)` and `[[wikilinks]]`.
fn extract_links(body: &str) -> Vec<ParsedLink> {
    let mut links = Vec::new();

    // Standard markdown links via pulldown-cmark
    let parser = Parser::new(body);
    let mut in_link: Option<String> = None;
    let mut link_text = String::new();

    for event in parser {
        match event {
            Event::Start(Tag::Link { dest_url, .. }) => {
                in_link = Some(dest_url.to_string());
                link_text.clear();
            }
            Event::Text(text) if in_link.is_some() => {
                link_text.push_str(&text);
            }
            Event::End(TagEnd::Link) => {
                if let Some(target) = in_link.take() {
                    links.push(ParsedLink {
                        link_text: link_text.clone(),
                        target,
                        is_wikilink: false,
                    });
                }
            }
            _ => {}
        }
    }

    // Wikilinks: [[target]] or [[target|display text]]
    let wikilink_re_str = r"\[\[([^\]|]+)(?:\|([^\]]+))?\]\]";
    // Simple regex-free wikilink extraction
    let mut rest = body;
    while let Some(start) = rest.find("[[") {
        let after_open = &rest[start + 2..];
        if let Some(end) = after_open.find("]]") {
            let inner = &after_open[..end];
            let (target, display) = match inner.split_once('|') {
                Some((t, d)) => (t.trim(), d.trim()),
                None => (inner.trim(), inner.trim()),
            };
            links.push(ParsedLink {
                link_text: display.to_string(),
                target: target.to_string(),
                is_wikilink: true,
            });
            rest = &after_open[end + 2..];
        } else {
            break;
        }
    }

    // Suppress unused variable warning for the regex string
    let _ = wikilink_re_str;

    links
}

/// Extract task items from markdown checkbox syntax: `- [ ]` and `- [x]`.
pub fn extract_tasks(body: &str) -> Vec<ParsedTask> {
    let mut tasks = Vec::new();

    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
            tasks.push(ParsedTask {
                title: rest.trim().to_string(),
                completed: false,
            });
        } else if let Some(rest) = trimmed
            .strip_prefix("- [x] ")
            .or_else(|| trimmed.strip_prefix("- [X] "))
        {
            tasks.push(ParsedTask {
                title: rest.trim().to_string(),
                completed: true,
            });
        }
    }

    tasks
}

// ── Frontmatter writing ──────────────────────────────────────

/// Serialize a JSON value back into YAML frontmatter string (with `---` fences).
fn serialize_frontmatter(value: &serde_json::Value) -> String {
    let mut lines = Vec::new();
    if let serde_json::Value::Object(map) = value {
        for (key, val) in map {
            match val {
                serde_json::Value::Array(arr) => {
                    let items: Vec<String> = arr
                        .iter()
                        .map(|v| match v {
                            serde_json::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                        .collect();
                    lines.push(format!("{key}: [{}]", items.join(", ")));
                }
                serde_json::Value::String(s) => {
                    lines.push(format!("{key}: {s}"));
                }
                other => {
                    lines.push(format!("{key}: {other}"));
                }
            }
        }
    }
    format!("---\n{}\n---\n", lines.join("\n"))
}

/// Inject edit metadata into a note's frontmatter.
///
/// Sets `edited_by` and `last_<editor>_edit` with a timestamp.
/// Creates frontmatter if the note doesn't have any.
/// Preserves all existing frontmatter fields.
///
/// # Arguments
/// - `raw_content`: the full note markdown (may or may not have frontmatter)
/// - `editor`: who made the edit — `"ai"` or a username like `"calebbarzee"`
///
/// # Frontmatter fields set
/// - `edited_by: <editor>`
/// - `last_<editor>_edit: <ISO 8601 timestamp>`
pub fn stamp_edit(raw_content: &str, editor: &str) -> String {
    let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let (existing_fm, body) = extract_frontmatter(raw_content);

    let mut map = match existing_fm {
        Some(serde_json::Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    };

    map.insert(
        "edited_by".to_string(),
        serde_json::Value::String(editor.to_string()),
    );
    map.insert(
        format!("last_{editor}_edit"),
        serde_json::Value::String(timestamp),
    );

    let fm_str = serialize_frontmatter(&serde_json::Value::Object(map));
    format!("{fm_str}\n{body}")
}

/// Compute a content hash for deduplication.
pub fn content_hash(content: &str) -> String {
    let hash = xxhash_rust::xxh3::xxh3_64(content.as_bytes());
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_note() {
        let raw = "# My Note\n\nSome content here.\n\n## Section One\n\nMore text.\n";
        let parsed = parse_markdown(raw);
        assert_eq!(parsed.title, "My Note");
        assert_eq!(parsed.headings.len(), 2);
        assert_eq!(parsed.headings[0].level, 1);
        assert_eq!(parsed.headings[0].text, "My Note");
        assert_eq!(parsed.headings[1].level, 2);
        assert_eq!(parsed.headings[1].text, "Section One");
    }

    #[test]
    fn test_parse_frontmatter() {
        let raw = "---\ntitle: Test Note\ntags: [rust, mcp]\n---\n\n# Content\n\nBody here.\n";
        let parsed = parse_markdown(raw);
        let fm = parsed.frontmatter.unwrap();
        assert_eq!(fm["title"], "Test Note");
        assert!(fm["tags"].is_array());
    }

    #[test]
    fn test_parse_no_frontmatter() {
        let raw = "# Just a heading\n\nBody text.\n";
        let parsed = parse_markdown(raw);
        assert!(parsed.frontmatter.is_none());
        assert_eq!(parsed.title, "Just a heading");
    }

    #[test]
    fn test_extract_markdown_links() {
        let raw = "# Links\n\nCheck [this out](./other.md) and [web](https://example.com).\n";
        let parsed = parse_markdown(raw);
        assert_eq!(parsed.links.len(), 2);
        assert_eq!(parsed.links[0].link_text, "this out");
        assert_eq!(parsed.links[0].target, "./other.md");
        assert!(!parsed.links[0].is_wikilink);
    }

    #[test]
    fn test_extract_wikilinks() {
        let raw = "# Wiki\n\nSee [[some note]] and [[path/to/note|display name]].\n";
        let parsed = parse_markdown(raw);
        let wikilinks: Vec<_> = parsed.links.iter().filter(|l| l.is_wikilink).collect();
        assert_eq!(wikilinks.len(), 2);
        assert_eq!(wikilinks[0].target, "some note");
        assert_eq!(wikilinks[0].link_text, "some note");
        assert_eq!(wikilinks[1].target, "path/to/note");
        assert_eq!(wikilinks[1].link_text, "display name");
    }

    #[test]
    fn test_content_hash_deterministic() {
        let content = "Hello, world!";
        let h1 = content_hash(content);
        let h2 = content_hash(content);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 16); // 64-bit hex
    }

    #[test]
    fn test_content_hash_different_for_different_content() {
        let h1 = content_hash("Hello");
        let h2 = content_hash("World");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_extract_tasks() {
        let raw = "# Tasks\n\n- [ ] Buy groceries\n- [x] Write tests\n- [X] Ship feature\n- Not a task\n";
        let parsed = parse_markdown(raw);
        assert_eq!(parsed.tasks.len(), 3);
        assert!(!parsed.tasks[0].completed);
        assert_eq!(parsed.tasks[0].title, "Buy groceries");
        assert!(parsed.tasks[1].completed);
        assert_eq!(parsed.tasks[1].title, "Write tests");
        assert!(parsed.tasks[2].completed);
    }

    #[test]
    fn test_stamp_edit_ai_no_frontmatter() {
        let raw = "# My Note\n\nBody text.\n";
        let stamped = stamp_edit(raw, "ai");
        assert!(stamped.starts_with("---\n"));
        assert!(stamped.contains("edited_by: ai"));
        assert!(stamped.contains("last_ai_edit:"));
        assert!(stamped.contains("# My Note"));
    }

    #[test]
    fn test_stamp_edit_ai_existing_frontmatter() {
        let raw = "---\ntitle: Test\ntags: [rust, mcp]\n---\n\n# Content\n";
        let stamped = stamp_edit(raw, "ai");
        assert!(stamped.contains("title: Test"));
        assert!(stamped.contains("tags: [rust, mcp]"));
        assert!(stamped.contains("edited_by: ai"));
        assert!(stamped.contains("# Content"));
    }

    #[test]
    fn test_stamp_edit_updates_existing_stamp() {
        let raw = "---\nedited_by: ai\nlast_ai_edit: 2020-01-01T00:00:00Z\n---\n\n# Old\n";
        let stamped = stamp_edit(raw, "ai");
        assert_eq!(stamped.matches("edited_by").count(), 1);
        assert!(!stamped.contains("2020-01-01"));
    }

    #[test]
    fn test_stamp_edit_human() {
        let raw = "# Note\n\nContent.\n";
        let stamped = stamp_edit(raw, "calebbarzee");
        assert!(stamped.contains("edited_by: calebbarzee"));
        assert!(stamped.contains("last_calebbarzee_edit:"));
        assert!(stamped.contains("# Note"));
    }

    #[test]
    fn test_stamp_edit_preserves_other_editor_timestamps() {
        let raw = "---\nedited_by: ai\nlast_ai_edit: 2026-01-01T00:00:00Z\n---\n\n# Note\n";
        let stamped = stamp_edit(raw, "calebbarzee");
        // Should update edited_by to human, add human timestamp, keep AI timestamp
        assert!(stamped.contains("edited_by: calebbarzee"));
        assert!(stamped.contains("last_calebbarzee_edit:"));
        assert!(stamped.contains("last_ai_edit: 2026-01-01"));
    }

    #[test]
    fn test_serialize_frontmatter_roundtrip() {
        let raw = "---\ntitle: Test\ntags: [a, b]\n---\n\n# Body\n";
        let (fm, _body) = extract_frontmatter(raw);
        let serialized = serialize_frontmatter(&fm.unwrap());
        assert!(serialized.contains("title: Test"));
        assert!(serialized.contains("tags: [a, b]"));
    }

    #[test]
    fn test_title_fallback_no_h1() {
        let raw = "## Not an H1\n\nSome body text.\n";
        let parsed = parse_markdown(raw);
        // Falls back to first non-empty line, stripped of leading ##
        assert_eq!(parsed.title, "Not an H1");
    }
}
