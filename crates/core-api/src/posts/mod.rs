//! The `posts` domain — built on the exact `users` template: model / repository /
//! service / handler, one responsibility each, handler → service → repository.

pub mod handler;
pub mod model;
pub mod repository;
pub mod service;

pub use service::PostService;
