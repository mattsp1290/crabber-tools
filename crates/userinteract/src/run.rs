//! Isolated host calls, aborted when the tool future is dropped.
use crate::UserInteractPolicy;
use crabber::{ExtensionError, extension::UserPrompter};
use crabber_tools_core::{category, failed};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
struct AbortOnDrop(JoinHandle<Result<String, String>>);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}
pub(crate) fn validate_answer(answer: &str, max_bytes: usize) -> Option<Value> {
    if answer.contains('\0') {
        Some(failed(category::VALIDATION, "answer contains NUL"))
    } else if answer.len() > max_bytes {
        Some(failed(
            category::TOO_LARGE,
            format!("answer exceeds {max_bytes} bytes"),
        ))
    } else {
        None
    }
}
pub(crate) async fn prompt(
    prompter: Arc<dyn UserPrompter>,
    question: String,
    policy: &UserInteractPolicy,
    cancel: &CancellationToken,
) -> Result<Value, ExtensionError> {
    let mut task = AbortOnDrop(tokio::spawn(async move { prompter.ask(&question).await }));
    tokio::select! { biased;
        _ = cancel.cancelled() => Err(ExtensionError::Tool("cancelled".into())),
        _ = tokio::time::sleep(policy.max_wait) => Ok(failed(category::TIMEOUT, "user response timed out")),
        joined = &mut task.0 => Ok(match joined {
            Ok(Ok(answer)) => {
                let answer = answer.trim_end_matches('\n');
                if let Some(failure) = validate_answer(answer, policy.max_answer_bytes) { failure }
                else if answer.is_empty() { json!({"outcome":"succeeded"}) }
                else { json!({"outcome":"succeeded","answer":answer}) }
            }
            Ok(Err(_)) => failed(category::IO, "prompt failed"),
            Err(_) => failed(category::UNKNOWN, "prompter panicked"),
        }),
    }
}
