//! Shared application state injected into every handler.
//!
//! Holds the connection pool (for readiness + future raw access) and one service
//! per domain. All fields are cheap to clone (`PgPool` is an Arc internally), so
//! axum can clone the state per request.

use crate::feed::FeedService;
use crate::follows::FollowService;
use crate::gems::SettlementService;
use crate::interactions::InteractionService;
use crate::media::MediaService;
use crate::posts::PostService;
use crate::queue::TranscodeQueue;
use crate::users::UserService;
use db::PgPool;
use storage::{Storage, StorageConfig};

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub users: UserService,
    pub posts: PostService,
    pub follows: FollowService,
    pub feed: FeedService,
    pub interactions: InteractionService,
    pub gems: SettlementService,
    pub media: MediaService,
    /// Exposed so the binary can ensure the bucket exists on startup.
    pub storage: Storage,
}

impl AppState {
    pub fn new(pool: PgPool) -> Self {
        let storage = Storage::new(StorageConfig::from_env());
        let users = UserService::new(pool.clone());
        let posts = PostService::new(pool.clone());
        let follows = FollowService::new(pool.clone());
        let feed = FeedService::new(pool.clone());
        let interactions = InteractionService::new(pool.clone());
        let gems = SettlementService::new(pool.clone());
        let redis_url =
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());
        let queue = TranscodeQueue::new(&redis_url).expect("valid REDIS_URL");
        let media = MediaService::new(pool.clone(), storage.clone(), queue);
        Self {
            pool,
            users,
            posts,
            follows,
            feed,
            interactions,
            gems,
            media,
            storage,
        }
    }
}
