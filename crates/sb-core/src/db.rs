pub mod chunks;
pub mod embeddings;
pub mod links;
pub mod notes;
pub mod projects;
pub mod queries;
pub mod skill_runs;
pub mod sync_state;
pub mod tags;
pub mod tasks;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// Shared database handle wrapping a connection pool.
#[derive(Debug, Clone)]
pub struct Database {
    pool: PgPool,
}

impl Database {
    /// Connect to PostgreSQL and run migrations.
    pub async fn connect(database_url: &str) -> anyhow::Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await?;

        tracing::info!("connected to database");

        // Run embedded migrations
        sqlx::migrate!("../../migrations").run(&pool).await?;
        tracing::info!("migrations applied");

        Ok(Self { pool })
    }

    /// Connect without running migrations (for testing with pre-migrated DBs).
    pub async fn connect_no_migrate(database_url: &str) -> anyhow::Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}
