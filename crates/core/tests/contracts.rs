//! Core invariants exercised with real capabilities, locks, and processes.
use crabber_tools_core::*;
use serde_json::json;
use std::{path::Path, process::Stdio, time::Duration};
use tokio_util::sync::CancellationToken;

fn limits() -> Limits {
    Limits {
        max_in_flight: 1,
        max_blocking_wait: Duration::from_millis(40),
    }
}
#[test]
fn paths_and_session_admission() {
    for path in ["a/b.txt", "./a", "~x"] {
        assert!(RelPath::parse(path, false).is_ok());
    }
    for path in ["", "/abs", "../x", "a/../..", "a\0b"] {
        assert!(RelPath::parse(path, false).is_err());
    }
    assert!(RelPath::parse(".", true).unwrap().is_root());
    assert!(WorkspaceRoot::open(Path::new(".")).is_err());
    let tmp = tempfile::tempdir().unwrap();
    let root = WorkspaceRoot::open(tmp.path()).unwrap();
    std::fs::write(tmp.path().join("file"), "hello").unwrap();
    assert!(WorkspaceRoot::open(&tmp.path().join("file")).is_err());
    assert!(root.matches_session_directory(None).is_ok());
    assert!(
        root.matches_session_directory(Some(tmp.path().to_str().unwrap()))
            .is_ok()
    );
    assert!(
        root.matches_session_directory(Some(tmp.path().join(".").to_str().unwrap()))
            .is_ok()
    );
    assert_eq!(
        root.matches_session_directory(Some("."))
            .unwrap_err()
            .category,
        category::WORKSPACE_MISMATCH
    );
    let other = tempfile::tempdir().unwrap();
    assert!(
        root.matches_session_directory(Some(other.path().to_str().unwrap()))
            .is_err()
    );
}
#[test]
fn real_symlinks_fail_closed() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("real"), "inside").unwrap();
    std::fs::write(outside.path().join("canary"), "outside").unwrap();
    let root = WorkspaceRoot::open(tmp.path()).unwrap();
    for (name, target) in [
        ("escape", outside.path().to_owned()),
        ("parent", Path::new("../outside").to_owned()),
        ("absolute", tmp.path().join("real")),
    ] {
        symlink(target, tmp.path().join(name)).unwrap();
        let e = root.dir().open(name).unwrap_err();
        assert_eq!(category_for_io(&e), category::PATH_ESCAPE, "{e}");
    }
    symlink("real", tmp.path().join("relative")).unwrap();
    assert!(root.dir().open("relative").is_ok());
    assert_eq!(
        std::fs::read_to_string(outside.path().join("canary")).unwrap(),
        "outside"
    );
}
#[tokio::test]
async fn capacity_timeout_and_global_lock() {
    let tmp = tempfile::tempdir().unwrap();
    let root = WorkspaceRoot::open(tmp.path()).unwrap();
    let again = WorkspaceRoot::open(tmp.path()).unwrap();
    let limits = limits();
    let capacity = Capacity::new(&limits).unwrap();
    let cancel = CancellationToken::new();
    let first = acquire_read(&capacity, &limits, &cancel).await.unwrap();
    assert_eq!(
        acquire_read(&capacity, &limits, &cancel).await.unwrap_err(),
        AcquireError::Unavailable
    );
    drop(first);
    let (lock, permit) = acquire_mutating(&RootLock::for_root(&root), &capacity, &limits, &cancel)
        .await
        .unwrap();
    drop(permit);
    assert!(acquire_read(&capacity, &limits, &cancel).await.is_ok());
    assert!(matches!(
        acquire_mutating(&RootLock::for_root(&again), &capacity, &limits, &cancel).await,
        Err(AcquireError::Unavailable)
    ));
    drop(lock);
    cancel.cancel();
    assert_eq!(
        acquire_read(&capacity, &limits, &cancel).await.unwrap_err(),
        AcquireError::Cancelled
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocking_guard_survives_drop_and_panic_releases() {
    let tmp = tempfile::tempdir().unwrap();
    let root = WorkspaceRoot::open(tmp.path()).unwrap();
    let lock = RootLock::for_root(&root);
    let limits = limits();
    let capacity = Capacity::new(&limits).unwrap();
    let cancel = CancellationToken::new();
    let guards = acquire_mutating(&lock, &capacity, &limits, &cancel)
        .await
        .unwrap();
    let (start_tx, start_rx) = tokio::sync::oneshot::channel();
    let (end_tx, end_rx) = std::sync::mpsc::channel();
    let job = tokio::spawn(async move {
        run_blocking(&CancellationToken::new(), move || {
            let _guards = guards;
            start_tx.send(()).unwrap();
            end_rx.recv().unwrap();
            json!(null)
        })
        .await
    });
    start_rx.await.unwrap();
    job.abort();
    assert!(matches!(
        acquire_mutating(&lock, &capacity, &limits, &cancel).await,
        Err(AcquireError::Unavailable)
    ));
    end_tx.send(()).unwrap();
    let longer = Limits {
        max_blocking_wait: Duration::from_secs(1),
        ..limits
    };
    let guards = acquire_mutating(&lock, &capacity, &longer, &cancel)
        .await
        .unwrap();
    let value = run_blocking(&cancel, move || {
        let _guards = guards;
        panic!("injected");
    })
    .await
    .unwrap();
    assert_eq!(value["error"]["category"], "unknown");
    assert!(
        acquire_mutating(&lock, &capacity, &longer, &cancel)
            .await
            .is_ok()
    );
}
#[test]
fn envelopes_and_hashes() {
    assert_eq!(
        failed(category::IO, "read failed"),
        json!({"outcome":"failed","error":{"category":"io","message":"read failed"}})
    );
    let info = crabber::core::ToolInfo {
        name: "x".into(),
        description: "one".into(),
        parameters: json!({"z":1,"a":{"b":2,"a":3}}),
        retry_safe: true,
        required_permissions: vec![],
    };
    let mut changed = info.clone();
    changed.parameters = json!({"a":{"a":3,"b":2},"z":1});
    assert_eq!(schema_hash(&info), schema_hash(&changed));
    changed.description = "two".into();
    assert_ne!(schema_hash(&info), schema_hash(&changed));
    let names: std::collections::HashSet<_> =
        (0..100).map(|_| temp_name("test-").unwrap()).collect();
    assert_eq!(names.len(), 100);
}
#[tokio::test]
async fn process_environment_caps_timeout_and_pipe_grace() {
    let mut cmd = tokio::process::Command::new("/bin/sh");
    cmd.args([
        "-c",
        "printf '%s:%s' \"$ONLY\" \"${HOME-unset}\"; printf err >&2",
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    apply_policy(
        &mut cmd,
        &EnvPolicy::Replace(vec![("ONLY".into(), "yes".into())]),
        None,
    );
    let mut guard = ProcessGroupGuard::spawn(&mut cmd).unwrap();
    let result = capture(
        &mut guard,
        4,
        tokio::time::Instant::now() + Duration::from_secs(2),
    )
    .await
    .unwrap();
    assert_eq!(result.stdout.text(), "yes:");
    assert!(result.stdout.truncated);
    assert_eq!(result.stderr.text(), "err");
    assert!(!result.timed_out);
    let mut cmd = tokio::process::Command::new("/bin/sh");
    cmd.args(["-c", "sleep 60 & echo started"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_policy(&mut cmd, &EnvPolicy::minimal_allowlist(), None);
    let mut guard = ProcessGroupGuard::spawn(&mut cmd).unwrap();
    let start = tokio::time::Instant::now();
    let result = capture(&mut guard, 1024, start + Duration::from_secs(30))
        .await
        .unwrap();
    assert_eq!(result.stdout.text(), "started\n");
    assert!(!result.timed_out);
    assert!(start.elapsed() < Duration::from_secs(7));
}
#[tokio::test]
async fn dropped_guard_kills_group() {
    let tmp = tempfile::tempdir().unwrap();
    let pidfile = tmp.path().join("child.pid");
    let mut cmd = tokio::process::Command::new("/bin/sh");
    cmd.arg("-c")
        .arg("sleep 60 & echo $! > \"$1\"; wait")
        .arg("sh")
        .arg(&pidfile);
    apply_policy(&mut cmd, &EnvPolicy::minimal_allowlist(), None);
    let guard = ProcessGroupGuard::spawn(&mut cmd).unwrap();
    let leader = guard.pid();
    for _ in 0..100 {
        if pidfile.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let child = std::fs::read_to_string(pidfile)
        .unwrap()
        .trim()
        .parse::<u32>()
        .unwrap();
    drop(guard);
    for pid in [leader, child] {
        for _ in 0..100 {
            if !process_running(pid) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(!process_running(pid), "process {pid} survived");
    }
}
fn process_running(pid: u32) -> bool {
    // Linux may retain a killed orphan as a zombie until its init process reaps it.
    #[cfg(target_os = "linux")]
    if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        && stat
            .rsplit_once(") ")
            .is_some_and(|(_, s)| s.starts_with('Z'))
    {
        return false;
    }
    rustix::process::test_kill_process(rustix::process::Pid::from_raw(pid as i32).unwrap()).is_ok()
}
