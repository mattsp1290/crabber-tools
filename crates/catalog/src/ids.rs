//! Canonical tool order and safety declarations.
use serde::Serialize;
/// The tools available in the first deliverable.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Debug, PartialOrd, Ord)]
pub enum ToolId {
    /// Read UTF-8 files.
    FileRead,
    /// Atomically create or overwrite files.
    FileWrite,
    /// Replace one unique anchor atomically.
    FileEdit,
    /// List directory entries.
    FileList,
    /// Search with ripgrep.
    Search,
    /// Execute a host-configured shell.
    Shell,
}
impl ToolId {
    /// Deterministic registration order.
    pub const ALL: [Self; 6] = [
        Self::FileRead,
        Self::FileWrite,
        Self::FileEdit,
        Self::FileList,
        Self::Search,
        Self::Shell,
    ];
    /// Stable catalog id.
    pub const fn id(self) -> &'static str {
        match self {
            Self::FileRead => "standard.file-read",
            Self::FileWrite => "standard.file-write",
            Self::FileEdit => "standard.file-edit",
            Self::FileList => "standard.file-list",
            Self::Search => "standard.search",
            Self::Shell => "standard.shell",
        }
    }
    /// Model-facing tool name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::FileRead => "file_read",
            Self::FileWrite => "file_write",
            Self::FileEdit => "file_edit",
            Self::FileList => "file_list",
            Self::Search => "search",
            Self::Shell => "shell",
        }
    }
    /// Whether repeating the tool has no intentional writes.
    pub const fn retry_safe(self) -> bool {
        matches!(self, Self::FileRead | Self::FileList | Self::Search)
    }
    /// Whether execution serializes with other writers of this root.
    pub const fn mutating(self) -> bool {
        !self.retry_safe()
    }
}
/// Deterministic, deduplicated allowlist. Empty is rejected at construction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnabledSet(std::collections::BTreeSet<ToolId>);
impl EnabledSet {
    /// Enable all six first-deliverable tools.
    pub fn all() -> Self {
        Self::only(ToolId::ALL)
    }
    /// Register exactly these ids. Does not impose run-wide restrictions.
    pub fn only(ids: impl IntoIterator<Item = ToolId>) -> Self {
        Self(ids.into_iter().collect())
    }
    /// Whether this id will be mounted.
    pub fn contains(&self, id: ToolId) -> bool {
        self.0.contains(&id)
    }
}
