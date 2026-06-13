//! Postgres-backed user repository — the ONLY place that knows users SQL.
//!
//! Concrete (not a trait) on purpose: there is a single backing (Postgres) and
//! tests run against a real DB via `#[sqlx::test]`, so a mock-enabling trait would
//! be premature abstraction. Extract a trait later only if a second backing
//! actually appears. Queries use `query_as!` so they are checked against the live
//! schema at compile time (the `.sqlx` cache lets CI do this without a database).

use crate::users::model::{NewUser, User};
use db::PgPool;

#[derive(Clone)]
pub struct UserRepository {
    pool: PgPool,
}

impl UserRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, new: &NewUser) -> Result<User, sqlx::Error> {
        sqlx::query_as!(
            User,
            r#"
            INSERT INTO users (declared_categories, bot_gate_v)
            VALUES ($1, $2)
            RETURNING id, created_at, declared_categories, bot_gate_v
            "#,
            &new.declared_categories,
            new.bot_gate_v
        )
        .fetch_one(&self.pool)
        .await
    }

    pub async fn get(&self, id: i64) -> Result<Option<User>, sqlx::Error> {
        sqlx::query_as!(
            User,
            r#"
            SELECT id, created_at, declared_categories, bot_gate_v
            FROM users
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(&self.pool)
        .await
    }
}
