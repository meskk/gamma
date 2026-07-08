//! Content-signal business logic: enforce the versioned signal contract
//! (ADR 0009) on write and map a write-back for an unknown post to a client
//! error.
//!
//! `schema_version` semantics: 0 = legacy free-form (pre-ADR writers; accepted
//! unvalidated, ignored by every consumer), 1 = the typed core below, anything
//! newer than this API knows = rejected (fail closed — deploy the API before
//! the analyzer that speaks the new version).

use serde_json::{Map, Value};

use crate::error::ApiError;
use crate::signals::model::{ContentSignal, SignalWriteback};
use crate::signals::repository::ContentSignalRepository;
use db::PgPool;

/// Caps that keep a v1 row sane; generous against any real analyzer output.
const MAX_TOPICS: usize = 16;
const MAX_TOPIC_LEN: usize = 64;
const MAX_LANGUAGE_LEN: usize = 35; // longest legal BCP-47 primary form
/// Hard ceiling on embedding dimensionality (ADR 0009 §3); typical text
/// encoders are 384–1536, so 4096 is headroom, not a target.
const MAX_EMBEDDING_DIM: usize = 4096;

#[derive(Clone)]
pub struct SignalService {
    repo: ContentSignalRepository,
}

impl SignalService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            repo: ContentSignalRepository::new(pool),
        }
    }

    /// Record the pipeline's signals (and optional embedding) for a post.
    pub async fn record(&self, post_id: i64, body: SignalWriteback) -> Result<(), ApiError> {
        let model_version = body.model_version.trim();
        if model_version.is_empty() {
            return Err(ApiError::Validation("empty_model_version"));
        }
        match body.schema_version {
            0 => {} // legacy free-form: accepted verbatim, consumed by nobody
            1 => validate_v1(&body.signals)?,
            _ => return Err(ApiError::Validation("unknown_schema_version")),
        }
        let embedding = match &body.embedding {
            Some(e) => {
                validate_embedding(e)?;
                Some(e.as_slice())
            }
            None => None,
        };
        self.repo
            .upsert(
                post_id,
                model_version,
                body.schema_version,
                &body.signals,
                embedding,
            )
            .await
            .map_err(map_fk)
    }

    /// The stored signals for a post, or 404 if none yet.
    pub async fn get(&self, post_id: i64) -> Result<ContentSignal, ApiError> {
        self.repo.get(post_id).await?.ok_or(ApiError::NotFound)
    }
}

/// The typed v1 core (ADR 0009 §2). Every field is optional, but a present
/// field must have the right type and range, and unknown top-level keys are
/// rejected — additions go through the `extras` annex or a schema bump.
/// JSON `null` counts as absent, so Python writers can send `None` freely.
fn validate_v1(signals: &Value) -> Result<(), ApiError> {
    let obj: &Map<String, Value> = signals
        .as_object()
        .ok_or(ApiError::Validation("signals_not_an_object"))?;

    for (key, value) in obj {
        match key.as_str() {
            "quality" => validate_unit_score(value, "invalid_quality")?,
            "bot_likelihood" => validate_unit_score(value, "invalid_bot_likelihood")?,
            "nsfw_likelihood" => validate_unit_score(value, "invalid_nsfw_likelihood")?,
            "topics" => validate_topics(value)?,
            "language" => validate_language(value)?,
            "extras" => {
                if !(value.is_null() || value.is_object()) {
                    return Err(ApiError::Validation("invalid_extras"));
                }
            }
            _ => return Err(ApiError::Validation("unknown_signal_field")),
        }
    }
    Ok(())
}

/// A score in [0, 1]. JSON numbers can't be NaN/Inf, so range is the whole check.
fn validate_unit_score(value: &Value, code: &'static str) -> Result<(), ApiError> {
    if value.is_null() {
        return Ok(());
    }
    match value.as_f64() {
        Some(x) if (0.0..=1.0).contains(&x) => Ok(()),
        _ => Err(ApiError::Validation(code)),
    }
}

/// Topic values share the app's category namespace (ADR 0009 §2: the owner
/// chose the app's own category set over a separate taxonomy). There is no
/// closed list — categories are user-declared — so the API enforces the
/// NAMESPACE rules (already-normalized per `users::normalize_categories`
/// semantics: trimmed, lowercase, non-empty, no duplicates); the label SPACE
/// an analyzer emits is that analyzer's contract.
fn validate_topics(value: &Value) -> Result<(), ApiError> {
    if value.is_null() {
        return Ok(());
    }
    let items = value
        .as_array()
        .ok_or(ApiError::Validation("invalid_topics"))?;
    if items.len() > MAX_TOPICS {
        return Err(ApiError::Validation("invalid_topics"));
    }
    let mut seen = std::collections::HashSet::new();
    for item in items {
        let s = item
            .as_str()
            .ok_or(ApiError::Validation("invalid_topics"))?;
        let normalized_form = s.trim().to_lowercase();
        if s.is_empty() || s.len() > MAX_TOPIC_LEN || s != normalized_form || !seen.insert(s) {
            return Err(ApiError::Validation("invalid_topics"));
        }
    }
    Ok(())
}

/// A loose BCP-47 primary tag: lowercase ASCII letters/digits/hyphens, e.g.
/// "de", "en", "zh-hant". Deliberately not a full BCP-47 parser.
fn validate_language(value: &Value) -> Result<(), ApiError> {
    if value.is_null() {
        return Ok(());
    }
    let s = value
        .as_str()
        .ok_or(ApiError::Validation("invalid_language"))?;
    let well_formed = (2..=MAX_LANGUAGE_LEN).contains(&s.len())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !s.starts_with('-')
        && !s.ends_with('-');
    if well_formed {
        Ok(())
    } else {
        Err(ApiError::Validation("invalid_language"))
    }
}

/// Embeddings: non-empty, bounded, finite. serde already rejects NaN/Inf
/// LITERALS, but an f64 beyond f32 range casts to ±inf — check finiteness.
fn validate_embedding(embedding: &[f32]) -> Result<(), ApiError> {
    if embedding.is_empty()
        || embedding.len() > MAX_EMBEDDING_DIM
        || embedding.iter().any(|x| !x.is_finite())
    {
        return Err(ApiError::Validation("invalid_embedding"));
    }
    Ok(())
}

/// A write-back for a non-existent post hits the FK — a client error, not a fault.
fn map_fk(err: sqlx::Error) -> ApiError {
    if let Some(db_err) = err.as_database_error() {
        if matches!(db_err.kind(), sqlx::error::ErrorKind::ForeignKeyViolation) {
            return ApiError::Validation("unknown_post");
        }
    }
    ApiError::Database(err)
}
