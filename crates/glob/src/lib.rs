//! Capability-confined glob discovery with bounded retained results.
use async_trait::async_trait;
use crabber::{
    ExtensionError, ToolDefinition, ToolExecutor, core::ToolInfo, extension::ToolContext,
};
use crabber_tools_core::*;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
mod walk;

/// Maximum number of returned paths.
pub const MAX_LIMIT: usize = 5000;
/// Default number of returned paths.
pub const DEFAULT_LIMIT: usize = 1000;
/// Host resources, shared with the other tools in a mount.
pub struct Options {
    /// Admitted directory capability.
    pub root: Arc<WorkspaceRoot>,
    /// Finite concurrency and acquisition bounds.
    pub limits: Limits,
    /// Mount-wide capacity.
    pub capacity: Capacity,
}
/// Deterministic model-facing schema and advisory read permission.
pub fn info() -> ToolInfo {
    ToolInfo {
        name: "glob".into(),
        description: "Discover workspace-relative paths with doublestar glob semantics (*, ?, **). Hidden files are included by default; VCS internals are skipped. Results are sorted, capped, and returned with is_dir metadata.".into(),
        parameters: json!({"type":"object","additionalProperties":false,"properties":{
            "pattern":{"type":"string","minLength":1,"description":"Doublestar glob pattern to match against paths under the search root, e.g. \"*.go\" or \"**/*_test.go\"."},
            "path":{"type":"string","description":"Workspace-relative directory to search. Omit or use \".\" for the workspace root."},
            "limit":{"type":"integer","minimum":1,"maximum":5000,"description":"Maximum number of paths to return. Default 1000; hard cap 5000."}},"required":["pattern"]}),
        retry_safe: true,
        required_permissions: vec![permission::FS_READ.into()],
    }
}
/// Construct a context-required executor using the admitted root.
pub fn definition(options: Arc<Options>) -> Arc<ToolDefinition> {
    Arc::new(ToolDefinition {
        info: info(),
        executor: Arc::new(GlobTool(options)),
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    pattern: String,
    #[serde(default)]
    path: String,
    #[serde(default = "default_limit")]
    limit: usize,
}
fn default_limit() -> usize {
    DEFAULT_LIMIT
}
impl Args {
    fn validate(self) -> Result<(RelPath, globset::GlobMatcher, usize), ToolError> {
        if self.pattern.trim().is_empty()
            || self.pattern.contains('\0')
            || std::path::Path::new(&self.pattern).is_absolute()
            || self.pattern.split('/').any(|p| p == "..")
            || !(1..=MAX_LIMIT).contains(&self.limit)
        {
            return Err(ToolError::new(
                category::VALIDATION,
                "invalid pattern or limit",
            ));
        }
        let pattern = globset::GlobBuilder::new(&self.pattern)
            .literal_separator(true)
            .build()
            .map_err(|_| ToolError::new(category::VALIDATION, "invalid glob pattern"))?
            .compile_matcher();
        Ok((RelPath::parse(&self.path, true)?, pattern, self.limit))
    }
}
fn failure(error: ToolError) -> Value {
    let mut v = error.value();
    v["paths"] = json!([]);
    v["count"] = json!(0);
    v["truncated"] = json!(false);
    v
}
struct GlobTool(Arc<Options>);
#[async_trait]
impl ToolExecutor for GlobTool {
    async fn execute(&self, _: Value) -> Result<Value, ExtensionError> {
        Ok(failure(ToolError::new(
            category::VALIDATION,
            "context required",
        )))
    }
    async fn execute_with_context(
        &self,
        ctx: ToolContext,
        args: Value,
    ) -> Result<Value, ExtensionError> {
        check_cancelled(&ctx.cancel)?;
        let args = match serde_json::from_value::<Args>(args)
            .map_err(|_| ToolError::new(category::VALIDATION, "invalid arguments"))
            .and_then(Args::validate)
        {
            Ok(a) => a,
            Err(e) => return Ok(failure(e)),
        };
        if let Err(e) = self
            .0
            .root
            .matches_session_directory(ctx.workspace().directory())
        {
            return Ok(failure(e));
        }
        let permit = match acquire_read(&self.0.capacity, &self.0.limits, &ctx.cancel).await {
            Ok(p) => p,
            Err(AcquireError::Cancelled) => return Err(ExtensionError::Tool("cancelled".into())),
            Err(AcquireError::Unavailable) => {
                return Ok(failure(ToolError::new(
                    category::UNAVAILABLE,
                    "workspace busy",
                )));
            }
        };
        let options = self.0.clone();
        let cancel = ctx.cancel.clone();
        run_blocking(&ctx.cancel, move || {
            let _permit = permit;
            walk::discover(&options.root, args.0, args.1, args.2, &cancel).unwrap_or_else(failure)
        })
        .await
    }
}
