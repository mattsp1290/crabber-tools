//! Interaction contracts through public executors and scripted Agent runs.
use async_trait::async_trait;
use crabber::{ToolDefinition, extension::UserPrompter};
use crabber_tools_core::{WorkspaceRoot, config_hash};
use crabber_tools_userinteract::*;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
#[path = "../../../test-support/mod.rs"]
mod support;

struct Scripted {
    replies: Mutex<VecDeque<Result<String, String>>>,
    calls: AtomicUsize,
    questions: Mutex<Vec<String>>,
}
impl Scripted {
    fn new(replies: Vec<Result<String, String>>) -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(replies.into()),
            calls: AtomicUsize::new(0),
            questions: Mutex::new(vec![]),
        })
    }
}
#[async_trait]
impl UserPrompter for Scripted {
    async fn ask(&self, q: &str) -> Result<String, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.questions.lock().unwrap().push(q.into());
        self.replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("scripted reply")
    }
}
struct Hanging {
    started: Notify,
    dropped: Notify,
    remaining: AtomicUsize,
}
struct Dropped<'a>(&'a Notify);
impl Drop for Dropped<'_> {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}
#[async_trait]
impl UserPrompter for Hanging {
    async fn ask(&self, _: &str) -> Result<String, String> {
        if self
            .remaining
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_err()
        {
            return Ok("recovered".into());
        }
        let _guard = Dropped(&self.dropped);
        self.started.notify_one();
        std::future::pending().await
    }
}
fn hanging(n: usize) -> Arc<Hanging> {
    Arc::new(Hanging {
        started: Notify::new(),
        dropped: Notify::new(),
        remaining: AtomicUsize::new(n),
    })
}
async fn notified(n: &Notify) {
    tokio::time::timeout(Duration::from_secs(2), n.notified())
        .await
        .unwrap();
}
fn prompter(p: Arc<dyn UserPrompter>) -> UserInteractPolicy {
    UserInteractPolicy {
        surface: Surface::Prompter {
            identity: "test/prompter".into(),
            prompter: p,
        },
        ..UserInteractPolicy::pending()
    }
}
fn tool(policy: UserInteractPolicy) -> Arc<ToolDefinition> {
    definition(Arc::new(Options { policy })).unwrap()
}
async fn invoke(
    t: &ToolDefinition,
    args: Value,
    cancel: CancellationToken,
) -> Result<Value, crabber::ExtensionError> {
    let dir = tempfile::tempdir().unwrap();
    let root = WorkspaceRoot::open(dir.path()).unwrap();
    t.executor
        .execute_with_context(support::context(&root, cancel), args)
        .await
}
fn category(v: &Value, c: &str) {
    assert_eq!(v["outcome"], "failed");
    assert_eq!(v["error"]["category"], c);
}

