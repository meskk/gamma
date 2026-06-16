//! User business logic. Thin today, but it owns the rules that are neither HTTP
//! concerns (handler) nor persistence (repository) — e.g. normalising declared
//! categories so the same topic isn't stored as "Tech", "tech ", and "TECH".

use std::collections::HashSet;

use crate::error::ApiError;
use crate::users::model::{NewUser, User};
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
