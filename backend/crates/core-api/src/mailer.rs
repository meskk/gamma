//! Outbound-email seam. Phase 1a has no real mail provider, so the default
//! [`LogMailer`] does NOT send anything — it records the code in the server log,
//! where a local/beta operator can read it. A real provider (SMTP or a
//! transactional API) is a later, drop-in impl behind this same trait; the auth
//! flows never change. Deliberately provider-agnostic, like the `LedgerBackend`
//! seam — the whole point is that swapping the backing doesn't ripple outward.

use std::fmt;

use crate::auth::model::CodePurpose;

/// A mail send failed. The message is for logs only — it must never be surfaced
/// to a client (that would leak account existence on the request-code path).
#[derive(Debug)]
pub struct MailError(pub String);

impl fmt::Display for MailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for MailError {}

/// The mail seam. One method for now — the only outbound mail the product sends.
pub trait Mailer: Send + Sync {
    /// Deliver a one-time `code` to `to` for the given `purpose`.
    fn send_code(&self, to: &str, purpose: CodePurpose, code: &str) -> Result<(), MailError>;
}

/// Dev/beta mailer: performs NO real send — it logs the code so a local operator
/// can read it from the server output.
///
/// DEV ONLY. A real `Mailer` implementation must never log the plaintext code.
pub struct LogMailer;

impl Mailer for LogMailer {
    fn send_code(&self, to: &str, purpose: CodePurpose, code: &str) -> Result<(), MailError> {
        // Fail-safer default: the plaintext code is logged ONLY in debug builds
        // (local dev / the designer setup). A release build that still has the
        // default LogMailer wired — i.e. production forgot to inject a real
        // provider — suppresses the code and warns loudly, so a config slip can't
        // leak login/reset codes into prod logs.
        if cfg!(debug_assertions) {
            tracing::info!(
                target: "mailer",
                purpose = %purpose,
                to = %to,
                code = %code,
                "DEV mailer: email code (no real send — configure a real Mailer for production)"
            );
        } else {
            tracing::warn!(
                target: "mailer",
                purpose = %purpose,
                to = %to,
                "LogMailer active in a RELEASE build — email code suppressed from logs; \
                 configure a real Mailer for production"
            );
        }
        Ok(())
    }
}
