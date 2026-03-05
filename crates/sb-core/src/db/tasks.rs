use crate::models::Task;
use sqlx::PgPool;
use uuid::Uuid;

/// Create a new task.
pub async fn create_task(
    pool: &PgPool,
    title: &str,
    project_id: Option<Uuid>,
    source_note_id: Option<Uuid>,
    created_by_skill: Option<&str>,
) -> anyhow::Result<Task> {
    let row = sqlx::query_as::<_, Task>(
        r#"
        INSERT INTO tasks (id, title, project_id, source_note_id, created_by_skill)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING *
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(title)
    .bind(project_id)
    .bind(source_note_id)
    .bind(created_by_skill)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Update a task's status.
pub async fn update_task_status(pool: &PgPool, task_id: Uuid, status: &str) -> anyhow::Result<()> {
    let completed_at = if status == "completed" {
        Some(chrono::Utc::now())
    } else {
        None
    };

    sqlx::query(
        r#"
        UPDATE tasks SET status = $2, completed_at = $3
        WHERE id = $1
        "#,
    )
    .bind(task_id)
    .bind(status)
    .bind(completed_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// List tasks filtered by status and/or project.
pub async fn list_tasks(
    pool: &PgPool,
    status: Option<&str>,
    project_id: Option<Uuid>,
    limit: i64,
) -> anyhow::Result<Vec<Task>> {
    let rows = sqlx::query_as::<_, Task>(
        r#"
        SELECT * FROM tasks
        WHERE ($1::TEXT IS NULL OR status = $1)
          AND ($2::UUID IS NULL OR project_id = $2)
        ORDER BY created_at DESC
        LIMIT $3
        "#,
    )
    .bind(status)
    .bind(project_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Get open tasks for a project.
pub async fn get_open_tasks_for_project(
    pool: &PgPool,
    project_id: Uuid,
) -> anyhow::Result<Vec<Task>> {
    let rows = sqlx::query_as::<_, Task>(
        r#"
        SELECT * FROM tasks
        WHERE project_id = $1 AND status != 'completed'
        ORDER BY created_at DESC
        "#,
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Get all open tasks (across all projects).
pub async fn get_all_open_tasks(pool: &PgPool, limit: i64) -> anyhow::Result<Vec<Task>> {
    let rows = sqlx::query_as::<_, Task>(
        r#"
        SELECT * FROM tasks
        WHERE status != 'completed'
        ORDER BY created_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Get completed tasks in a date range (for summarization/condensation).
pub async fn get_completed_tasks_in_range(
    pool: &PgPool,
    start: chrono::DateTime<chrono::Utc>,
    end: chrono::DateTime<chrono::Utc>,
    project_id: Option<Uuid>,
) -> anyhow::Result<Vec<Task>> {
    let rows = sqlx::query_as::<_, Task>(
        r#"
        SELECT * FROM tasks
        WHERE status = 'completed'
          AND completed_at >= $1 AND completed_at < $2
          AND ($3::UUID IS NULL OR project_id = $3)
        ORDER BY completed_at DESC
        "#,
    )
    .bind(start)
    .bind(end)
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
