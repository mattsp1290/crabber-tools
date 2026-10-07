//! Capability-rooted workspace admission and syntactic path validation.
use crate::{ToolError, category};
use cap_std::{ambient_authority, fs::Dir};
use crabber::ExtensionError;
use std::{
    io,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

/// An admitted directory inode; reopen after the host replaces the directory.
pub struct WorkspaceRoot {
    root: PathBuf,
    dir: Dir,
}
impl WorkspaceRoot {
    /// Admit an existing absolute directory. Invalid admission yields Plan("workspace").
    pub fn open(path: &Path) -> Result<Arc<Self>, ExtensionError> {
        let fail = || ExtensionError::Plan("workspace".into());
        if !path.is_absolute() {
            return Err(fail());
        }
        let root = path.canonicalize().map_err(|_| fail())?;
        let dir = Dir::open_ambient_dir(&root, ambient_authority()).map_err(|_| fail())?;
        Ok(Arc::new(Self { root, dir }))
    }
    /// Canonical absolute path used for cwd, root locks, and identity.
    pub fn path(&self) -> &Path {
        &self.root
    }
    /// All file operations must use this directory capability.
    pub fn dir(&self) -> &Dir {
        &self.dir
    }
    /// Absent directories are accepted; relative or different directories fail closed.
    pub fn matches_session_directory(&self, directory: Option<&str>) -> Result<(), ToolError> {
        let Some(d) = directory else {
            return Ok(());
        };
        if Path::new(d).is_absolute()
            && Path::new(d).canonicalize().ok().as_ref() == Some(&self.root)
        {
            Ok(())
        } else {
            Err(ToolError::new(
                category::WORKSPACE_MISMATCH,
                "session directory differs from admitted workspace",
            ))
        }
    }
}
/// A normalized workspace-relative name with no parent traversal or NUL.
#[derive(Clone, Debug)]
pub struct RelPath(PathBuf);
impl RelPath {
    /// Empty and dot paths are root only when allow_empty is true. Never expands tilde.
    pub fn parse(value: &str, allow_empty: bool) -> Result<Self, ToolError> {
        if value.contains('\0') {
            return Err(ToolError::new(category::VALIDATION, "path contains NUL"));
        }
        let mut path = PathBuf::new();
        for c in Path::new(value).components() {
            match c {
                Component::Normal(p) => path.push(p),
                Component::CurDir => {}
                _ => {
                    return Err(ToolError::new(
                        category::PATH_ESCAPE,
                        "path must stay inside workspace",
                    ));
                }
            }
        }
        if path.as_os_str().is_empty() && !allow_empty {
            if !value.is_empty() {
                return Ok(Self(PathBuf::from(".")));
            }
            return Err(ToolError::new(category::VALIDATION, "path is required"));
        }
        Ok(Self(path))
    }
    /// Capability-relative path (dot represents the root).
    pub fn as_path(&self) -> &Path {
        if self.is_root() {
            Path::new(".")
        } else {
            &self.0
        }
    }
    /// Whether this names the admitted root.
    pub fn is_root(&self) -> bool {
        self.0.as_os_str().is_empty() || self.0 == Path::new(".")
    }
}
/// Map filesystem failures without exposing the operating-system error message.
pub fn category_for_io(error: &io::Error) -> &'static str {
    if error
        .to_string()
        .contains("a path led outside of the filesystem")
    {
        return category::PATH_ESCAPE;
    }
    match error.kind() {
        io::ErrorKind::NotFound => category::NOT_FOUND,
        io::ErrorKind::NotADirectory => category::NOT_DIRECTORY,
        io::ErrorKind::IsADirectory => category::IS_DIRECTORY,
        _ => category::IO,
    }
}
