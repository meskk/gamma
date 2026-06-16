//! Database foundation: the Postgres connection pool and migration runner.
//!
//! Both `core-api` and (later) the Postgres-backed `OffChainLedger` depend on
//! this crate, so pool setup and migrations live in exactly one place. Migrations
//! are embedded at compile time from the workspace `migrations/` directory and
//! applied on startup; they are forward-only.
//!
//! `PgPool` is re-exported so callers don't need a direct `sqlx` dependency.

pub use sqlx::PgPool;

use sqlx::postgres::PgPoolOptions;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("failed to connect to database: {0}")]
    Connect(#[source] sqlx::Error),
    #[error("failed to run migrations: {0}")]
    Migrate(#[source] sqlx::migrate::MigrateError),
    #[error("database query failed: {0}")]
    Query(#[source] sqlx::Error),
}

/// Embedded, forward-only migrations from the workspace `migrations/` directory.
/// The path is resolved at compile time relative to this crate's manifest.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

/// Open a connection pool. `max_connections` bounds concurrent DB work; size it
/// to the database's connection budget, not the number of app instances.
pub async fn connect(database_url: &str, max_connections: u32) -> Result<PgPool, DbError> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(database_url)
        .await
        .map_err(DbError::Connect)
}

/// Apply all pending migrations. Idempotent: already-applied migrations are
/// skipped, so this is safe to call on every startup.
pub async fn run_migrations(pool: &PgPool) -> Result<(), DbError> {
    MIGRATOR.run(pool).await.map_err(DbError::Migrate)
}

/// Readiness probe — confirms the pool can reach Postgres and run a trivial query.
/// Liveness (is the process up) is separate and needs no database.
pub async fn ping(pool: &PgPool) -> Result<(), DbError> {
    sqlx::query("SELECT 1")
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(DbError::Query)
}
