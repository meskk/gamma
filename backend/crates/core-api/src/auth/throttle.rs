//! Pure login-throttle policy: how long an email is locked after N consecutive
//! failed logins. A pure function of the failure count — no I/O, no clock —
//! so the curve is unit-testable and the repository/service stay thin.

use std::time::Duration;

/// Failures 1..=FREE_FAILURES trigger no lock; the per-IP bucket paces them.
const FREE_FAILURES: u32 = 4;
/// Lock after the first over-limit failure (failure number FREE_FAILURES + 1).
const FIRST_LOCK_SECS: u64 = 60;
/// Locks double per further failure, capped here. The cap bounds griefing (an
/// attacker hammering a victim's email) to 15-minute windows — there is
/// deliberately NO permanent lockout.
const MAX_LOCK_SECS: u64 = 15 * 60;

/// How long the account is locked after `failed_count` consecutive failures,
/// or `None` while the count is still in the free band.
pub fn lock_duration(failed_count: u32) -> Option<Duration> {
    let over = failed_count.checked_sub(FREE_FAILURES + 1)?;
    // 2^over with saturation: from 32 doublings on we are far past the cap.
    let secs = if over >= 32 {
        MAX_LOCK_SECS
    } else {
        (FIRST_LOCK_SECS << over).min(MAX_LOCK_SECS)
    };
    Some(Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_band_has_no_lock() {
        for n in 0..=4 {
            assert_eq!(lock_duration(n), None, "failure {n} must not lock");
        }
    }

    #[test]
    fn curve_doubles_from_60s_and_caps_at_15_min() {
        assert_eq!(lock_duration(5), Some(Duration::from_secs(60)));
        assert_eq!(lock_duration(6), Some(Duration::from_secs(120)));
        assert_eq!(lock_duration(7), Some(Duration::from_secs(240)));
        assert_eq!(lock_duration(8), Some(Duration::from_secs(480)));
        // 960 would exceed the cap:
        assert_eq!(lock_duration(9), Some(Duration::from_secs(900)));
        assert_eq!(lock_duration(100), Some(Duration::from_secs(900)));
        assert_eq!(lock_duration(u32::MAX), Some(Duration::from_secs(900)));
    }
}
