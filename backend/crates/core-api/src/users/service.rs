//! User business logic. Thin today, but it owns the rules that are neither HTTP
//! concerns (handler) nor persistence (repository) — e.g. normalising declared
//! categories so the same topic isn't stored as "Tech", "tech ", and "TECH".

use std::collections::HashSet;

use crate::error::ApiError;
use crate::users::model::{NewUser, ReferralTerms, User};
use crate::users::repository::UserRepository;
use db::PgPool;

#[derive(Clone)]
pub struct UserService {
    repo: UserRepository,
}

impl UserService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            repo: UserRepository::new(pool),
        }
    }

    pub async fn create(&self, mut new: NewUser) -> Result<User, ApiError> {
        new.declared_categories = normalize_categories(new.declared_categories);
        Ok(self.repo.create(&new).await?)
    }

    pub async fn get(&self, id: i64) -> Result<User, ApiError> {
        self.repo.get(id).await?.ok_or(ApiError::NotFound)
    }

    /// Set a user's bot-gate (verified) flag. Operator-only at the HTTP layer.
    /// 404 if the user does not exist.
    pub async fn set_verification(&self, id: i64, verified: bool) -> Result<User, ApiError> {
        self.repo
            .set_bot_gate(id, verified)
            .await?
            .ok_or(ApiError::NotFound)
    }

    /// Upsert a creator's referral contract (P-2). Operator-only at the HTTP
    /// layer; the operator id is logged as the audit trail alongside the row's
    /// own updated_at. Applies to referrals recruited FROM NOW ON — existing
    /// referrals keep their frozen terms.
    pub async fn set_referral_terms(
        &self,
        operator_id: i64,
        referrer_id: i64,
        bps: i32,
        duration_epochs: i64,
        note: Option<&str>,
    ) -> Result<ReferralTerms, ApiError> {
        if !(0..=10_000).contains(&bps) {
            return Err(ApiError::Validation("invalid_bps"));
        }
        if duration_epochs < 0 {
            return Err(ApiError::Validation("invalid_duration"));
        }
        let terms = self
            .repo
            .upsert_referral_terms(referrer_id, bps, duration_epochs, note)
            .await
            .map_err(|e| ApiError::on_fk_violation(e, ApiError::NotFound))?;
        tracing::info!(
            operator_id,
            referrer_id,
            bps,
            duration_epochs,
            "referral terms set"
        );
        Ok(terms)
    }
}

/// Trim, lowercase, drop empties, and de-duplicate while preserving first-seen order.
fn normalize_categories(input: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    input
        .into_iter()
        .map(|c| c.trim().to_lowercase())
        .filter(|c| !c.is_empty())
        .filter(|c| seen.insert(c.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::normalize_categories;

    #[test]
    fn normalises_and_dedupes_preserving_order() {
        let out = normalize_categories(vec![
            "Tech".into(),
            "tech ".into(),
            "".into(),
            "  ".into(),
            "Art".into(),
        ]);
        assert_eq!(out, vec!["tech".to_string(), "art".to_string()]);
    }
}
