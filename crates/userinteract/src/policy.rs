//! Explicit host interaction surface and finite prompt bounds.
use crabber::{ExtensionError, extension::UserPrompter};
use serde::{Serialize, Serializer};
use std::{fmt, sync::Arc, time::Duration};
/// Maximum question size in bytes.
pub const MAX_QUESTION_BYTES: usize = 64 * 1024;
/// Maximum answer size in bytes.
pub const MAX_ANSWER_BYTES: usize = 1024 * 1024;
/// Maximum host response wait.
pub const MAX_WAIT: Duration = Duration::from_secs(600);
/// Maximum number of outstanding host prompts.
pub const MAX_OUTSTANDING_PROMPTS: usize = 16;
/// Host-owned answer collection mode.
#[derive(Clone)]
pub enum Surface {
    /// Relay a question immediately; the host supplies an answer in a later call.
    /// This surface does not enforce approval or correlate question/answer pairs.
    Pending,
    /// Wait for a non-blocking, drop-safe host prompter; model answers are rejected.
    Prompter {
        /// Stable host-assigned identity; only this value is serialized.
        identity: String,
        /// Host answer collector. Must not be used to collect secrets.
        prompter: Arc<dyn UserPrompter>,
    },
}
impl fmt::Debug for Surface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => f.write_str("Pending"),
            Self::Prompter { identity, .. } => f
                .debug_struct("Prompter")
                .field("identity", identity)
                .finish(),
        }
    }
}
impl Serialize for Surface {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Pending => serializer.serialize_str("pending"),
            Self::Prompter { identity, .. } => {
                use serde::ser::SerializeMap;
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("prompter", identity)?;
                map.end()
            }
        }
    }
}
/// Host interaction policy; every bound participates in configuration identity.
#[derive(Clone, Debug, Serialize)]
pub struct UserInteractPolicy {
    /// Answer collection mode.
    pub surface: Surface,
    /// Maximum question bytes, in 1..=MAX_QUESTION_BYTES.
    pub max_question_bytes: usize,
    /// Maximum answer bytes, in 1..=MAX_ANSWER_BYTES.
    pub max_answer_bytes: usize,
    /// Nonzero response timeout, at most MAX_WAIT; unused on Pending.
    pub max_wait: Duration,
    /// Prompt capacity, in 1..=MAX_OUTSTANDING_PROMPTS; unused on Pending.
    pub max_outstanding_prompts: usize,
}
impl UserInteractPolicy {
    /// Default relay policy, without a host prompter.
    pub fn pending() -> Self {
        Self {
            surface: Surface::Pending,
            max_question_bytes: MAX_QUESTION_BYTES,
            max_answer_bytes: MAX_ANSWER_BYTES,
            max_wait: Duration::from_secs(300),
            max_outstanding_prompts: 1,
        }
    }
    /// Validate bounds and the serialized host identity on either surface.
    pub fn validate(&self) -> Result<(), ExtensionError> {
        let identity_valid = match &self.surface {
            Surface::Pending => true,
            Surface::Prompter { identity, .. } => {
                !identity.is_empty()
                    && identity.len() <= 128
                    && !identity.starts_with('-')
                    && identity
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_./".contains(&b))
            }
        };
        if !(1..=MAX_QUESTION_BYTES).contains(&self.max_question_bytes)
            || !(1..=MAX_ANSWER_BYTES).contains(&self.max_answer_bytes)
            || self.max_wait.is_zero()
            || self.max_wait > MAX_WAIT
            || !(1..=MAX_OUTSTANDING_PROMPTS).contains(&self.max_outstanding_prompts)
            || !identity_valid
        {
            return Err(ExtensionError::Plan("user_interact policy".into()));
        }
        Ok(())
    }
}
