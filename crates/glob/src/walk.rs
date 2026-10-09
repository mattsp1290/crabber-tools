use cap_std::fs::Dir;
use crabber_tools_core::{RelPath, ToolError, WorkspaceRoot, category, category_for_io};
use globset::GlobMatcher;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

fn io_error(e: std::io::Error) -> ToolError {
    ToolError::new(category_for_io(&e), "workspace traversal failed")
}
fn check(cancel: &CancellationToken) -> Result<(), ToolError> {
    if cancel.is_cancelled() {
        Err(ToolError::new(category::UNKNOWN, "cancelled"))
    } else {
        Ok(())
    }
}

// Keep at most eight directory batches of 256 names, with no directory handles
// retained between calls. Evicted or exhausted batches resume after the last
// visited name. This bounds both wide-tree memory and deep-tree descriptor use.
const BATCH: usize = 256;
const CACHED_DIRS: usize = 8;
#[derive(Default)]
struct Cursor {
    batches: BTreeMap<PathBuf, Batch>,
    clock: u64,
}
struct Batch {
    after: Option<OsString>,
    names: VecDeque<OsString>,
    used: u64,
}
impl Cursor {
    fn next_child(
        &mut self,
        dir: &Dir,
        parent: &Path,
        after: Option<&OsStr>,
        cancel: &CancellationToken,
    ) -> Result<Option<OsString>, ToolError> {
        check(cancel)?;
        self.clock += 1;
        if let Some(batch) = self.batches.get_mut(parent)
            && batch.after.as_deref() == after
            && let Some(name) = batch.names.pop_front()
        {
            batch.after = Some(name.clone());
            batch.used = self.clock;
            return Ok(Some(name));
        }
        self.batches.remove(parent);
        let dir = dir
            .open_dir(if parent.as_os_str().is_empty() {
                Path::new(".")
            } else {
                parent
            })
            .map_err(io_error)?;
        let mut names = BTreeSet::new();
        for entry in dir.entries().map_err(io_error)? {
            check(cancel)?;
            let name = entry.map_err(io_error)?.file_name();
            if after.is_none_or(|last| name.as_os_str() > last) {
                names.insert(name);
                if names.len() > BATCH {
                    names.pop_last();
                }
            }
        }
        let mut names: VecDeque<_> = names.into_iter().collect();
        let next = names.pop_front();
        if let Some(ref name) = next {
            if self.batches.len() == CACHED_DIRS {
                let oldest = self
                    .batches
                    .iter()
                    .min_by_key(|(_, b)| b.used)
                    .unwrap()
                    .0
                    .clone();
                self.batches.remove(&oldest);
            }
            self.batches.insert(
                parent.to_owned(),
                Batch {
                    after: Some(name.clone()),
                    names,
                    used: self.clock,
                },
            );
        }
        Ok(next)
    }
}

pub(crate) fn discover(
    root: &WorkspaceRoot,
    start: RelPath,
    pattern: GlobMatcher,
    limit: usize,
    cancel: &CancellationToken,
) -> Result<Value, ToolError> {
    let dir = root.dir().open_dir(start.as_path()).map_err(io_error)?;
    let prefix = if start.is_root() {
        Path::new("")
    } else {
        start.as_path()
    };
    let mut selected = BTreeMap::new();
    let mut truncated = false;
    let mut cursor = Cursor::default();
    let mut current = cursor
        .next_child(&dir, Path::new(""), None, cancel)?
        .map(PathBuf::from);
    while let Some(path) = current.take() {
        check(cancel)?;
        let meta = dir.symlink_metadata(&path).map_err(io_error)?;
        let skip = meta.is_dir()
            && matches!(
                path.file_name().and_then(OsStr::to_str),
                Some(".git" | ".hg" | ".svn" | ".jj")
            );
        if !skip && pattern.is_match(&path) {
            selected.insert(
                prefix.join(&path).to_string_lossy().into_owned(),
                meta.is_dir(),
            );
            if selected.len() > limit {
                selected.pop_last();
                truncated = true;
            }
        }
        if meta.is_dir()
            && !skip
            && let Some(child) = cursor.next_child(&dir, &path, None, cancel)?
        {
            current = Some(path.join(child));
            continue;
        }
        let mut finished = path;
        loop {
            let parent = finished.parent().expect("relative child");
            if let Some(next) = cursor.next_child(&dir, parent, finished.file_name(), cancel)? {
                current = Some(parent.join(next));
                break;
            }
            if parent.as_os_str().is_empty() {
                break;
            }
            finished = parent.to_owned();
        }
    }
    let paths: Vec<_> = selected
        .into_iter()
        .map(|(path, is_dir)| json!({"path":path,"is_dir":is_dir}))
        .collect();
    Ok(json!({"outcome":"succeeded","count":paths.len(),"paths":paths,"truncated":truncated}))
}
