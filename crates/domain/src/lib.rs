//! Core Strife domain types and business rules.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Domain identifier for a file-system node.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct NodeId(Uuid);

impl NodeId {
    /// Wraps a UUID as a node identifier.
    #[must_use]
    pub const fn new(value: Uuid) -> Self {
        Self(value)
    }

    /// Returns the underlying UUID.
    #[must_use]
    pub const fn into_uuid(self) -> Uuid {
        self.0
    }
}

impl From<Uuid> for NodeId {
    fn from(value: Uuid) -> Self {
        Self::new(value)
    }
}

impl From<NodeId> for Uuid {
    fn from(value: NodeId) -> Self {
        value.into_uuid()
    }
}

/// Whether a node represents a folder or a file.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Folder,
    File,
}

/// Lifecycle visibility of a node.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LifecycleState {
    Active,
    Trashed,
    Deleted,
}

/// A domain node independent of database and HTTP representations.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Node {
    pub id: NodeId,
    pub parent_id: Option<NodeId>,
    pub name: String,
    pub kind: NodeKind,
    pub lifecycle_state: LifecycleState,
    pub source_created_at: Option<DateTime<Utc>>,
    pub source_modified_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Query boundary for services that need to inspect a folder hierarchy.
pub trait FolderTree {
    /// Returns one node, or `None` when the identifier is unknown.
    ///
    /// # Errors
    ///
    /// Returns a domain error when the hierarchy cannot satisfy the query.
    fn get_node(&self, id: NodeId) -> Result<Option<Node>, FolderError>;

    /// Returns active children of the requested folder.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an unknown folder or another domain error when
    /// the hierarchy cannot satisfy the query.
    fn list_children(&self, parent_id: NodeId) -> Result<Vec<Node>, FolderError>;
}

/// Expected business-rule failures for folder operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum FolderError {
    #[error("an active sibling already has that name")]
    NameConflict,
    #[error("the move would create a folder cycle")]
    CycleDetected,
    #[error("the folder or destination was not found")]
    NotFound,
    #[error("folder names cannot be empty")]
    InvalidName,
}

/// Stateless folder hierarchy validation service.
pub struct FolderRules;

impl FolderRules {
    /// Validates that a folder name contains at least one character.
    ///
    /// # Errors
    ///
    /// Returns `InvalidName` when the name is empty.
    pub fn validate_name(name: &str) -> Result<&str, FolderError> {
        if name.is_empty() {
            Err(FolderError::InvalidName)
        } else {
            Ok(name)
        }
    }

    /// Enforces case-sensitive uniqueness against active sibling nodes.
    ///
    /// # Errors
    ///
    /// Returns `NameConflict` when an active sibling has the exact name.
    pub fn validate_unique_sibling(name: &str, siblings: &[Node]) -> Result<(), FolderError> {
        if siblings.iter().any(|sibling| {
            sibling.lifecycle_state == LifecycleState::Active && sibling.name == name
        }) {
            Err(FolderError::NameConflict)
        } else {
            Ok(())
        }
    }

    /// Rejects moving a folder beneath itself or any known descendant.
    ///
    /// # Errors
    ///
    /// Returns `CycleDetected` when the target is the moving folder or one of
    /// its descendants.
    pub fn validate_move_target(
        folder_id: NodeId,
        target_id: NodeId,
        descendant_ids: &[NodeId],
    ) -> Result<(), FolderError> {
        if target_id == folder_id || descendant_ids.contains(&target_id) {
            Err(FolderError::CycleDetected)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use super::{FolderError, FolderRules, LifecycleState, Node, NodeId, NodeKind};

    fn node(name: &str, lifecycle_state: LifecycleState) -> Node {
        let now = Utc::now();
        Node {
            id: NodeId::new(Uuid::new_v4()),
            parent_id: None,
            name: name.to_owned(),
            kind: NodeKind::Folder,
            lifecycle_state,
            source_created_at: None,
            source_modified_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn rejects_only_empty_names() {
        assert_eq!(
            FolderRules::validate_name(""),
            Err(FolderError::InvalidName)
        );
        assert_eq!(FolderRules::validate_name(" "), Ok(" "));
        assert_eq!(FolderRules::validate_name("Photos"), Ok("Photos"));
    }

    #[test]
    fn sibling_uniqueness_is_active_and_case_sensitive() {
        let siblings = [
            node("Photos", LifecycleState::Active),
            node("Archive", LifecycleState::Trashed),
        ];

        assert_eq!(
            FolderRules::validate_unique_sibling("Photos", &siblings),
            Err(FolderError::NameConflict)
        );
        assert_eq!(
            FolderRules::validate_unique_sibling("photos", &siblings),
            Ok(())
        );
        assert_eq!(
            FolderRules::validate_unique_sibling("Archive", &siblings),
            Ok(())
        );
    }

    #[test]
    fn rejects_self_and_descendant_move_targets() {
        let folder_id = NodeId::new(Uuid::new_v4());
        let child_id = NodeId::new(Uuid::new_v4());
        let unrelated_id = NodeId::new(Uuid::new_v4());

        assert_eq!(
            FolderRules::validate_move_target(folder_id, folder_id, &[child_id]),
            Err(FolderError::CycleDetected)
        );
        assert_eq!(
            FolderRules::validate_move_target(folder_id, child_id, &[child_id]),
            Err(FolderError::CycleDetected)
        );
        assert_eq!(
            FolderRules::validate_move_target(folder_id, unrelated_id, &[child_id]),
            Ok(())
        );
    }
}
