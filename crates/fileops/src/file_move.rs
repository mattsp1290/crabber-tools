//! Platform facts for capability-confined hard-link moves.

#[cfg(test)]
mod tests {
    use cap_std::{ambient_authority, fs::Dir};
    use std::os::unix::fs::symlink;

    #[test]
    fn hard_link_platform_facts() {
        let root = tempfile::tempdir().unwrap();
        let dir = Dir::open_ambient_dir(root.path(), ambient_authority()).unwrap();
        dir.write("source", b"contents").unwrap();
        dir.hard_link("source", &dir, "destination").unwrap();
        assert!(dir.symlink_metadata("destination").unwrap().is_file());
        assert_eq!(dir.read("destination").unwrap(), b"contents");

        symlink("source", root.path().join("symlink")).unwrap();
        dir.hard_link("symlink", &dir, "linked-symlink").unwrap();
        let metadata = dir.symlink_metadata("linked-symlink").unwrap();
        eprintln!(
            "linked source symlink: is_symlink={}, is_file={}",
            metadata.is_symlink(),
            metadata.is_file()
        );
        // cap-std links the name itself on the supported Unix platforms.
        assert!(metadata.is_symlink());
        assert_eq!(
            dir.read_link("linked-symlink").unwrap(),
            std::path::Path::new("source")
        );

        if let Ok(other) = tempfile::tempdir_in("/dev/shm") {
            use std::os::unix::fs::MetadataExt;
            if std::fs::metadata(root.path()).unwrap().dev()
                != std::fs::metadata(other.path()).unwrap().dev()
            {
                let destination = Dir::open_ambient_dir(other.path(), ambient_authority()).unwrap();
                let error = dir
                    .hard_link("source", &destination, "cross-device")
                    .unwrap_err();
                assert_eq!(error.raw_os_error(), Some(libc::EXDEV));
                assert!(!other.path().join("cross-device").exists());
                assert_eq!(dir.read("source").unwrap(), b"contents");
            }
        }
    }
}
