//! Ten-tool mount and explicit second-deliverable host policy contracts.
#[path = "../../../test-support/mod.rs"]
mod support;
use crabber::{
    Agent, FakeProvider, PermissionDecision, StaticPolicy,
    extension::{Extension, Scope},
    session::MemoryStore,
};
use crabber_tools_catalog::prelude::*;
use serde_json::json;
use std::{collections::BTreeSet, path::Path, sync::Arc};
use support::*;
fn options(root: &Path, hub: &Path) -> Options {
    let binary = hub.join("fake-bn");
    script(
        &binary,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$BEANS_HUB/argv\"\n",
    );
    Options {
        root: WorkspaceRoot::open(root).unwrap(),
        enabled: EnabledSet::all(),
        restrict_to_enabled: false,
        shell: ShellPolicy {
            shell_binary: "/bin/sh".into(),
            startup_mode: StartupMode::NonLogin,
            env: EnvPolicy::minimal_allowlist(),
            run_as: None,
            output_cap_bytes: 65536,
        },
        search: SearchPolicy::resolve_rg_from_path(EnvPolicy::minimal_allowlist()).unwrap(),
        url_fetch: Some(UrlFetchPolicy::default()),
        tracker: Some(TrackerPolicy {
            bn_binary: binary,
            project: "demo".into(),
            hub: hub.into(),
            actor: "test".into(),
            statuses: BTreeSet::from(["open".into()]),
            environment: vec![("PATH".into(), "/usr/bin:/bin".into())],
        }),
        limits: limits(),
    }
}
#[test]
fn explicit_policies_are_required_only_for_enabled_tools_and_hashed() {
    let root = tempfile::tempdir().unwrap();
    let hub = tempfile::tempdir().unwrap();
    for id in [ToolId::UrlFetch, ToolId::TrackerWrite] {
        let mut o = options(root.path(), hub.path());
        o.enabled = EnabledSet::only([id]);
        o.url_fetch = None;
        o.tracker = None;
        assert!(matches!(
            StandardTools::new(o),
            Err(crabber::ExtensionError::Plan(_))
        ));
    }
    let mut subset = options(root.path(), hub.path());
    subset.enabled = EnabledSet::only([ToolId::FileRead]);
    subset.url_fetch = None;
    subset.tracker = None;
    assert_eq!(StandardTools::new(subset).unwrap().definitions().len(), 1);
    for invalid in 0..2 {
        let mut o = options(root.path(), hub.path());
        o.enabled = EnabledSet::only([ToolId::FileRead]);
        if invalid == 0 {
            o.url_fetch
                .as_mut()
                .unwrap()
                .allow
                .push(HostPattern::Exact("EXAMPLE.COM".into()));
        } else {
            o.tracker.as_mut().unwrap().project = "--flag".into();
        }
        assert!(matches!(
            StandardTools::new(o),
            Err(crabber::ExtensionError::Plan(_))
        ));
    }
    let hash = StandardTools::new(options(root.path(), hub.path()))
        .unwrap()
        .config_hash();
    for change in 0..6 {
        let mut o = options(root.path(), hub.path());
        match change {
            0 => o.url_fetch.as_mut().unwrap().deny_private_ranges = false,
            1 => o
                .url_fetch
                .as_mut()
                .unwrap()
                .allow
                .push(HostPattern::exact("example.com").unwrap()),
            2 => o.tracker.as_mut().unwrap().actor = "other".into(),
            3 => o.tracker.as_mut().unwrap().project = "other".into(),
            4 => {
                o.tracker.as_mut().unwrap().statuses.insert("closed".into());
            }
            _ => o
                .tracker
                .as_mut()
                .unwrap()
                .environment
                .push(("LANG".into(), "C".into())),
        }
        assert_ne!(hash, StandardTools::new(o).unwrap().config_hash());
    }
    let tools = StandardTools::new(options(root.path(), hub.path())).unwrap();
    assert_eq!(tools.definitions().len(), 10);
    assert_eq!(
        tools.definitions().iter().map(|d| d.id).collect::<Vec<_>>(),
        ToolId::ALL
    );
    assert!(!ToolId::TrackerWrite.retry_safe());
    assert!(!ToolId::TrackerWrite.mutating());
    assert!(ToolId::ApplyPatch.mutating());
}
#[tokio::test]
async fn all_ten_tools_execute_on_one_agent_without_network_or_real_hub() {
    let root = tempfile::tempdir().unwrap();
    let hub = tempfile::tempdir().unwrap();
    let o = options(root.path(), hub.path());
    let file_url = url_from_path(&o.root.path().join("x"));
    let store = Arc::new(MemoryStore::new());
    let agent=Agent::builder().store(store.clone()).provider(Arc::new(FakeProvider::scripted(vec![
        turn("file_write",json!({"path":"x","content":"hello"})),
        turn("file_read",json!({"path":"x"})),
        turn("file_edit",json!({"path":"x","anchor":"hello","replacement":"world"})),
        turn("file_list",json!({})),
        turn("search",json!({"pattern":"world"})),
        turn("shell",json!({"cmd":"cat x"})),
        turn("glob",json!({"pattern":"*"})),
        turn("apply_patch",json!({"patch_text":"*** Begin Patch\n*** Add File: patched.txt\n+patched\n*** End Patch"})),
        turn("url_fetch",json!({"url":file_url})),
        turn("tracker_write",json!({"op":"comment","id":"demo-ab12","body":"done"})),done(),
    ]))).config(config(root.path())).policy(Arc::new(StaticPolicy::new(PermissionDecision::Allow))).extension(Arc::new(StandardTools::new(o).unwrap()),Scope::Global).build().unwrap();
    let run = agent.prompt(None, "ten tools").await.unwrap();
    let session = run.session_id().clone();
    run.done().await.unwrap();
    let records = calls(&store, &session).await;
    assert_eq!(records.len(), 10);
    for r in &records {
        assert_eq!(r.status, crabber::core::ToolCallStatus::Completed, "{r:?}");
        assert_eq!(value(r)["outcome"], "succeeded", "{r:?}");
    }
    assert_eq!(value(&records[8])["content"], "world");
    assert_eq!(
        std::fs::read_to_string(root.path().join("patched.txt")).unwrap(),
        "patched\n"
    );
    assert_eq!(
        std::fs::read_to_string(hub.path().join("argv")).unwrap(),
        "--project\ndemo\nnote\n--\ndemo-ab12\ndone\n"
    );
    agent.close_extensions().await.unwrap();
}
fn url_from_path(path: &Path) -> String {
    url::Url::from_file_path(path).unwrap().into()
}

