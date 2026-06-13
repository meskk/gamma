//! The `media` domain — upload tickets and playback URLs for images, video and
//! audio. Same template as the other domains, plus a `storage` dependency.
//!
//! The bytes never flow through this service: a client requests an upload ticket
//! (a presigned PUT URL), uploads directly to the object store, then finalizes
//! (we confirm the object landed via a HEAD). Playback is a presigned GET URL.
//! Transcoding to adaptive HLS for long video/audio is the next step (M3).

pub mod handler;
pub mod model;
pub mod repository;
pub mod service;
mod transcode;

pub use service::MediaService;
