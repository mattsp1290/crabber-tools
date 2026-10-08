//! Model-facing result envelopes. Cancellation is an executor error, never an envelope.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Eino-compatible orchestration outcomes; tools emit Succeeded or Failed only.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Completed tool work (including a shell command with nonzero status).
    Succeeded,
    /// Tool-level failure.
    Failed,
    /// Reserved for caller timeouts.
    TimedOut,
    /// Reserved for orchestration rejection.
    Rejected,
}
/// A sanitized error: messages must not reveal host paths or contents.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct ToolError {
    /// Stable model-facing category.
    pub category: &'static str,
    /// Sanitized explanation.
    pub message: String,
}
impl ToolError {
    /// Construct an error without any operating-system error text.
    pub fn new(category: &'static str, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
        }
    }
    /// Convert to the failure envelope.
    pub fn value(&self) -> Value {
        failed(self.category, &self.message)
    }
}
/// Stable error category vocabulary.
pub mod category {
    /// The `validation` failure category.
    pub const VALIDATION: &str = "validation";
    /// The `path_escape` failure category.
    pub const PATH_ESCAPE: &str = "path_escape";
    /// The `not_found` failure category.
    pub const NOT_FOUND: &str = "not_found";
    /// The `is_directory` failure category.
    pub const IS_DIRECTORY: &str = "is_directory";
    /// The `not_directory` failure category.
    pub const NOT_DIRECTORY: &str = "not_directory";
    /// The `anchor_not_found` failure category.
    pub const ANCHOR_NOT_FOUND: &str = "anchor_not_found";
    /// The `anchor_ambiguous` failure category.
    pub const ANCHOR_AMBIGUOUS: &str = "anchor_ambiguous";
    /// The `too_large` failure category.
    pub const TOO_LARGE: &str = "too_large";
    /// The `binary` failure category.
    pub const BINARY: &str = "binary";
    /// The `io` failure category.
    pub const IO: &str = "io";
    /// The `timeout` failure category.
    pub const TIMEOUT: &str = "timeout";
    /// The `exec_failed` failure category.
    pub const EXEC_FAILED: &str = "exec_failed";
    /// The `invalid_pattern` failure category.
    pub const INVALID_PATTERN: &str = "invalid_pattern";
    /// The `workspace_mismatch` failure category.
    pub const WORKSPACE_MISMATCH: &str = "workspace_mismatch";
    /// The `unavailable` failure category.
    pub const UNAVAILABLE: &str = "unavailable";
    /// The `unknown` failure category.
    pub const UNKNOWN: &str = "unknown";
}
/// Build an envelope containing exactly outcome and error.
pub fn failed(category: &'static str, message: impl Into<String>) -> Value {
    json!({"outcome":"failed","error": {"category":category,"message":message.into()}})
}
