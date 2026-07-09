//! P-4/A2 data-layer tests: area configuration, entitlement semantics
//! (permanent vs. expiring — subscriptions revoke by lapse, no cron; grants
//! only ever EXTEND, so out-of-order webhooks can neither rewind an expiry
//! nor destroy a permanent purchase), the purchases audit mirror's
//! idempotency anchor, and the schema's own guards.

use chrono::{Duration, SubsecRound, Utc};
use core_api::private_area::model::{AccessModel, EntitlementSource, NewPurchase};
use core_api::private_area::repository::PrivateAreaRepository;
use core_api::users::model::NewUser;
use core_api::users::repository::UserRepository;
use sqlx::PgPool;

async fn seed_user(pool: &PgPool) -> i64 {
    UserRepository::new(pool.clone())
        .create(&NewUser {
            declared_categories: vec![],
            bot_gate_v: false,
        })
        .await
        .expect("user")
        .id
}

#[sqlx::test(migrations = "../../migrations")]
async fn area_config_upserts_one_row_per_creator(pool: PgPool) {
    let repo = PrivateAreaRepository::new(pool.clone());
    let creator = seed_user(&pool).await;

    // No configuration until the creator sets one up.
    assert!(repo.get_area(creator).await.unwrap().is_none());

    let area = repo
        .upsert_area(creator, AccessModel::OneTime, 500, "Mein privater Bereich")
        .await
        .unwrap();
    assert_eq!(area.access_model, "one_time");
    assert_eq!(area.price_cents, 500);
    assert_eq!(area.currency, "EUR");

    // The creator changes their mind: same row, new model (owner decision —
    // the CREATOR picks the access model).
    let area = repo
        .upsert_area(creator, AccessModel::Free, 0, "jetzt offen")
        .await
        .unwrap();
    assert_eq!(area.access_model, "free");
    assert_eq!(area.price_cents, 0);
    let stored = repo.get_area(creator).await.unwrap().unwrap();
    assert_eq!(stored.description, "jetzt offen");
}

