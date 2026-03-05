use crate::models::SkillRun;
use sqlx::PgPool;
use uuid::Uuid;

/// Create a new skill run record.
pub async fn create_skill_run(
    pool: &PgPool,
    skill_name: &str,
    input_params: Option<&serde_json::Value>,
) -> anyhow::Result<SkillRun> {
    let row = sqlx::query_as::<_, SkillRun>(
        r#"
        INSERT INTO skill_runs (id, skill_name, input_params)
        VALUES ($1, $2, $3)
        RETURNING *
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(skill_name)
    .bind(input_params)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Mark a skill run as completed.
pub async fn complete_skill_run(
    pool: &PgPool,
    run_id: Uuid,
    status: &str,
    output_summary: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE skill_runs
        SET completed_at = NOW(), status = $2, output_summary = $3
        WHERE id = $1
        "#,
    )
    .bind(run_id)
    .bind(status)
    .bind(output_summary)
    .execute(pool)
    .await?;
    Ok(())
}

/// Get recent skill runs.
pub async fn list_skill_runs(
    pool: &PgPool,
    skill_name: Option<&str>,
    limit: i64,
) -> anyhow::Result<Vec<SkillRun>> {
    let rows = match skill_name {
        Some(name) => {
            sqlx::query_as::<_, SkillRun>(
                r#"
                SELECT * FROM skill_runs
                WHERE skill_name = $1
                ORDER BY started_at DESC
                LIMIT $2
                "#,
            )
            .bind(name)
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query_as::<_, SkillRun>(
                "SELECT * FROM skill_runs ORDER BY started_at DESC LIMIT $1",
            )
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
    };
    Ok(rows)
}
