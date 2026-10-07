//! The file_list tool definition.
use crate::{FileTool, Kind, Options};
use crabber::{ToolDefinition, core::ToolInfo};
use std::sync::Arc;
/// Deterministic model-facing schema and advisory safety metadata.
pub fn info() -> ToolInfo {
    ToolInfo {name: "file_list".into(), description: "List directory entries under a workspace-relative path (empty or \".\" lists the workspace root). Output is sorted and capped at 5000 entries; oversize results set truncated=true.".into(), parameters: serde_json::json!({"properties": {"path": {"minLength": 1, "description": "Workspace-relative directory. Use '.' for the workspace root. Omit the field entirely to default to '.'.", "type": "string"}, "recursive": {"description": "If true, walk descendants. Default false (immediate children only).", "type": "boolean"}}, "additionalProperties": false, "type": "object"}), retry_safe: true, required_permissions: vec![crabber_tools_core::permission::FS_READ.into()]}
}
/// Build a context-required executor sharing the mount's capacity.
pub fn definition(options: Arc<Options>) -> Arc<ToolDefinition> {
    Arc::new(ToolDefinition {
        info: info(),
        executor: Arc::new(FileTool {
            options,
            kind: Kind::List,
        }),
    })
}

use crate::{
    MAX_LIST_ENTRIES,
    operations::{ListArgs, Request, io_error},
};
use crabber_tools_core::{ToolError, category};
use serde_json::{Value, json};
use std::path::Path;
use tokio_util::sync::CancellationToken;

impl Request {
    pub(crate) fn list(
        &self,
        options: &Options,
        a: &ListArgs,
        cancel: &CancellationToken,
    ) -> Result<Value, ToolError> {
        let dir = options
            .root
            .dir()
            .open_dir(self.path.as_path())
            .map_err(io_error)?;
        let mut entries = vec![];
        let truncated = walk(
            &dir,
            if self.path.is_root() {
                Path::new("")
            } else {
                self.path.as_path()
            },
            a.recursive,
            cancel,
            &mut entries,
        )?;
        Ok(if truncated {
            json!({"outcome":"succeeded","entries":entries,"truncated":true})
        } else {
            json!({"outcome":"succeeded","entries":entries})
        })
    }
}

fn walk(
    dir: &cap_std::fs::Dir,
    prefix: &Path,
    recursive: bool,
    cancel: &CancellationToken,
    out: &mut Vec<Value>,
) -> Result<bool, ToolError> {
    // Keep only the smallest remaining names. A huge directory cannot allocate
    // an unbounded vector; the total result and sorting heap are both capped.
    let mut names = std::collections::BinaryHeap::new();
    let mut overflow = false;
    for entry in dir.entries().map_err(io_error)? {
        if cancel.is_cancelled() {
            return Err(ToolError::new(category::UNKNOWN, "cancelled"));
        }
        let name = entry.map_err(io_error)?.file_name();
        names.push(name);
        if names.len() > MAX_LIST_ENTRIES - out.len() {
            names.pop();
            overflow = true;
        }
    }
    for name in names.into_sorted_vec() {
        if cancel.is_cancelled() {
            return Err(ToolError::new(category::UNKNOWN, "cancelled"));
        }
        let is_dir = dir.symlink_metadata(&name).map_err(io_error)?.is_dir();
        let path = prefix.join(&name);
        out.push(json!({"path":path.to_string_lossy(),"is_dir":is_dir}));
        if out.len() == MAX_LIST_ENTRIES {
            return Ok(true);
        }
        if recursive
            && is_dir
            && walk(
                &dir.open_dir(&name).map_err(io_error)?,
                &path,
                true,
                cancel,
                out,
            )?
        {
            return Ok(true);
        }
    }
    Ok(overflow)
}
