//! WebSocket notification types for real-time event broadcasting.
//!
//! When a promotion occurs on the server, a [`Notification`] is broadcast
//! to all connected WebSocket clients. The notification contains detailed
//! information about the promotion: which changesets were promoted, from
//! which channel, to which channel, and the affected resources.
//!
//! ## Wire Format
//!
//! Notifications are serialized as JSON and sent as WebSocket text frames.
//!
//! ```json
//! {
//!   "kind": "promotion",
//!   "timestamp": "2026-03-01T12:00:00Z",
//!   "payload": {
//!     "source_channel": "feature-users",
//!     "target_channel": "main",
//!     "promoted_changesets": [
//!       {
//!         "change_id": "a7f3bc12",
//!         "message": "Add user management",
//!         "author": "alice",
//!         "patch_count": 3,
//!         "affected_resources": ["acme.entity.User", "acme.entity.Role"]
//!       }
//!     ],
//!     "new_head": "a7f3bc12",
//!     "total_resources_affected": 2
//!   }
//! }
//! ```

use serde::{Deserialize, Serialize};

/// A notification event broadcast over WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    /// The kind of notification event.
    pub kind: NotificationKind,
    /// ISO 8601 timestamp of when the event occurred.
    pub timestamp: String,
    /// The detailed payload of the notification.
    pub payload: NotificationPayload,
}

/// The kind of notification event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    /// A promotion from one channel to another completed successfully.
    Promotion,
    /// A push to a channel completed successfully.
    Push,
}

/// The payload of a notification, varying by kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NotificationPayload {
    /// Payload for a promotion notification.
    Promotion(PromotionPayload),
    /// Payload for a push notification.
    Push(PushPayload),
}

/// Detailed information about a promotion event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionPayload {
    /// The source channel from which changesets were promoted.
    pub source_channel: String,
    /// The target channel to which changesets were promoted.
    pub target_channel: String,
    /// Details of each promoted changeset.
    pub promoted_changesets: Vec<PromotedChangesetInfo>,
    /// The new head change_id of the target channel.
    pub new_head: Option<String>,
    /// Total number of unique resources affected across all promoted changesets.
    pub total_resources_affected: usize,
}

/// Summary information about a single promoted changeset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotedChangesetInfo {
    /// The changeset's unique change_id.
    pub change_id: String,
    /// The commit message.
    pub message: String,
    /// The author of the changeset.
    pub author: String,
    /// Number of patches in the changeset.
    pub patch_count: usize,
    /// Resource IDs affected by this changeset's patches.
    pub affected_resources: Vec<String>,
}

/// Detailed information about a push event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushPayload {
    /// The channel that was pushed to.
    pub channel: String,
    /// Number of changesets pushed.
    pub changeset_count: usize,
    /// The new head change_id of the channel.
    pub new_head: Option<String>,
}

impl Notification {
    /// Create a promotion notification from a completed promote operation.
    pub fn promotion(
        source_channel: String,
        target_channel: String,
        promoted_changesets: Vec<PromotedChangesetInfo>,
        new_head: Option<String>,
    ) -> Self {
        let total_resources_affected = promoted_changesets
            .iter()
            .flat_map(|cs| cs.affected_resources.iter())
            .collect::<std::collections::HashSet<_>>()
            .len();

        Self {
            kind: NotificationKind::Promotion,
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: NotificationPayload::Promotion(PromotionPayload {
                source_channel,
                target_channel,
                promoted_changesets,
                new_head,
                total_resources_affected,
            }),
        }
    }

    /// Create a push notification.
    pub fn push(channel: String, changeset_count: usize, new_head: Option<String>) -> Self {
        Self {
            kind: NotificationKind::Push,
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: NotificationPayload::Push(PushPayload {
                channel,
                changeset_count,
                new_head,
            }),
        }
    }

    /// Serialize the notification to a JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize a notification from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}
