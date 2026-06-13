//! Shared application state injected into every handler.
//!
//! Holds the connection pool (for readiness + future raw access) and one service
//! per domain. All fields are cheap to clone (`PgPool` is an Arc internally), so
//! axum can clone the state per request.

use crate::feed::FeedService;
use crate::follows::FollowService;
use crate::gems::SettlementService;
use crate::interactions::InteractionService;
use crate::posts::PostService;
use crate::users::UserService;
use db::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub users: UserService,
    pub posts: PostService,
    pub follows: FollowService,
    pub feed: FeedService,
    pub interactions: InteractionService,
    pub gems: SettlementService,
}

impl AppState {
    pub fn new(pool: PgPool) -> Self {
        let users = UserService::new(pool.clone());
        let posts = PostService::new(pool.clone());
        let follows = FollowService::new(pool.clone());
        let feed = FeedService::new(pool.clone());
        let interactions = InteractionService::new(pool.clone());
        let gems = SettlementService::new(pool.clone());
        Self {
            pool,
            users,
            posts,
            follows,
            feed,
            interactions,
            gems,
        }
    }
}