#[test]
fn metadata() {
    let i = info();
    let mut upstream: Value = serde_json::from_str(include_str!(
        "../../../fixtures/eino-tools/user_interact.json"
    ))
    .unwrap();
    // Two intentional description edits; schemas otherwise remain identical.
    upstream["parameters"]["properties"]["answer"]["description"] =
        i.parameters["properties"]["answer"]["description"].clone();
    assert_eq!(i.parameters, upstream["parameters"]);
    assert_eq!(i.name, "user_interact");
    assert!(!i.retry_safe);
    assert_eq!(i.required_permissions, ["interaction.ask"]);
}
#[tokio::test]
async fn pending_agent_run_is_completed() {
    use crabber::{
        Agent, FakeProvider, PermissionDecision, StaticPolicy, core::ToolCallStatus,
        session::MemoryStore,
    };
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(MemoryStore::new());
    let agent = Agent::builder()
        .store(store.clone())
        .provider(Arc::new(FakeProvider::scripted(vec![
            support::turn("user_interact", json!({"question":"Q?"})),
            support::done(),
        ])))
        .config(support::config(dir.path()))
        .policy(Arc::new(StaticPolicy::new(PermissionDecision::Allow)))
        .tool(tool(UserInteractPolicy::pending()))
        .build()
        .unwrap();
    let run = agent.prompt(None, "test").await.unwrap();
    let id = run.session_id().clone();
    run.done().await.unwrap();
    let calls = support::calls(&store, &id).await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].status, ToolCallStatus::Completed);
    assert_eq!(
        support::value(&calls[0]),
        json!({"outcome":"pending","question":"Q?"})
    );
    agent.close_extensions().await.unwrap();
}
#[tokio::test]
async fn answer_precedence_is_surface_specific() {
    let args = json!({"question":"Q?","answer":"yes"});
    assert_eq!(
        invoke(
            &tool(UserInteractPolicy::pending()),
            args.clone(),
            CancellationToken::new()
        )
        .await
        .unwrap(),
        json!({"outcome":"succeeded","answer":"yes"})
    );
    let p = Scripted::new(vec![]);
    category(
        &invoke(&tool(prompter(p.clone())), args, CancellationToken::new())
            .await
            .unwrap(),
        "validation",
    );
    assert_eq!(p.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn host_answers_trim_omit_and_sanitize() {
    let p = Scripted::new(vec![
        Ok("line one\nline two\n\n".into()),
        Ok("".into()),
        Err("host detail".into()),
        Ok("x\0".into()),
        Ok("abcd".into()),
    ]);
    let mut policy = prompter(p.clone());
    let t = tool(policy.clone());
    let args = json!({"question":"Q?"});
    assert_eq!(
        invoke(&t, args.clone(), CancellationToken::new())
            .await
            .unwrap(),
        json!({"outcome":"succeeded","answer":"line one\nline two"})
    );
    assert_eq!(
        invoke(&t, args.clone(), CancellationToken::new())
            .await
            .unwrap(),
        json!({"outcome":"succeeded"})
    );
    let fail = invoke(&t, args.clone(), CancellationToken::new())
        .await
        .unwrap();
    category(&fail, "io");
    assert!(!fail.to_string().contains("host detail"));
    category(
        &invoke(&t, args.clone(), CancellationToken::new())
            .await
            .unwrap(),
        "validation",
    );
    policy.max_answer_bytes = 3;
    category(
        &invoke(&tool(policy), args, CancellationToken::new())
            .await
            .unwrap(),
        "too_large",
    );
    assert!(p.questions.lock().unwrap().iter().all(|q| q == "Q?"));
}
#[tokio::test]
async fn timeout_aborts_host() {
    let p = hanging(1);
    let mut policy = prompter(p.clone());
    policy.max_wait = Duration::from_millis(50);
    category(
        &invoke(
            &tool(policy),
            json!({"question":"Q?"}),
            CancellationToken::new(),
        )
        .await
        .unwrap(),
        "timeout",
    );
    notified(&p.dropped).await;
}
#[tokio::test]
async fn cancellation_aborts_host() {
    let p = hanging(1);
    let t = tool(prompter(p.clone()));
    let c = CancellationToken::new();
    c.cancel();
    assert!(invoke(&t, json!({"question":"Q?"}), c).await.is_err());
    assert_eq!(p.remaining.load(Ordering::SeqCst), 1);
    let c = CancellationToken::new();
    let task = tokio::spawn({
        let t = t.clone();
        let c = c.clone();
        async move { invoke(&t, json!({"question":"Q?"}), c).await }
    });
    notified(&p.started).await;
    c.cancel();
    assert!(task.await.unwrap().is_err());
    notified(&p.dropped).await;
}
#[tokio::test]
async fn dropping_tool_future_aborts_host() {
    let p = hanging(1);
    let t = tool(prompter(p.clone()));
    let task = tokio::spawn(async move {
        invoke(&t, json!({"question":"Q?"}), CancellationToken::new()).await
    });
    notified(&p.started).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    notified(&p.dropped).await;
}
#[tokio::test]
async fn outstanding_bounds_and_permit_recovery() {
    for n in [1, 2] {
        let p = hanging(n);
        let mut policy = prompter(p.clone());
        policy.max_outstanding_prompts = n;
        let t = tool(policy);
        let mut tasks = vec![];
        for _ in 0..n {
            let t = t.clone();
            let cancel = CancellationToken::new();
            let c = cancel.clone();
            tasks.push((
                cancel,
                tokio::spawn(async move { invoke(&t, json!({"question":"Q?"}), c).await }),
            ));
            notified(&p.started).await;
        }
        category(
            &tokio::time::timeout(
                Duration::from_secs(2),
                invoke(&t, json!({"question":"Q?"}), CancellationToken::new()),
            )
            .await
            .unwrap()
            .unwrap(),
            "unavailable",
        );
        for (cancel, task) in tasks {
            cancel.cancel();
            assert!(task.await.unwrap().is_err());
            notified(&p.dropped).await;
        }
        assert_eq!(
            invoke(&t, json!({"question":"Q?"}), CancellationToken::new())
                .await
                .unwrap()["answer"],
            "recovered"
        );
    }
}
#[tokio::test]
async fn arguments_are_validated_before_dispatch() {
    let mut p = UserInteractPolicy::pending();
    p.max_question_bytes = 3;
    p.max_answer_bytes = 3;
    let t = tool(p);
    for (args, c) in [
        (json!({"question":""}), "validation"),
        (json!({"question":"  "}), "validation"),
        (json!({"question":"Q","extra":1}), "validation"),
        (json!({"question":"\0"}), "validation"),
        (json!({"question":"\u{1b}"}), "validation"),
        (json!({"question":"abcd"}), "too_large"),
        (json!({"question":"Q","answer":"abcd"}), "too_large"),
        (json!({"question":"Q","answer":"\0"}), "validation"),
        (json!({"question":null}), "validation"),
        (json!({}), "validation"),
    ] {
        category(
            &invoke(&t, args, CancellationToken::new()).await.unwrap(),
            c,
        );
    }
    category(
        &t.executor.execute(json!({"question":"Q"})).await.unwrap(),
        "validation",
    );
    assert_eq!(
        invoke(&t, json!({"question":"Q\n\t"}), CancellationToken::new())
            .await
            .unwrap()["outcome"],
        "pending"
    );
}
struct PanicOnce(AtomicUsize);
#[async_trait]
impl UserPrompter for PanicOnce {
    async fn ask(&self, _: &str) -> Result<String, String> {
        if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("host panic");
        }
        Ok("yes".into())
    }
}
#[tokio::test]
async fn panics_are_isolated_and_release_permit() {
    let dir = tempfile::tempdir().unwrap();
    let t = tool(prompter(Arc::new(PanicOnce(AtomicUsize::new(0)))));
    category(
        &support::execute(dir.path(), t.clone(), json!({"question":"Q?"})).await,
        "unknown",
    );
    assert_eq!(
        invoke(&t, json!({"question":"Q?"}), CancellationToken::new())
            .await
            .unwrap()["answer"],
        "yes"
    );
}
#[test]
fn policy_validation_and_identity() {
    let valid = UserInteractPolicy::pending();
    valid.validate().unwrap();
    for (q, a, w, n) in [
        (0, 1, 1, 1),
        (MAX_QUESTION_BYTES + 1, 1, 1, 1),
        (1, 0, 1, 1),
        (1, MAX_ANSWER_BYTES + 1, 1, 1),
        (1, 1, 0, 1),
        (1, 1, 601, 1),
        (1, 1, 1, 0),
        (1, 1, 1, 17),
    ] {
        let mut p = valid.clone();
        p.max_question_bytes = q;
        p.max_answer_bytes = a;
        p.max_wait = Duration::from_secs(w);
        p.max_outstanding_prompts = n;
        assert!(p.validate().is_err());
    }
    let base = prompter(Scripted::new(vec![]));
    for identity in ["", "-bad", "has space", "é", "x\0", &"x".repeat(129)] {
        let mut p = base.clone();
        if let Surface::Prompter { identity: id, .. } = &mut p.surface {
            *id = identity.into();
        }
        assert!(p.validate().is_err());
    }
    base.validate().unwrap();
    assert_ne!(config_hash(&valid), config_hash(&base));
    assert_eq!(
        config_hash(&base),
        config_hash(&prompter(Scripted::new(vec![])))
    );
    let mut p = base.clone();
    if let Surface::Prompter { identity, .. } = &mut p.surface {
        *identity = "other".into();
    }
    assert_ne!(config_hash(&base), config_hash(&p));
    let mut p = base.clone();
    p.max_wait = Duration::from_secs(1);
    assert_ne!(config_hash(&base), config_hash(&p));
    let mut p = base.clone();
    p.max_outstanding_prompts = 2;
    assert_ne!(config_hash(&base), config_hash(&p));
    for question in [true, false] {
        let mut p = base.clone();
        if question {
            p.max_question_bytes -= 1;
        } else {
            p.max_answer_bytes -= 1;
        }
        assert_ne!(config_hash(&base), config_hash(&p));
    }
}
#[tokio::test]
async fn upstream_run_cases() {
    use sha2::{Digest, Sha256};
    let cases: Vec<Value> = serde_json::from_str(include_str!("fixtures/go-cases.json")).unwrap();
    assert_eq!(
        cases[0]["upstream_sha256"],
        format!(
            "{:x}",
            Sha256::digest(include_bytes!("fixtures/upstream_userinteract_test.go"))
        )
    );
    for case in &cases[1..] {
        let p = Scripted::new(vec![if case["stdin_error"] == true {
            Err("fake read error".into())
        } else {
            Ok(case["stdin_lines"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|v| v.as_str().unwrap())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default())
        }]);
        let policy = if case["surface"] == "cli" {
            prompter(p.clone())
        } else {
            UserInteractPolicy::pending()
        };
        let result = invoke(
            &tool(policy),
            case["args"].clone(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        let e = &case["expected"];
        assert_eq!(result["outcome"], e["outcome"], "{}", case["name"]);
        assert_eq!(
            result.get("answer").is_some(),
            e["answer_key_present"].as_bool().unwrap(),
            "{}",
            case["name"]
        );
        for k in ["answer", "question"] {
            if let Some(v) = e.get(k) {
                assert_eq!(&result[k], v, "{}", case["name"]);
            }
        }
        if let Some(c) = e.get("category") {
            assert_eq!(&result["error"]["category"], c, "{}", case["name"]);
        }
        if case["surface"] == "cli" && result["outcome"] != "failed" {
            assert_eq!(
                p.questions.lock().unwrap().as_slice(),
                [case["args"]["question"].as_str().unwrap()]
            );
        }
    }
}
#[test]
fn no_process_io() {
    for source in [
        include_str!("../src/lib.rs"),
        include_str!("../src/info.rs"),
        include_str!("../src/policy.rs"),
        include_str!("../src/run.rs"),
    ] {
        for token in ["stdin(", "stderr("] {
            assert!(!source.contains(token));
        }
    }
}

#[tokio::test]
async fn aborted_host_holds_capacity_until_dropped() {
    use std::{
        future::Future,
        task::{Context, Poll, Waker},
    };
    for cancel_explicitly in [true, false] {
        let p = hanging(1);
        let t = tool(prompter(p.clone()));
        let cancel = CancellationToken::new();
        let mut first = Box::pin(invoke(&t, json!({"question":"Q?"}), cancel.clone()));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(first.as_mut().poll(&mut cx).is_pending());
        notified(&p.started).await;
        if cancel_explicitly {
            cancel.cancel();
            assert!(matches!(first.as_mut().poll(&mut cx), Poll::Ready(Err(_))));
        }
        drop(first);
        // No yield: Tokio has not processed the child's abort yet.
        let mut replacement = Box::pin(invoke(
            &t,
            json!({"question":"Q?"}),
            CancellationToken::new(),
        ));
        let Poll::Ready(Ok(result)) = replacement.as_mut().poll(&mut cx) else {
            panic!("replacement admitted before the old host future was dropped");
        };
        category(&result, "unavailable");
        drop(replacement);
        notified(&p.dropped).await;
        assert_eq!(
            invoke(&t, json!({"question":"Q?"}), CancellationToken::new())
                .await
                .unwrap()["answer"],
            "recovered"
        );
    }
}
