//! Standalone consumer graph: catalog + command guard + host-owned tools.
mod probe;
use crabber_extensions::command_guard::{CommandGuard, Limits, Options, Rule, default_bindings};
use std::sync::Arc;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let guard = CommandGuard::new(Options {
        bindings: default_bindings(),
        rules: vec![Rule {
            id: "deny-push".into(),
            executable: "git".into(),
            arg_prefix: vec!["push".into()],
        }],
        limits: Limits {
            max_bindings: 8,
            max_rules: 8,
            max_rule_bytes: 4096,
            max_prefix_args: 16,
            max_json_depth: 16,
            max_json_nodes: 1024,
            max_command_bytes: 16384,
            max_analysis_bytes: 65536,
            max_ast_nodes: 4096,
            max_ast_depth: 16,
            max_words: 2048,
            max_word_bytes: 16384,
            max_wrapper_depth: 16,
            max_in_flight: 4,
        },
    })?;
    probe::run(false, Some(Arc::new(guard))).await?;
    probe::run(true, None).await?;
    Ok(())
}
