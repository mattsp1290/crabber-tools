//! Consumer-shaped, credential-free scripted host integration.
use async_trait::async_trait;
use crabber::{
    Agent, AgentConfig, ExtensionError, FakeProvider, PermissionDecision, Selection, StaticPolicy,
    StreamDelta, ToolDefinition, ToolExecutor,
    core::{ContentBlock, EventKind, ToolCallId, ToolCallStatus, ToolInfo},
    extension::{Extension, Registrar, Scope},
    session::{MemoryStore, SnapshotLimits, SnapshotOutcome, SnapshotRequest, Store},
};
use crabber_tools_catalog::prelude::*;
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
fn turn(name: &str, args: Value) -> Vec<StreamDelta> {
    let id = ToolCallId::new();
    vec![
        StreamDelta::ToolCallStart {
            call_id: id.clone(),
            name: name.into(),
        },
        StreamDelta::ToolCallArgsDelta {
            call_id: id.clone(),
            text: args.to_string(),
        },
        StreamDelta::ToolCallDone { call_id: id },
        StreamDelta::Completed,
    ]
}
struct Record(Arc<Mutex<Vec<Value>>>);
#[async_trait]
impl ToolExecutor for Record {
    async fn execute(&self, args: Value) -> Result<Value, ExtensionError> {
        self.0.lock().unwrap().push(args);
        Ok(json!({"recorded":true}))
    }
}
struct HostExtension(Arc<Mutex<Vec<Value>>>);
fn record_tool(name: &str, record: Arc<Mutex<Vec<Value>>>) -> Arc<ToolDefinition> {
    Arc::new(ToolDefinition {
        info: ToolInfo {
            name: name.into(),
            description: "Record a synthetic host payload".into(),
            parameters: json!({"type":"object","properties":{"label":{"type":"string"},"payload":{}},"additionalProperties":false}),
            retry_safe: false,
            required_permissions: vec![],
        },
        executor: Arc::new(Record(record)),
    })
}
#[async_trait]
impl Extension for HostExtension {
    fn id(&self) -> &str {
        "example/host"
    }
    fn version(&self) -> &str {
        "1"
    }
    fn config_hash(&self) -> String {
        "host-v1".into()
    }
    async fn install(&self, r: &mut Registrar) -> Result<(), ExtensionError> {
        r.tool(record_tool("finish_job", self.0.clone()));
        Ok(())
    }
}
/// Mount a per-node subset beside a host extension and a plain host tool.
/// Optionally add a real external extension (the consumer-probe uses CommandGuard).
pub async fn run(
    deny_shell: bool,
    extra: Option<Arc<dyn Extension>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    std::fs::write(temp.path().join("README.md"), "known-line\n")?;
    std::fs::write(temp.path().join("AGENTS.md"), "Use the standard tools.\n")?;
    let root = WorkspaceRoot::open(temp.path())?;
    let tools = StandardTools::new(Options {
        root: root.clone(),
        enabled: EnabledSet::only([
            ToolId::FileRead,
            ToolId::FileWrite,
            ToolId::FileEdit,
            ToolId::Search,
            ToolId::Shell,
        ]),
        restrict_to_enabled: false,
        shell: ShellPolicy {
            shell_binary: "/bin/sh".into(),
            startup_mode: StartupMode::NonLogin,
            env: EnvPolicy::minimal_allowlist(),
            run_as: None,
            output_cap_bytes: 65536,
        },
        search: SearchPolicy::resolve_rg_from_path(EnvPolicy::minimal_allowlist())?,
        limits: Limits {
            max_in_flight: 4,
            max_blocking_wait: Duration::from_secs(2),
        },
    })?;
    let mut script = vec![
        turn(
            "file_write",
            json!({"path":"notes.txt","content":"hello world\n"}),
        ),
        turn("search", json!({"pattern":"known-line","path":"README.md"})),
        turn("shell", json!({"cmd":"cat notes.txt"})),
        turn(
            "file_edit",
            json!({"path":"notes.txt","anchor":"world","replacement":"Crabber"}),
        ),
        turn("file_list", json!({})),
        turn("finish_job", json!({"label":"done","payload":{"ok":true}})),
    ];
    if extra.is_some() {
        script.push(turn("shell", json!({"cmd":"git push origin"})));
        script.push(turn("shell", json!({"cmd":"echo ok"})));
    }
    script.push(vec![
        StreamDelta::TextDelta("done".into()),
        StreamDelta::Completed,
    ]);
    let record = Arc::new(Mutex::new(vec![]));
    let store = Arc::new(MemoryStore::new());
    let mut config = AgentConfig::new(Selection {
        provider_id: "fake".into(),
        model_id: "scripted".into(),
    });
    config.workspace_id = "probe".into();
    config.directory = root.path().to_string_lossy().into_owned();
    let policy = if deny_shell {
        StaticPolicy::new(PermissionDecision::Allow).with_rule("shell", PermissionDecision::Deny)
    } else {
        StaticPolicy::new(PermissionDecision::Allow)
    };
    let has_extra = extra.is_some();
    let mut builder = Agent::builder()
        .store(store.clone())
        .provider(Arc::new(FakeProvider::scripted(script)))
        .config(config)
        .policy(Arc::new(policy))
        .extension(Arc::new(tools), Scope::Global)
        .extension(Arc::new(HostExtension(record.clone())), Scope::Global)
        .tool(record_tool("fixture", Arc::new(Mutex::new(vec![]))));
    if let Some(extension) = extra {
        builder = builder.extension(extension, Scope::Global);
    }
    let agent = builder.build()?;
    let mut run = agent.prompt(None, "Exercise the tools").await?;
    let session = run.session_id().clone();
    let mut events = run.events();
    let mut settled = 0;
    while let Some(event) = events.recv().await? {
        if matches!(
            event.kind,
            EventKind::ToolCallStarted | EventKind::ToolCallSettled
        ) {
            println!("{}", serde_json::to_string(&*event)?);
            if event.kind == EventKind::ToolCallSettled {
                settled += 1;
            }
        }
    }
    run.done().await?;
    let SnapshotOutcome::Page(page) = store
        .snapshot(SnapshotRequest {
            session_id: session,
            limits: SnapshotLimits {
                messages: 100,
                tool_calls: 100,
                parts: 100,
                text_bytes: 1_000_000,
                encoded_bytes: 2_000_000,
            },
            continuation: None,
        })
        .await?
    else {
        panic!("probe snapshot page required")
    };
    assert_eq!(page.tool_calls.len(), if has_extra { 8 } else { 6 });
    assert!(settled >= 6);
    for (i, call) in page.tool_calls.iter().enumerate() {
        let expected = if i == 4 || ((i == 2 || i == 7) && deny_shell) || i == 6 {
            ToolCallStatus::Failed
        } else {
            ToolCallStatus::Completed
        };
        assert_eq!(call.status, expected, "{}", call.name);
    }
    let value = |i: usize| -> Value {
        match &page.tool_calls[i].result.as_ref().unwrap().content[0] {
            ContentBlock::Text { text } => serde_json::from_str(text).unwrap(),
            v => panic!("unexpected {v:?}"),
        }
    };
    assert_eq!(value(0)["bytes_written"], 12);
    assert_eq!(value(1)["match_count"], 1);
    assert_eq!(value(3)["anchor_occurrences"], 1);
    if !deny_shell {
        assert_eq!(value(2)["stdout"], "hello world\n");
        if has_extra {
            assert_eq!(value(7)["stdout"], "ok\n");
        }
    }
    assert_eq!(
        *record.lock().unwrap(),
        vec![json!({"label":"done","payload":{"ok":true}})]
    );
    assert_eq!(
        std::fs::read_to_string(temp.path().join("notes.txt"))?,
        "hello Crabber\n"
    );
    agent.close_extensions().await?;
    Ok(())
}
