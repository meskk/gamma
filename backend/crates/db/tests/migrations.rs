//! Integration test against a real Postgres.
//!
//! `#[sqlx::test]` spins up an isolated temporary database, runs our migrations
//! into it, and hands us a pool — then drops the database afterwards. This is the
//! template every future DB test follows: no shared state, no cleanup code.
//!
//! Requires `DATABASE_URL` pointing at a reachable Postgres server (see `.env`);
//! the configured user needs CREATE DATABASE (the dev container's user has it).

use sqlx::PgPool;

#[sqlx::test(migrations = "../../migrations")]
async fn migrations_apply_and_core_tables_exist(pool: PgPool) {
    // interaction_events is the week-one, can't-backfill capture (Dossier B.1).
    // If migrations ran, it must exist.
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT FROM information_schema.tables
            WHERE table_schema = 'public' AND table_name = 'interaction_events'
        )",
    )
    .fetch_one(&pool)
    .await
    .expect("information_schema query should run");

    assert!(
        exists,
        "interaction_events table must exist after migrations"
    );

    // The readiness probe should succeed against a live, migrated database.
    db::ping(&pool).await.expect("ping should succeed");
}
