//! Per-message-group operational state: Running, Paused or Blocked (Go
//! `GroupStateManager`, `flowcatalyst-go/internal/outbox/group_state.go`).
//!
//! A group is Blocked when one of its items failed for good (a terminal
//! status, or retries exhausted) while block-on-error is on: the group then
//! never advances past that item until an operator unblocks it (the item is
//! re-queued for a fresh attempt) or skips it (the item stays failed). A
//! Paused or Blocked group's claimed items are released back to PENDING each
//! poll instead of being sent. Groups not listed are Running.

use fc_common::OutboxItemType;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::RwLock;

/// A group's processing state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GroupStatus {
    Running,
    Paused,
    Blocked,
}

/// A snapshot of one group's state (Go `GroupInfo`, same JSON).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupInfo {
    pub group: String,
    pub status: GroupStatus,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub blocked_item_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
}

/// The item a group is blocked on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedItem {
    pub id: String,
    pub item_type: OutboxItemType,
}

#[derive(Debug, Clone)]
struct GroupState {
    status: GroupStatus,
    blocked: Option<BlockedItem>,
    error: String,
}

/// Holds the non-Running groups. Safe for concurrent use.
#[derive(Debug, Default)]
pub struct GroupStateManager {
    groups: RwLock<HashMap<String, GroupState>>,
}

impl GroupStateManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the group may send now (Running).
    pub fn is_active(&self, group: &str) -> bool {
        self.groups
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(group)
            .is_none_or(|g| g.status == GroupStatus::Running)
    }

    /// Blocks the group on a poison item until unblocked or skipped.
    pub fn block(&self, group: &str, item: BlockedItem, error: &str) {
        self.groups
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                group.to_string(),
                GroupState {
                    status: GroupStatus::Blocked,
                    blocked: Some(item),
                    error: error.to_string(),
                },
            );
    }

    /// Running → Paused (no-op when Blocked or already Paused).
    pub fn pause(&self, group: &str) {
        let mut groups = self.groups.write().unwrap_or_else(|e| e.into_inner());
        groups.entry(group.to_string()).or_insert(GroupState {
            status: GroupStatus::Paused,
            blocked: None,
            error: String::new(),
        });
    }

    /// Paused → Running (no-op otherwise).
    pub fn resume(&self, group: &str) {
        let mut groups = self.groups.write().unwrap_or_else(|e| e.into_inner());
        if groups.get(group).map(|g| g.status) == Some(GroupStatus::Paused) {
            groups.remove(group);
        }
    }

    /// Blocked → Running, returning the item it was blocked on; `None` when
    /// the group wasn't Blocked. Backs both unblock (re-queue the item) and
    /// skip (leave it failed).
    pub fn clear_block(&self, group: &str) -> Option<BlockedItem> {
        let mut groups = self.groups.write().unwrap_or_else(|e| e.into_inner());
        if groups.get(group).map(|g| g.status) != Some(GroupStatus::Blocked) {
            return None;
        }
        groups.remove(group).and_then(|g| g.blocked)
    }

    /// Every non-Running group.
    pub fn snapshot(&self) -> Vec<GroupInfo> {
        let groups = self.groups.read().unwrap_or_else(|e| e.into_inner());
        let mut out: Vec<GroupInfo> = groups
            .iter()
            .map(|(group, s)| GroupInfo {
                group: group.clone(),
                status: s.status,
                blocked_item_id: s.blocked.as_ref().map(|b| b.id.clone()).unwrap_or_default(),
                error: s.error.clone(),
            })
            .collect();
        out.sort_by(|a, b| a.group.cmp(&b.group));
        out
    }

    /// Only the Blocked groups.
    pub fn blocked(&self) -> Vec<GroupInfo> {
        self.snapshot()
            .into_iter()
            .filter(|g| g.status == GroupStatus::Blocked)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> BlockedItem {
        BlockedItem {
            id: id.into(),
            item_type: OutboxItemType::Event,
        }
    }

    #[test]
    fn transitions_as_go() {
        let m = GroupStateManager::new();
        assert!(m.is_active("g"));

        m.pause("g");
        assert!(!m.is_active("g"));
        assert_eq!(m.snapshot()[0].status, GroupStatus::Paused);
        m.resume("g");
        assert!(m.is_active("g"));
        assert!(m.snapshot().is_empty());

        m.block("g", item("i1"), "boom");
        assert!(!m.is_active("g"));
        // Pause and resume don't touch a Blocked group.
        m.pause("g");
        m.resume("g");
        assert_eq!(m.blocked().len(), 1);
        assert_eq!(m.blocked()[0].blocked_item_id, "i1");
        assert_eq!(m.blocked()[0].error, "boom");

        assert_eq!(m.clear_block("g"), Some(item("i1")));
        assert!(m.is_active("g"));
        assert_eq!(m.clear_block("g"), None);

        m.pause("p");
        assert_eq!(m.clear_block("p"), None, "a Paused group is not Blocked");
    }

    #[test]
    fn serialises_as_go() {
        let info = GroupInfo {
            group: "g".into(),
            status: GroupStatus::Blocked,
            blocked_item_id: "i".into(),
            error: String::new(),
        };
        assert_eq!(
            serde_json::to_value(&info).unwrap(),
            serde_json::json!({"group": "g", "status": "BLOCKED", "blockedItemId": "i"})
        );
    }
}
