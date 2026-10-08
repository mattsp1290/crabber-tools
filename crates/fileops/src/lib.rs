//! Capability-confined reads, atomic writes, anchored edits, and directory listings.
use async_trait::async_trait;
use crabber::{ExtensionError, ToolExecutor, extension::ToolContext};
use crabber_tools_core::*;
use serde_json::Value;
use std::sync::Arc;
mod atomic;
pub mod edit;
pub mod list;
mod operations;
mod options;
pub mod read;
mod text;
pub mod write;
pub use options::*;
/// Maximum retained output and write input size.
pub const MAX_OUTPUT_BYTES: usize = 256 * 1024;
/// Default window length when an offset is supplied.
pub const DEFAULT_READ_WINDOW_LINES: usize = 2000;
/// Maximum requested window size.
pub const MAX_READ_WINDOW_LINES: usize = 5000;
/// Retained bytes per line in window mode.
pub const MAX_READ_LINE_BYTES: usize = 16 * 1024;
/// Maximum entries in a directory listing.
pub const MAX_LIST_ENTRIES: usize = 5000;
/// Permission bits for new files.
pub const NEW_FILE_MODE: u32 = 0o644;
/// Atomic-write siblings; a host crash may leave one behind.
pub const TEMP_PREFIX: &str = ".crabber-tools-tmp-";
#[derive(Clone, Copy)]
enum Kind {
    Read,
    Write,
    Edit,
    List,
}
struct FileTool {
    options: Arc<Options>,
    kind: Kind,
}
#[async_trait]
impl ToolExecutor for FileTool {
    async fn execute(&self, _: Value) -> Result<Value, ExtensionError> {
        Ok(failed(category::VALIDATION, "context required"))
    }
    async fn execute_with_context(
        &self,
        ctx: ToolContext,
        args: Value,
    ) -> Result<Value, ExtensionError> {
        self.invoke(ctx, args).await.map(|mut value| {
            let extra = match self.kind {
                Kind::List => Some(("entries", serde_json::json!([]))),
                Kind::Edit => Some(("anchor_occurrences", serde_json::json!(0))),
                _ => None,
            };
            if let Some((key, default)) = extra {
                value
                    .as_object_mut()
                    .expect("envelope object")
                    .entry(key)
                    .or_insert(default);
            }
            value
        })
    }
}
impl FileTool {
    async fn invoke(&self, ctx: ToolContext, args: Value) -> Result<Value, ExtensionError> {
        check_cancelled(&ctx.cancel)?;
        let request = match operations::Request::parse(self.kind, args) {
            Ok(r) => r,
            Err(e) => return Ok(e.value()),
        };
        if let Err(e) = self
            .options
            .root
            .matches_session_directory(ctx.workspace().directory())
        {
            return Ok(e.value());
        }
        let lock = RootLock::for_root(&self.options.root);
        let guards = match self.kind {
            Kind::Write | Kind::Edit => acquire_mutating(
                &lock,
                &self.options.capacity,
                &self.options.limits,
                &ctx.cancel,
            )
            .await
            .map(|(g, p)| (Some(g), p)),
            _ => acquire_read(&self.options.capacity, &self.options.limits, &ctx.cancel)
                .await
                .map(|p| (None, p)),
        };
        let guards = match guards {
            Ok(g) => g,
            Err(AcquireError::Cancelled) => return Err(ExtensionError::Tool("cancelled".into())),
            Err(AcquireError::Unavailable) => {
                return Ok(failed(category::UNAVAILABLE, "workspace busy"));
            }
        };
        let options = self.options.clone();
        let cancel = ctx.cancel.clone();
        run_blocking(&ctx.cancel, move || {
            let _guards = guards;
            request.run(&options, &cancel)
        })
        .await
    }
}