#[tokio::test]
async fn tracker_and_file_url_share_mount_capacity() {
    let root = tempfile::tempdir().unwrap();
    let hub = tempfile::tempdir().unwrap();
    let mut o = options(root.path(), hub.path());
    o.limits.max_blocking_wait = std::time::Duration::from_millis(30);
    script(
        &o.tracker.as_ref().unwrap().bn_binary,
        "#!/bin/sh\ntouch \"$BEANS_HUB/started\"\nexec sleep 60\n",
    );
    let admitted = o.root.clone();
    std::fs::write(admitted.path().join("text"), "ok").unwrap();
    let tools = StandardTools::new(o).unwrap();
    let find = |id| {
        tools
            .definitions()
            .iter()
            .find(|d| d.id == id)
            .unwrap()
            .definition
            .clone()
    };
    let tracker = find(ToolId::TrackerWrite);
    let url = find(ToolId::UrlFetch);
    let cancel = tokio_util::sync::CancellationToken::new();
    let ctx = context(&admitted, cancel.clone());
    let task = tokio::spawn(async move {
        tracker
            .executor
            .execute_with_context(ctx, json!({"op":"close","id":"demo-ab12"}))
            .await
    });
    wait_file(&hub.path().join("started")).await;
    let args = json!({"url":url_from_path(&admitted.path().join("text"))});
    let result = url
        .executor
        .execute_with_context(
            context(&admitted, tokio_util::sync::CancellationToken::new()),
            args.clone(),
        )
        .await
        .unwrap();
    assert_eq!(result["error"]["category"], "unavailable");
    cancel.cancel();
    assert!(task.await.unwrap().is_err());
    let result = url
        .executor
        .execute_with_context(
            context(&admitted, tokio_util::sync::CancellationToken::new()),
            args,
        )
        .await
        .unwrap();
    assert_eq!(result["content"], "ok");
}

#[tokio::test]
async fn file_url_encodes_special_path_characters() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("percent%23path #space");
    std::fs::create_dir(&root).unwrap();
    let hub = tempfile::tempdir().unwrap();
    let mut o = options(&root, hub.path());
    o.enabled = EnabledSet::only([ToolId::UrlFetch]);
    let path = o.root.path().join("text%23 #.txt");
    std::fs::write(&path, "encoded path").unwrap();
    let admitted = o.root.clone();
    let tools = StandardTools::new(o).unwrap();
    let result = tools.definitions()[0]
        .definition
        .executor
        .execute_with_context(
            context(&admitted, tokio_util::sync::CancellationToken::new()),
            json!({"url":url_from_path(&path)}),
        )
        .await
        .unwrap();
    assert_eq!(result["content"], "encoded path");
}
