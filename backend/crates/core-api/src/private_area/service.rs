//! Validation and orchestration for the private-area configuration (A3).
//! The service decides what a VALID configuration is; the DB CHECKs stay the
//! fail-closed backstop, never the primary gate.

use crate::error::ApiError;
use crate::private_area::model::{AccessModel, PrivateAreaConfigRequest, PrivateAreaView};
use crate::private_area::repository::PrivateAreaRepository;
use db::PgPool;

/// Longest accepted area description (bytes, like the other length caps).
const MAX_DESCRIPTION_LEN: usize = 500;
/// Price ceiling in EUR cents (€10,000) — a fat-finger sanity bound, not an
/// economic knob: it prices nothing and takes no cut (that is
/// `private_area_fee_bps`, applied at checkout creation in A5/A6).
const MAX_PRICE_CENTS: i64 = 1_000_000;

#[derive(Clone)]
pub struct PrivateAreaService {
    repo: PrivateAreaRepository,
}

impl PrivateAreaService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            repo: PrivateAreaRepository::new(pool),
        }
    }

    /// Create or update the caller's own area configuration.
    pub async fn configure(
        &self,
        creator_id: i64,
        req: PrivateAreaConfigRequest,
    ) -> Result<PrivateAreaView, ApiError> {
        let model = AccessModel::parse(&req.access_model)
            .ok_or(ApiError::Validation("unknown_access_model"))?;
        if req.price_cents < 0 {
            return Err(ApiError::Validation("negative_price"));
        }
        if req.price_cents > MAX_PRICE_CENTS {
            return Err(ApiError::Validation("price_too_high"));
        }
        // The area-level price is exactly the price of AREA access: the paid
        // area models require one, the others must not carry one (free is
        // free; per_post prices individual posts once that stage lands) — a
        // stored-but-meaningless price would render a lying offer in A7.
        match model {
            AccessModel::OneTime | AccessModel::Subscription => {
                if req.price_cents == 0 {
                    return Err(ApiError::Validation("missing_price"));
                }
            }
            AccessModel::Free | AccessModel::PerPost => {
                if req.price_cents != 0 {
                    return Err(ApiError::Validation("price_on_unpriced_model"));
                }
            }
        }
        let description = req.description.trim();
        if description.len() > MAX_DESCRIPTION_LEN {
            return Err(ApiError::Validation("description_too_long"));
        }
        let area = self
            .repo
            .upsert_area(creator_id, model, req.price_cents, description)
            .await?;
        Ok(area.into())
    }

    /// A creator's area terms — their own or anyone's (the terms are the
    /// public offer). NotFound until the creator configures the area.
    pub async fn get(&self, creator_id: i64) -> Result<PrivateAreaView, ApiError> {
        Ok(self
            .repo
            .get_area(creator_id)
            .await?
            .ok_or(ApiError::NotFound)?
            .into())
    }
}
