//! The `comments` domain — text comments on a post. The interaction graph already
//! captures the comment EDGE (for gem weighting); this stores the TEXT. Built on the
//! usual split: handler → service → repository.

pub mod handler;
pub mod model;
pub mod repository;
pub mod service;

pub use service::CommentService;
