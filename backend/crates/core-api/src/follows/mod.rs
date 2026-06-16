//! The `follows` domain — the social graph edges. Same template as users/posts
//! (model / repository / service / handler), though the entity is a relationship
//! (follower → followee) rather than a document.
//!
//! These edges feed the feed candidate set (Dossier Appendix A.2) and, later, the
//! Gamma interaction graph. Writing follow events into `interaction_events` is a
//! separate (interaction-capture) step.

pub mod handler;
pub mod model;
pub mod repository;
pub mod service;

pub use service::FollowService;
