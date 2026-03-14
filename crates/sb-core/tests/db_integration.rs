use sb_core::Database;
use sb_core::db::notes;
use sb_core::models::CreateNote;

fn test_db_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://secondbrain:secondbrain@localhost:5432/secondbrain".to_string()
    })
}

#[tokio::test]
async fn test_db_connection() {
    let db = Database::connect(&test_db_url()).await.unwrap();
    // Simple connectivity check
    let row: (i32,) = sqlx::query_as("SELECT 1")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(row.0, 1);
}

#[tokio::test]
async fn test_pgvector_extension() {
    let db = Database::connect(&test_db_url()).await.unwrap();
    let row: (String,) =
        sqlx::query_as("SELECT extname FROM pg_extension WHERE extname = 'vector'")
            .fetch_one(db.pool())
            .await
            .unwrap();
    assert_eq!(row.0, "vector");
}

#[tokio::test]
async fn test_note_crud() {
    let db = Database::connect(&test_db_url()).await.unwrap();
    let pool = db.pool();

    // Clean up any previous test data (scoped to this test's prefix)
    sqlx::query("DELETE FROM notes WHERE file_path LIKE '/test/crud-%'")
        .execute(pool)
        .await
        .unwrap();

    // Create
    let note = CreateNote {
        file_path: "/test/crud-test.md".to_string(),
        title: "CRUD Test Note".to_string(),
        content_hash: "abc123".to_string(),
        raw_content: "# CRUD Test Note\n\nThis is a test.".to_string(),
        frontmatter: None,
    };
    let created = notes::upsert_note(pool, &note).await.unwrap();
    assert_eq!(created.title, "CRUD Test Note");
    assert_eq!(created.file_path, "/test/crud-test.md");

    // Read by path
    let fetched = notes::get_note_by_path(pool, "/test/crud-test.md")
        .await
        .unwrap()
        .expect("note should exist");
    assert_eq!(fetched.id, created.id);

    // Read by ID
    let fetched_by_id = notes::get_note_by_id(pool, created.id)
        .await
        .unwrap()
        .expect("note should exist");
    assert_eq!(fetched_by_id.title, "CRUD Test Note");

    // Update (upsert same path with different content)
    let updated_note = CreateNote {
        file_path: "/test/crud-test.md".to_string(),
        title: "Updated Title".to_string(),
        content_hash: "def456".to_string(),
        raw_content: "# Updated Title\n\nUpdated content.".to_string(),
        frontmatter: Some(serde_json::json!({"tags": ["test"]})),
    };
    let updated = notes::upsert_note(pool, &updated_note).await.unwrap();
    assert_eq!(updated.id, created.id); // same ID (upsert)
    assert_eq!(updated.title, "Updated Title");
    assert!(updated.frontmatter.is_some());

    // List
    let all = notes::list_notes(pool, 100, 0).await.unwrap();
    assert!(all.iter().any(|n| n.id == created.id));

    // Soft delete
    let deleted = notes::soft_delete_note(pool, "/test/crud-test.md")
        .await
        .unwrap();
    assert!(deleted);

    // Should not appear in queries after soft delete
    let gone = notes::get_note_by_path(pool, "/test/crud-test.md")
        .await
        .unwrap();
    assert!(gone.is_none());

    // Cleanup
    sqlx::query("DELETE FROM notes WHERE file_path LIKE '/test/crud-%'")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_content_hash_change_detection() {
    let db = Database::connect(&test_db_url()).await.unwrap();
    let pool = db.pool();

    // Clean up
    sqlx::query("DELETE FROM notes WHERE file_path = '/test/hash-test.md'")
        .execute(pool)
        .await
        .unwrap();

    // New note should be "changed" (doesn't exist yet)
    let changed = notes::note_content_changed(pool, "/test/hash-test.md", "hash1")
        .await
        .unwrap();
    assert!(changed);

    // Insert the note
    let note = CreateNote {
        file_path: "/test/hash-test.md".to_string(),
        title: "Hash Test".to_string(),
        content_hash: "hash1".to_string(),
        raw_content: "content".to_string(),
        frontmatter: None,
    };
    notes::upsert_note(pool, &note).await.unwrap();

    // Same hash should NOT be changed
    let unchanged = notes::note_content_changed(pool, "/test/hash-test.md", "hash1")
        .await
        .unwrap();
    assert!(!unchanged);

    // Different hash SHOULD be changed
    let changed = notes::note_content_changed(pool, "/test/hash-test.md", "hash2")
        .await
        .unwrap();
    assert!(changed);

    // Cleanup
    sqlx::query("DELETE FROM notes WHERE file_path = '/test/hash-test.md'")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_fulltext_search() {
    let db = Database::connect(&test_db_url()).await.unwrap();
    let pool = db.pool();

    // Clean up
    sqlx::query("DELETE FROM notes WHERE file_path LIKE '/test/search%'")
        .execute(pool)
        .await
        .unwrap();

    // Insert notes with different content
    let notes_data = vec![
        CreateNote {
            file_path: "/test/search-rust.md".to_string(),
            title: "Rust Programming".to_string(),
            content_hash: "s1".to_string(),
            raw_content:
                "Rust is a systems programming language focused on safety and performance."
                    .to_string(),
            frontmatter: None,
        },
        CreateNote {
            file_path: "/test/search-python.md".to_string(),
            title: "Python Basics".to_string(),
            content_hash: "s2".to_string(),
            raw_content:
                "Python is an interpreted language popular for data science and scripting."
                    .to_string(),
            frontmatter: None,
        },
        CreateNote {
            file_path: "/test/search-cooking.md".to_string(),
            title: "Cooking Tips".to_string(),
            content_hash: "s3".to_string(),
            raw_content: "Always preheat the oven before baking. Use fresh ingredients."
                .to_string(),
            frontmatter: None,
        },
    ];

    for note in &notes_data {
        notes::upsert_note(pool, note).await.unwrap();
    }

    // Search for "programming" — should find Rust note
    let results = notes::search_notes(pool, "programming", 10).await.unwrap();
    assert!(!results.is_empty());
    assert!(
        results
            .iter()
            .any(|n| n.file_path == "/test/search-rust.md")
    );

    // Search for "cooking" — should find cooking note only
    let results = notes::search_notes(pool, "cooking", 10).await.unwrap();
    assert!(
        results
            .iter()
            .any(|n| n.file_path == "/test/search-cooking.md")
    );
    assert!(
        !results
            .iter()
            .any(|n| n.file_path == "/test/search-rust.md")
    );

    // Search for "language" — should find Rust and Python
    let results = notes::search_notes(pool, "language", 10).await.unwrap();
    assert!(results.len() >= 2);

    // Cleanup
    sqlx::query("DELETE FROM notes WHERE file_path LIKE '/test/search%'")
        .execute(pool)
        .await
        .unwrap();
}
