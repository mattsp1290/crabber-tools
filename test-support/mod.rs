//! Shared integration harness; every assertion runs through the public Crabber API.
#![allow(dead_code)]
use crabber::{
    Agent, AgentConfig, FakeProvider, PermissionDecision, Selection, StaticPolicy, StreamDelta,
    ToolDefinition,
    core::{SessionId, ToolCallId, ToolCallRecord},
    session::{MemoryStore, SnapshotLimits, SnapshotOutcome, SnapshotRequest, Store},
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
pub fn context(
    root: &crabber_tools_core::WorkspaceRoot,
    cancel: CancellationToken,
) -> crabber::extension::ToolContext {
    use crabber::{
        core::{RunId, ToolCallId},
        extension::{HostServices, ToolContext, WorkspaceContext},
    };
    ToolContext::new(
        SessionId::new(),
        RunId::new(),
        ToolCallId::new(),
        cancel,
        HostServices::default(),
        WorkspaceContext::from_persisted("test", root.path().to_str().unwrap()),
        Arc::new(|_| {}),
        None,
    )
}
pub fn limits() -> crabber_tools_core::Limits {
    crabber_tools_core::Limits {
        max_in_flight: 1,
        max_blocking_wait: Duration::from_secs(2),
    }
}
pub fn turn(name: &str, args: Value) -> Vec<StreamDelta> {
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
pub fn done() -> Vec<StreamDelta> {
    vec![
        StreamDelta::TextDelta("done".into()),
        StreamDelta::Completed,
    ]
}
pub fn config(directory: &std::path::Path) -> AgentConfig {
    let mut c = AgentConfig::new(Selection {
        provider_id: "fake".into(),
        model_id: "scripted".into(),
    });
    c.directory = directory.to_string_lossy().into_owned();
    c.workspace_id = "probe".into();
    c
}
pub async fn calls(store: &MemoryStore, id: &SessionId) -> Vec<ToolCallRecord> {
    let SnapshotOutcome::Page(page) = store
        .snapshot(SnapshotRequest {
            session_id: id.clone(),
            limits: SnapshotLimits {
                messages: 100,
                tool_calls: 100,
                parts: 100,
                text_bytes: 1_000_000,
                encoded_bytes: 2_000_000,
            },
            continuation: None,
        })
        .await
        .unwrap()
    else {
        panic!("snapshot page")
    };
    page.tool_calls
}
pub fn value(call: &ToolCallRecord) -> Value {
    let result = call.result.as_ref().unwrap();
    match &result.content[0] {
        crabber::core::ContentBlock::Text { text } => serde_json::from_str(text).unwrap(),
        other => panic!("unexpected content {other:?}"),
    }
}
pub async fn execute(root: &std::path::Path, tool: Arc<ToolDefinition>, args: Value) -> Value {
    let store = Arc::new(MemoryStore::new());
    let provider = Arc::new(FakeProvider::scripted(vec![
        turn(&tool.info.name, args),
        done(),
    ]));
    let agent = Agent::builder()
        .store(store.clone())
        .provider(provider)
        .config(config(root))
        .policy(Arc::new(StaticPolicy::new(PermissionDecision::Allow)))
        .tool(tool)
        .build()
        .unwrap();
    let run = agent.prompt(None, "test").await.unwrap();
    let session = run.session_id().clone();
    run.done().await.unwrap();
    let calls = calls(&store, &session).await;
    assert_eq!(calls.len(), 1);
    agent.close_extensions().await.unwrap();
    value(&calls[0])
}
pub fn process_running(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        && stat
            .rsplit_once(") ")
            .is_some_and(|(_, s)| s.starts_with('Z'))
    {
        return false;
    }
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
pub async fn assert_gone(pid: u32) {
    for _ in 0..200 {
        if !process_running(pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!process_running(pid), "process {pid} survived");
}
pub async fn wait_file(path: &std::path::Path) {
    for _ in 0..200 {
        if path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("missing pid file");
}
pub fn pid(path: &std::path::Path) -> u32 {
    std::fs::read_to_string(path)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}
pub fn script(path: &std::path::Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}
pub fn match_line() -> String {
    json!({"type":"match","data":{"path":{"text":"fixture"},"line_number":1,"lines":{"text":"hello\n"},"submatches":[{"match":{"text":"hello"},"start":0,"end":5}]}}).to_string()
}