async fn stored_expiry(pool: &PgPool, viewer: i64, creator: i64) -> Option<chrono::DateTime<Utc>> {
    sqlx::query_scalar!(
        r#"SELECT expires_at FROM area_entitlements WHERE viewer_id = $1 AND creator_id = $2"#,
        viewer,
        creator
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn entitlements_expire_by_lapse_and_grants_only_ever_extend(pool: PgPool) {
    let repo = PrivateAreaRepository::new(pool.clone());
    let creator = seed_user(&pool).await;
    let viewer = seed_user(&pool).await;

    // The creator is always entitled to their own area; a stranger is not.
    assert!(repo.is_entitled(creator, creator).await.unwrap());
    assert!(!repo.is_entitled(viewer, creator).await.unwrap());

    // A subscription that already lapsed answers false — no cleanup job, the
    // expiry itself revokes.
    let expired = Utc::now() - Duration::hours(1);
    repo.grant_entitlement(
        viewer,
        creator,
        EntitlementSource::Subscription,
        Some(expired),
    )
    .await
    .unwrap();
    assert!(!repo.is_entitled(viewer, creator).await.unwrap());

    // invoice.paid semantics: the re-grant pushes the expiry forward and
    // access is live again. (Truncated to microseconds — TIMESTAMPTZ
    // resolution — so the roundtrip equality below is exact.)
    let renewed = (Utc::now() + Duration::days(30)).trunc_subsecs(6);
    repo.grant_entitlement(
        viewer,
        creator,
        EntitlementSource::Subscription,
        Some(renewed),
    )
    .await
    .unwrap();
    assert!(repo.is_entitled(viewer, creator).await.unwrap());

    // A stale, out-of-order event (Stripe retries for days, unordered) can
    // NOT rewind the expiry — the grant is monotone.
    let stale = Utc::now() + Duration::days(3);
    repo.grant_entitlement(
        viewer,
        creator,
        EntitlementSource::Subscription,
        Some(stale),
    )
    .await
    .unwrap();
    assert_eq!(stored_expiry(&pool, viewer, creator).await, Some(renewed));

    // A one-time purchase upgrades to PERMANENT access (expires_at NULL)...
    repo.grant_entitlement(viewer, creator, EntitlementSource::Purchase, None)
        .await
        .unwrap();
    assert!(repo.is_entitled(viewer, creator).await.unwrap());
    assert_eq!(stored_expiry(&pool, viewer, creator).await, None);

    // ...and NO later subscription event may destroy it: paid-forever stays
    // forever (owner decision: Einmalpreis = dauerhafter Zugang). Deliberate
    // revocation becomes its own explicit path in A6, never a grant side
    // effect.
    repo.grant_entitlement(
        viewer,
        creator,
        EntitlementSource::Subscription,
        Some(expired),
    )
    .await
    .unwrap();
    assert!(repo.is_entitled(viewer, creator).await.unwrap());
    assert_eq!(stored_expiry(&pool, viewer, creator).await, None);
}

#[sqlx::test(migrations = "../../migrations")]
async fn purchases_are_idempotent_on_the_provider_ref(pool: PgPool) {
    let repo = PrivateAreaRepository::new(pool.clone());
    let creator = seed_user(&pool).await;
    let viewer = seed_user(&pool).await;

    let purchase = NewPurchase {
        provider: "stripe",
        provider_ref: "cs_test_123",
        viewer_id: viewer,
        creator_id: creator,
        kind: "one_time",
        amount_cents: 500,
        currency: "EUR",
        fee_cents: 50,
        status: "paid",
    };

    // First delivery records; the webhook REPLAY records nothing twice — the
    // caller uses this signal to skip side effects too.
    assert!(repo.record_purchase(purchase.clone()).await.unwrap());
    assert!(!repo.record_purchase(purchase.clone()).await.unwrap());

    // The anchor is the COMPOSITE (provider, provider_ref): the same ref under
    // a DIFFERENT provider (the decided stage-2 wallet path) is a legitimate
    // new purchase, not a replay — narrowing the unique scope to provider_ref
    // alone would silently drop paying users' purchases.
    let other_provider = NewPurchase {
        provider: "other",
        ..purchase
    };
    assert!(repo.record_purchase(other_provider).await.unwrap());

    let count: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "count!" FROM purchases WHERE provider_ref = 'cs_test_123'"#
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 2);
}

#[sqlx::test(migrations = "../../migrations")]
async fn schema_guards_hold(pool: PgPool) {
    let creator = seed_user(&pool).await;

    // Every existing and new post lives in 'public' until A4 wires the
    // private write path — nothing changes for the current product.
    let area: String = sqlx::query_scalar!(
        r#"
        INSERT INTO posts (author_id, body) VALUES ($1, 'hello')
        RETURNING area AS "area!"
        "#,
        creator
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(area, "public");

    // The DB itself rejects vocabulary outside the contract (fail closed
    // even if a service-level check were forgotten).
    for (table, sql) in [
        (
            "posts.area",
            format!("INSERT INTO posts (author_id, body, area) VALUES ({creator}, 'x', 'secret')"),
        ),
        (
            "private_areas.access_model",
            format!(
                "INSERT INTO private_areas (creator_id, access_model) VALUES ({creator}, 'vip')"
            ),
        ),
        (
            "private_areas.price_cents",
            format!("INSERT INTO private_areas (creator_id, price_cents) VALUES ({creator}, -1)"),
        ),
        (
            "area_entitlements.source",
            format!(
                "INSERT INTO area_entitlements (viewer_id, creator_id, source) \
                 VALUES ({creator}, {creator}, 'gift')"
            ),
        ),
        // purchases is where unexpected provider-side values will land (A6):
        // the schema backstop matters most exactly here.
        (
            "purchases.kind",
            format!(
                "INSERT INTO purchases (provider, provider_ref, viewer_id, creator_id, kind, \
                 amount_cents, currency, fee_cents, status) \
                 VALUES ('stripe', 'g1', {creator}, {creator}, 'gift', 100, 'EUR', 10, 'paid')"
            ),
        ),
        (
            "purchases.amount_cents",
            format!(
                "INSERT INTO purchases (provider, provider_ref, viewer_id, creator_id, kind, \
                 amount_cents, currency, fee_cents, status) \
                 VALUES ('stripe', 'g2', {creator}, {creator}, 'one_time', -1, 'EUR', 0, 'paid')"
            ),
        ),
        (
            "purchases.fee_cents",
            format!(
                "INSERT INTO purchases (provider, provider_ref, viewer_id, creator_id, kind, \
                 amount_cents, currency, fee_cents, status) \
                 VALUES ('stripe', 'g3', {creator}, {creator}, 'one_time', 100, 'EUR', -1, 'paid')"
            ),
        ),
    ] {
        let err = sqlx::query(&sql).execute(&pool).await;
        assert!(err.is_err(), "{table} accepted invalid input");
    }

    // AccessModel::parse mirrors the CHECK vocabulary exactly.
    for model in [
        AccessModel::Free,
        AccessModel::OneTime,
        AccessModel::Subscription,
        AccessModel::PerPost,
    ] {
        assert_eq!(AccessModel::parse(model.as_str()), Some(model));
    }
    assert_eq!(AccessModel::parse("vip"), None);
}
