//! Opaque feed-pagination cursor (MASTERPLAN B1).
//!
//! The cold-start ranker's score is TIME-DEPENDENT (recency decay), so naive
//! keyset pagination over `(score, id)` is unstable — every request re-scores
//! with a fresh clock and items drift across page boundaries. The cursor
//! therefore freezes the RANKING TIME on page one; every later page re-runs
//! the same candidate query but scores with the frozen clock, making the order
//! reproducible: no duplicates, no gaps for items present at freeze time. New
//! posts appear on the next refresh (fresh cursor), standard feed semantics.
//!
//! Encoding: `v1.<ranked_at>.<score_bits>.<last_id>` hex-wrapped — opaque to
//! clients, versioned for us. `score_bits` is the exact `f64::to_bits` of the
//! last served item's score so the keyset comparison is bit-exact.

/// A decoded cursor: the frozen ranking time (unix seconds — WHOLE seconds, so
/// the clock survives the round-trip exactly) and the keyset position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedCursor {
    pub ranked_at: i64,
    pub score_bits: u64,
    pub last_id: i64,
}

pub fn encode(c: &FeedCursor) -> String {
    hex::encode(format!("v1.{}.{}.{}", c.ranked_at, c.score_bits, c.last_id))
}

/// `None` for anything that is not a well-formed v1 cursor — the caller maps
/// that to a 400 `invalid_cursor`.
pub fn decode(s: &str) -> Option<FeedCursor> {
    let bytes = hex::decode(s).ok()?;
    let text = String::from_utf8(bytes).ok()?;
    let mut parts = text.split('.');
    if parts.next()? != "v1" {
        return None;
    }
    let cursor = FeedCursor {
        ranked_at: parts.next()?.parse().ok()?,
        score_bits: parts.next()?.parse().ok()?,
        last_id: parts.next()?.parse().ok()?,
    };
    if parts.next().is_some() {
        return None;
    }
    Some(cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_exactly() {
        let c = FeedCursor {
            ranked_at: 1_752_000_000,
            score_bits: 4614256656552045848, // PI's bits — an arbitrary f64
            last_id: 42,
        };
        assert_eq!(decode(&encode(&c)), Some(c));
    }

    #[test]
    fn rejects_garbage_wrong_version_and_trailing_parts() {
        assert_eq!(decode("not-hex!"), None);
        assert_eq!(decode(&hex::encode("v2.1.2.3")), None);
        assert_eq!(decode(&hex::encode("v1.1.2")), None);
        assert_eq!(decode(&hex::encode("v1.1.2.3.4")), None);
        assert_eq!(decode(&hex::encode("v1.x.2.3")), None);
    }
}
