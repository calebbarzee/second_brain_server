use crate::db::notes;
use crate::models::{Note, Project};
use sqlx::PgPool;
use uuid::Uuid;

/// Create or update a project.
pub async fn upsert_project(
    pool: &PgPool,
    name: &str,
    root_path: &str,
    description: Option<&str>,
) -> anyhow::Result<Project> {
    let row = sqlx::query_as::<_, Project>(
        r#"
        INSERT INTO projects (id, name, root_path, description)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (name) DO UPDATE SET
            root_path = EXCLUDED.root_path,
            description = COALESCE(EXCLUDED.description, projects.description)
        RETURNING *
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(name)
    .bind(root_path)
    .bind(description)
    .fetch_one(pool)
    .await?;

    Ok(row)
}

/// Get a project by name.
pub async fn get_project_by_name(pool: &PgPool, name: &str) -> anyhow::Result<Option<Project>> {
    let row = sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE name = $1")
        .bind(name)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// Get a project by ID.
pub async fn get_project_by_id(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Project>> {
    let row = sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// List all projects.
pub async fn list_projects(pool: &PgPool) -> anyhow::Result<Vec<Project>> {
    let rows = sqlx::query_as::<_, Project>("SELECT * FROM projects ORDER BY name")
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// Associate a note with a project.
pub async fn associate_note_project(
    pool: &PgPool,
    note_id: Uuid,
    project_id: Uuid,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO note_projects (note_id, project_id)
        VALUES ($1, $2)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(note_id)
    .bind(project_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Get all notes associated with a project.
pub async fn get_notes_for_project(
    pool: &PgPool,
    project_id: Uuid,
    limit: i64,
) -> anyhow::Result<Vec<Note>> {
    let sql = format!(
        "SELECT {} FROM notes n
         JOIN note_projects np ON np.note_id = n.id
         WHERE np.project_id = $1 AND n.deleted = false
         ORDER BY n.updated_at DESC
         LIMIT $2",
        notes::NOTE_COLS_N,
    );
    let rows = sqlx::query_as::<_, Note>(&sql)
        .bind(project_id)
        .bind(limit)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// Count notes per project.
#[derive(Debug, sqlx::FromRow, serde::Serialize)]
pub struct ProjectNoteCount {
    pub project_name: String,
    pub project_id: Uuid,
    pub note_count: i64,
}

pub async fn list_projects_with_counts(pool: &PgPool) -> anyhow::Result<Vec<ProjectNoteCount>> {
    let rows = sqlx::query_as::<_, ProjectNoteCount>(
        r#"
        SELECT p.name as project_name, p.id as project_id,
               COUNT(np.note_id) as note_count
        FROM projects p
        LEFT JOIN note_projects np ON np.project_id = p.id
        LEFT JOIN notes n ON n.id = np.note_id AND n.deleted = false
        GROUP BY p.id, p.name
        ORDER BY p.name
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Get notes not associated with any project.
pub async fn get_unassociated_notes(pool: &PgPool, limit: i64) -> anyhow::Result<Vec<Note>> {
    let sql = format!(
        "SELECT {} FROM notes n
         LEFT JOIN note_projects np ON np.note_id = n.id
         WHERE np.note_id IS NULL AND n.deleted = false
         ORDER BY n.updated_at DESC
         LIMIT $1",
        notes::NOTE_COLS_N,
    );
    let rows = sqlx::query_as::<_, Note>(&sql)
        .bind(limit)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}
