//! Feed response shapes. Items reuse `posts::model::Post`; the page wrapper
//! carries the opaque continuation cursor (B1).

use serde::Serialize;
use ts_rs::TS;

use crate::posts::model::Post;

/// One page of the personalized feed. `next_cursor` is `None` on the last
/// page; otherwise pass it back verbatim as `?cursor=` for the next page.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct FeedPage {
    pub items: Vec<Post>,
    pub next_cursor: Option<String>,
}
