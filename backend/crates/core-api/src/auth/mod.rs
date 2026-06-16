//! Authentication: registration, login, opaque bearer-token sessions, and the
//! `AuthUser` extractor that turns a `Authorization: Bearer <token>` header into
//! a verified user id.
//!
//! Auth1 (this module) provides the mechanism. Auth2 wires it into the other
//! domains so the acting identity comes from the session, not from a spoofable
//! request field — closing the current "act as any user" hole.

pub mod extract;
pub mod handler;
pub mod model;
pub mod repository;
pub mod service;

pub use extract::{AdminUser, AuthUser, Caller};
pub use service::AuthService;
