//! Response shapes for the gems/settlement endpoints.

use serde::Serialize;
use ts_rs::TS;

/// Outcome of settling (or attempting to settle) one epoch.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct SettlementSummary {
    pub epoch_k: i64,
    /// PT base units minted this epoch (0 if there were no eligible participants).
    pub emission: i64,
    /// Number of users that participated in the interaction graph this epoch.
    pub user_count: i32,
    /// True if the epoch had already been settled and this call was a no-op.
    pub already_settled: bool,
}

/// A user's current off-chain gem balance.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct GemBalance {
    pub user_id: i64,
    /// PT base units.
    pub balance: i64,
}
