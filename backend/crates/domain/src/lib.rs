//! Shared domain types — the seam-free middle every other crate depends on.
//!
//! All money is integer fixed-point. No floats ever touch a conserved quantity
//! (Rebuild Dossier v5, Appendix B.2). Newtypes stop you mixing up a satoshi
//! amount with a PT amount with a user id at compile time.

use serde::{Deserialize, Serialize};

/// A user account id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UserId(pub u64);

/// A post id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PostId(pub u64);

/// A daily epoch index (one epoch = one day). Boundaries cannot be reconstructed
/// retroactively — every interaction event is stamped with its epoch from day one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Epoch(pub u64);

/// Seconds in one UTC day — the epoch length (one epoch = one day).
pub const EPOCH_SECONDS: i64 = 86_400;

impl Epoch {
    /// The daily epoch containing the given Unix timestamp (seconds). Used to
    /// stamp interaction events live and to bound settlement. Kept dependency-free
    /// (takes raw seconds) so `domain` needs no time crate.
    pub fn from_unix_seconds(secs: i64) -> Epoch {
        Epoch((secs.max(0) as u64) / EPOCH_SECONDS as u64)
    }
}

/// BTC amount in satoshis. Integer — never a float.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub struct Sats(pub u64);

/// Peer Token amount in base units. 9 on-chain decimals ⇒ 1 PEER = `PT_ONE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub struct PtAmount(pub u128);

/// On-chain decimals for PT (matches SOL convention; Dossier §3).
pub const PT_DECIMALS: u32 = 9;

/// One whole PEER expressed in base units.
pub const PT_ONE: u128 = 1_000_000_000;
