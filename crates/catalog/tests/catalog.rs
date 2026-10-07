//! Catalog identities, independent parity fixtures, and mounting semantics.
#[path = "../../../examples/mount-standard/src/probe.rs"]
mod probe;
#[path = "../../../test-support/mod.rs"]
mod support;
use crabber::{
    Agent, ExtensionError, FakeProvider, PermissionDecision, StaticPolicy, ToolDefinition,
    ToolExecutor,
    core::{ToolCallStatus, ToolInfo},
    extension::{Extension, Scope},
    session::MemoryStore,
};
use crabber_tools_catalog::prelude::*;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use support::*;
fn options(path: &std::path::Path) -> Options {
    Options {
        root: WorkspaceRoot::open(path).unwrap(),
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
        limits: Limits {
            max_in_flight: 4,
            max_blocking_wait: Duration::from_secs(2),
        },
    }
}
#[test]
fn metadata_parity_docs_hashes_and_prelude() {
    let first = metadata();
    assert_eq!(first, metadata());
    // Initial first-deliverable schemas, including the explicit policy description deviations.
    let hashes: Vec<String> = serde_json::from_str(include_str!("hashes.json")).unwrap();
    assert_eq!(
        first.iter().map(|x| x.2.clone()).collect::<Vec<_>>(),
        hashes
    );
    let doc = include_str!("../../../docs/tool-contract.md");
    let allowed = include_str!("../../../fixtures/eino-tools/ALLOWED-DIFFS.md");
    let allowed: Vec<Value> = serde_json::from_str(
        allowed
            .split("```json\n")
            .nth(1)
            .unwrap()
            .split("```")
            .next()
            .unwrap(),
    )
    .unwrap();
    let mut used = 0;
    for (id, info, _) in first {
        assert_eq!(info.name, id.name());
        assert_eq!(info.retry_safe, id.retry_safe());
        assert_eq!(id.mutating(), !info.retry_safe);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../fixtures/eino-tools/{}.json", info.name));
        let mut golden: Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for d in allowed.iter().filter(|d| d["tool"] == info.name) {
            let slot = golden.pointer_mut(d["pointer"].as_str().unwrap()).unwrap();
            assert_eq!(*slot, d["upstream"]);
            *slot = d["rust"].clone();
            used += 1;
        }
        assert_eq!(
            golden,
            json!({"name":info.name,"description":info.description,"parameters":info.parameters})
        );
        let section = doc
            .split(&format!("## {}\n", info.name))
            .nth(1)
            .unwrap()
            .split("\n## ")
            .next()
            .unwrap();
        let schema: Value = serde_json::from_str(
            section
                .split("```json\n")
                .nth(1)
                .unwrap()
                .split("```")
                .next()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(schema, info.parameters);
    }
    assert_eq!(used, allowed.len());
    // These names compile from the prelude alone.
    let _: Option<(Definition, RelPath, RunAs)> = None;
}
#[test]
fn hashes_cover_policies_root_enabled_and_restrictions() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let hash = StandardTools::new(options(a.path())).unwrap().config_hash();
    assert_eq!(
        hash,
        StandardTools::new(options(a.path())).unwrap().config_hash()
    );
    assert_ne!(
        hash,
        StandardTools::new(options(b.path())).unwrap().config_hash()
    );
    for change in 0..4 {
        let mut o = options(a.path());
        match change {
            0 => o.shell.env = EnvPolicy::Replace(vec![]),
            1 => o.shell.run_as = Some(RunAs { uid: 123, gid: 456 }),
            2 => o.restrict_to_enabled = true,
            _ => o.enabled = EnabledSet::only([ToolId::FileRead]),
        };
        assert_ne!(hash, StandardTools::new(o).unwrap().config_hash());
    }
    let mut o = options(a.path());
    o.enabled = EnabledSet::only([]);
    assert!(StandardTools::new(o).is_err());
}
struct Host;
#[async_trait::async_trait]
impl ToolExecutor for Host {
    async fn execute(&self, _: Value) -> Result<Value, ExtensionError> {
        Ok(json!({"host":true}))
    }
}
fn host() -> Arc<ToolDefinition> {
    Arc::new(ToolDefinition {
        info: ToolInfo {
            name: "finish_job".into(),
            description: "test host tool".into(),
            parameters: json!({"type":"object"}),
            retry_safe: false,
            required_permissions: vec![],
        },
        executor: Arc::new(Host),
    })
}
#[tokio::test]
async fn allowlist_and_run_wide_restriction() {
    let tmp = tempfile::tempdir().unwrap();
    for restrict in [false, true] {
        let mut o = options(tmp.path());
        o.enabled = EnabledSet::only([ToolId::FileRead]);
        o.restrict_to_enabled = restrict;
        let tools = StandardTools::new(o).unwrap();
        assert_eq!(tools.definitions().len(), 1);
        let store = Arc::new(MemoryStore::new());
        let agent = Agent::builder()
            .store(store.clone())
            .provider(Arc::new(FakeProvider::scripted(vec![
                turn("shell", json!({"cmd":"true"})),
                turn("finish_job", json!({})),
                done(),
            ])))
            .config(config(tmp.path()))
            .policy(Arc::new(StaticPolicy::new(PermissionDecision::Allow)))
            .extension(Arc::new(tools), Scope::Global)
            .tool(host())
            .build()
            .unwrap();
        let run = agent.prompt(None, "allowlist").await.unwrap();
        let session = run.session_id().clone();
        run.done().await.unwrap();
        let records = calls(&store, &session).await;
        assert_eq!(records[0].status, ToolCallStatus::Failed);
        assert_eq!(
            records[1].status,
            if restrict {
                ToolCallStatus::Failed
            } else {
                ToolCallStatus::Completed
            }
        );
        agent.close_extensions().await.unwrap();
    }
}
#[tokio::test]
async fn collision_propagates() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let agent = Agent::builder()
        .memory()
        .provider(Arc::new(FakeProvider::scripted(vec![done()])))
        .config(config(a.path()))
        .extension(
            Arc::new(StandardTools::new(options(a.path())).unwrap()),
            Scope::Global,
        )
        .extension(
            Arc::new(StandardTools::new(options(b.path())).unwrap()),
            Scope::Global,
        )
        .build()
        .unwrap();
    let error = match agent.prompt(None, "collision").await {
        Ok(run) => match run.done().await {
            Ok(_) => panic!("collision accepted"),
            Err(e) => e,
        },
        Err(e) => e,
    };
    assert!(error.to_string().contains("tool name collision"), "{error}");
    agent.close_extensions().await.unwrap();
}
#[tokio::test]
async fn all_six_tools_execute_and_probe() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(MemoryStore::new());
    let agent = Agent::builder()
        .store(store.clone())
        .provider(Arc::new(FakeProvider::scripted(vec![
            turn("file_write", json!({"path":"x","content":"hello"})),
            turn("file_read", json!({"path":"x"})),
            turn(
                "file_edit",
                json!({"path":"x","anchor":"hello","replacement":"world"}),
            ),
            turn("file_list", json!({})),
            turn("search", json!({"pattern":"world"})),
            turn("shell", json!({"cmd":"cat x"})),
            done(),
        ])))
        .config(config(tmp.path()))
        .policy(Arc::new(StaticPolicy::new(PermissionDecision::Allow)))
        .extension(
            Arc::new(StandardTools::new(options(tmp.path())).unwrap()),
            Scope::Global,
        )
        .build()
        .unwrap();
    let run = agent.prompt(None, "six tools").await.unwrap();
    let session = run.session_id().clone();
    run.done().await.unwrap();
    let records = calls(&store, &session).await;
    assert_eq!(records.len(), 6);
    for r in &records {
        assert_eq!(r.status, ToolCallStatus::Completed);
        assert_eq!(value(r)["outcome"], "succeeded");
    }
    assert_eq!(value(&records[5])["stdout"], "world");
    agent.close_extensions().await.unwrap();
    probe::run(false, None).await.unwrap();
    probe::run(true, None).await.unwrap();
}

#[test]
fn consumer_probe_copies_stay_in_sync() {
    assert_eq!(
        include_str!("../../../examples/mount-standard/src/probe.rs"),
        include_str!("../../../ci/consumer-probe/src/probe.rs"),
        "update both consumer-shaped probe copies together",
    );
}
