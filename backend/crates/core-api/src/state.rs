//! Shared application state injected into every handler.
//!
//! Holds the connection pool (for readiness + future raw access) and one service
//! per domain. All fields are cheap to clone (`PgPool` is an Arc internally), so
//! axum can clone the state per request.

use crate::auth::AuthService;
use crate::feed::FeedService;
use crate::follows::FollowService;
use crate::gems::SettlementService;
use crate::interactions::InteractionService;
use crate::media::MediaService;
use crate::posts::PostService;
use crate::queue::{IngestionQueue, TranscodeQueue};
use crate::signals::SignalService;
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
    pub auth: AuthService,
    pub signals: SignalService,
    /// Exposed so the binary can ensure the bucket exists on startup.
    pub storage: Storage,
}

impl AppState {
    pub fn new(pool: PgPool) -> Self {
        let storage = Storage::new(StorageConfig::from_env());
        let redis_url =
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());
        // Loaded once from config (or defaults) and threaded into the services that
        // spend/mint, so the running app uses the configured knobs — not an inline
        // default (ADR 0003).
        let econ = crate::load_econ_params();
        let users = UserService::new(pool.clone());
        let ingestion = IngestionQueue::new(&redis_url).expect("valid REDIS_URL");
        let posts = PostService::with_ingestion(pool.clone(), ingestion);
        let follows = FollowService::new(pool.clone());
        let feed = FeedService::new(pool.clone());
        let interactions = InteractionService::new(pool.clone());
        let gems = SettlementService::with_econ(pool.clone(), econ.clone());
        let queue = TranscodeQueue::new(&redis_url).expect("valid REDIS_URL");
        let media = MediaService::with_econ(pool.clone(), storage.clone(), queue, econ);
        let auth = AuthService::new(pool.clone());
        let signals = SignalService::new(pool.clone());
        Self {
            pool,
            users,
            posts,
            follows,
            feed,
            interactions,
            gems,
            media,
            auth,
            signals,
            storage,
        }
    }
}
