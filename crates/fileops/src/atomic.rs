//! Temp siblings, preserved modes and ownership, and atomic rename through capabilities.
use crate::{NEW_FILE_MODE, TEMP_PREFIX};
use cap_std::fs::PermissionsExt;
use cap_std::fs::{Dir, OpenOptions, Permissions};
use std::{
    io::{self, Write},
    path::Path,
};

pub(crate) fn write(dir: &Dir, target: &Path, bytes: &[u8]) -> io::Result<bool> {
    let target = match dir.symlink_metadata(target) {
        Ok(meta) if meta.file_type().is_symlink() => dir.canonicalize(target)?,
        Ok(_) => target.to_owned(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => target.to_owned(),
        Err(e) => return Err(e),
    };
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = dir.open_dir(parent)?;
    let name = target
        .file_name()
        .ok_or_else(|| io::Error::from(io::ErrorKind::IsADirectory))?;
    let existing = match parent.metadata(name) {
        Ok(m) => Some(m),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    if existing.as_ref().is_some_and(|m| m.is_dir()) {
        return Err(io::Error::from(io::ErrorKind::IsADirectory));
    }
    if existing.as_ref().is_some_and(|m| !m.is_file()) {
        return Err(io::Error::other("not a regular file"));
    }
    let owner = existing.clone().unwrap_or(parent.dir_metadata()?);
    let mode = existing
        .as_ref()
        .map(|m| m.permissions().mode())
        .unwrap_or(NEW_FILE_MODE);
    let temp = crabber_tools_core::temp_name(TEMP_PREFIX)?;
    let mut file = parent.open_with(&temp, OpenOptions::new().write(true).create_new(true))?;
    let result = (|| {
        use cap_std::fs::MetadataExt;
        let uid = rustix::process::geteuid();
        if uid.is_root() || uid.as_raw() == owner.uid() {
            rustix::fs::fchown(
                &file,
                Some(rustix::fs::Uid::from_raw(owner.uid())),
                Some(rustix::fs::Gid::from_raw(owner.gid())),
            )?;
        }
        file.set_permissions(Permissions::from_mode(mode))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        parent.rename(&temp, &parent, name)
    })();
    if result.is_err() {
        let _ = parent.remove_file(&temp);
    }
    result.map(|()| existing.is_none())
}
