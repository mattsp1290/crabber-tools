//! Host-owned user interaction with a pending relay and bounded prompt waits.
//! Questions and answers cross the host boundary unredacted. Hosts should mount
//! a tool-result redactor; this tool must not be used to collect secrets.
use async_trait::async_trait;
use crabber::{ExtensionError, ToolDefinition, ToolExecutor, extension::ToolContext};
use crabber_tools_core::{category, check_cancelled, failed};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::Semaphore;
mod info;
mod policy;
mod run;
pub use info::info;
pub use policy::{
    MAX_ANSWER_BYTES, MAX_OUTSTANDING_PROMPTS, MAX_QUESTION_BYTES, MAX_WAIT, Surface,
    UserInteractPolicy,
};
/// Explicit host policy; prompt waits do not consume workspace capacity.
pub struct Options {
    /// Validated interaction surface and bounds.
    pub policy: UserInteractPolicy,
}
/// Validate policy and construct an executor with its own prompt semaphore.
pub fn definition(options: Arc<Options>) -> Result<Arc<ToolDefinition>, ExtensionError> {
    options.policy.validate()?;
    let prompts = Arc::new(Semaphore::new(options.policy.max_outstanding_prompts));
    Ok(Arc::new(ToolDefinition {
        info: info(),
        executor: Arc::new(UserTool { options, prompts }),
    }))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    question: String,
    #[serde(default)]
    answer: String,
}
struct UserTool {
    options: Arc<Options>,
    prompts: Arc<Semaphore>,
}
#[async_trait]
impl ToolExecutor for UserTool {
    async fn execute(&self, _: Value) -> Result<Value, ExtensionError> {
        Ok(failed(category::VALIDATION, "context required"))
    }
    async fn execute_with_context(
        &self,
        ctx: ToolContext,
        value: Value,
    ) -> Result<Value, ExtensionError> {
        check_cancelled(&ctx.cancel)?;
        let args = match serde_json::from_value::<Args>(value) {
            Ok(args) => args,
            Err(_) => return Ok(failed(category::VALIDATION, "invalid arguments")),
        };
        let policy = &self.options.policy;
        if args.question.trim().is_empty() {
            return Ok(failed(category::VALIDATION, "question is required"));
        }
        if args
            .question
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Ok(failed(
                category::VALIDATION,
                "question contains control characters",
            ));
        }
        if args.question.len() > policy.max_question_bytes {
            return Ok(failed(
                category::TOO_LARGE,
                format!("question exceeds {} bytes", policy.max_question_bytes),
            ));
        }
        if let Some(failure) = run::validate_answer(&args.answer, policy.max_answer_bytes) {
            return Ok(failure);
        }
        match &policy.surface {
            Surface::Pending if !args.answer.is_empty() => {
                Ok(json!({"outcome":"succeeded","answer":args.answer}))
            }
            Surface::Pending => Ok(json!({"outcome":"pending","question":args.question})),
            Surface::Prompter { .. } if !args.answer.is_empty() => Ok(failed(
                category::VALIDATION,
                "answer is reserved for the pending surface",
            )),
            Surface::Prompter { prompter, .. } => {
                let Ok(permit) = self.prompts.clone().try_acquire_owned() else {
                    return Ok(failed(
                        category::UNAVAILABLE,
                        "another question is outstanding",
                    ));
                };
                run::prompt(prompter.clone(), args.question, policy, &ctx.cancel, permit).await
            }
        }
    }
}
