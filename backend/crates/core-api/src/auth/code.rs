//! Pure email-code policy: how a one-time code is generated and the fixed
//! lifetime / attempt / cooldown knobs. No I/O and no clock beyond the CSPRNG
//! draw, so the numbers live in one place and stay unit-testable — the service
//! and repository stay thin (mirrors `throttle.rs`).

use std::time::Duration;

use rand::Rng;

/// A one-time code is valid for this long after issuance.
pub const CODE_TTL: Duration = Duration::from_secs(10 * 60);

/// Wrong-code guesses allowed before the code is burned (forcing a re-request).
/// Bounds online guessing of the 6-digit space to a negligible success chance.
pub const MAX_ATTEMPTS: i32 = 5;

/// Minimum seconds between two code requests for the same (email, purpose).
/// Bounds email-bombing a known address.
pub const REQUEST_COOLDOWN: Duration = Duration::from_secs(60);

/// A fresh uniformly-random 6-digit code, zero-padded (`"000000"`..="999999").
/// Drawn from the OS CSPRNG — codes must not be guessable from prior ones.
pub fn generate_code() -> String {
    let n: u32 = rand::rngs::OsRng.gen_range(0..1_000_000);
    format!("{n:06}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_is_six_digits() {
        for _ in 0..1000 {
            let c = generate_code();
            assert_eq!(c.len(), 6, "code {c} must be 6 chars");
            assert!(
                c.chars().all(|ch| ch.is_ascii_digit()),
                "code {c} must be digits"
            );
        }
    }
}
