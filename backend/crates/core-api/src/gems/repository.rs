//! Settlement-marker persistence — the `epoch_settlements` idempotency guard.

use db::PgPool;
use sqlx::PgConnection;

/// The recorded outcome of an already-settled epoch (read from the marker on the
/// fast path, so we don't recompute the graph to answer "is it done?").
pub struct SettledMarker {
    pub emission: i64,
    pub user_count: i32,
}

#[derive(Clone)]
pub struct GemRepository {
    pool: PgPool,
}

impl GemRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// The settlement marker for an epoch, if it has been settled — `None`
    /// otherwise. Cheap (indexed PK lookup), so it is the FIRST thing settlement
    /// checks: an already-settled epoch returns here before the expensive graph
    /// build + PageRank ever run.
    pub async fn settled_marker(&self, epoch_k: i64) -> Result<Option<SettledMarker>, sqlx::Error> {
        sqlx::query_as!(
            SettledMarker,
            r#"SELECT emission, user_count FROM epoch_settlements WHERE epoch_k = $1"#,
            epoch_k
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// Serialize settlers of the SAME epoch with a transaction-scoped advisory lock
    /// (auto-released on commit/rollback). Two concurrent settlers — the scheduler
    /// and a manual `POST /epochs/:k/settle`, or two scheduler instances — would
    /// otherwise both pass the "is it settled?" check and interleave their mints;
    /// this makes the whole check→mint→mark sequence atomic per epoch.
    pub async fn lock_epoch(conn: &mut PgConnection, epoch_k: i64) -> Result<(), sqlx::Error> {
        sqlx::query!("SELECT pg_advisory_xact_lock($1)", epoch_k)
            .execute(conn)
            .await?;
        Ok(())
    }

    /// Whether the marker exists, read INSIDE the caller's transaction (used for the
    /// re-check after acquiring the advisory lock: another settler may have
    /// completed the epoch while we were computing).
    pub async fn is_settled_tx(conn: &mut PgConnection, epoch_k: i64) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM epoch_settlements WHERE epoch_k = $1) AS "exists!""#,
            epoch_k
        )
        .fetch_one(conn)
        .await
    }

    /// Record the settlement marker within the caller's transaction, so it commits
    /// atomically with the mint. Returns `true` if THIS call recorded it.
    pub async fn claim_epoch_tx(
        conn: &mut PgConnection,
        epoch_k: i64,
        emission: i64,
        user_count: i32,
    ) -> Result<bool, sqlx::Error> {
        let res = sqlx::query!(
            r#"
            INSERT INTO epoch_settlements (epoch_k, emission, user_count)
            VALUES ($1, $2, $3)
            ON CONFLICT (epoch_k) DO NOTHING
            "#,
            epoch_k,
            emission,
            user_count
        )
        .execute(conn)
        .await?;
        Ok(res.rows_affected() == 1)
    }
}
