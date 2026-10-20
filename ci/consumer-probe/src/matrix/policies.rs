//! Observe preserved policies in factory-created workspaces without network I/O.
use super::durable::{done, results, turn, value};
use crabber::{
    Admission, Agent, AgentConfig, ExecutionContext, FakeProvider, PermissionDecision, Selection,
    SessionContext, StaticPolicy,
    core::{
        AdmissionKey, AdmissionOptions, ControlId, InputFingerprint, SessionId, ToolCallStatus,
    },
    extension::Scope,
    session::MemoryStore,
};
use crabber_tools_catalog::prelude::*;
use serde_json::json;
use std::{sync::Arc, time::Duration};
pub(super) fn policies(enabled: EnabledSet) -> Result<Policies, Box<dyn std::error::Error>> {
    Ok(Policies {
        enabled,
        restrict_to_enabled: false,
        shell: ShellPolicy {
            shell_binary: "/bin/sh".into(),
            startup_mode: StartupMode::NonLogin,
            env: EnvPolicy::minimal_allowlist(),
            run_as: None,
            output_cap_bytes: 16,
        },
        search: SearchPolicy::resolve_rg_from_path(EnvPolicy::minimal_allowlist())?,
        url_fetch: Some(UrlFetchPolicy {
            allow: vec![HostPattern::exact("allowed.invalid")?],
            ..UrlFetchPolicy::default()
        }),
        tracker: None,
        user_interact: None,
        limits: Limits {
            max_in_flight: 1,
            max_blocking_wait: Duration::from_millis(100),
        },
    })
}
pub(super) async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let a = tempfile::tempdir()?;
    let b = tempfile::tempdir()?;
    for root in [a.path(), b.path()] {
        std::fs::write(root.join("x"), "owned")?;
    }
    for mode in 0..4 {
        let store = Arc::new(MemoryStore::new());
        let mut p = policies(if mode < 2 {
            EnabledSet::only([ToolId::FileRead])
        } else {
            EnabledSet::only([ToolId::FileRead, ToolId::Shell, ToolId::UrlFetch])
        })?;
        p.restrict_to_enabled = mode == 1;
        let script = (0..2)
            .flat_map(|_| {
                let mut turns = vec![
                    turn(
                        "shell",
                        if mode == 3 {
                            json!({"cmd":"touch forbidden"})
                        } else {
                            json!({"cmd":"printf 12345678901234567890"})
                        },
                    ),
                    turn("file_read", json!({"path":"x"})),
                    turn("consumer_marker", json!({})),
                ];
                if mode >= 2 {
                    turns.push(turn("url_fetch", json!({"url":"https://example.com/"})));
                }
                turns.push(done());
                turns
            })
            .collect();
        let roots = ExactRoots::new([
            ("a".into(), a.path().canonicalize()?),
            ("b".into(), b.path().canonicalize()?),
        ])?;
        let permission = if mode == 3 {
            StaticPolicy::new(PermissionDecision::Allow)
                .with_rule("shell", PermissionDecision::Deny)
        } else {
            StaticPolicy::new(PermissionDecision::Allow)
        };
        let agent = Agent::builder()
            .behavior_version("consumer-policies/0.4.0")
            .store(store.clone())
            .provider(Arc::new(FakeProvider::scripted(script)))
            .config(AgentConfig::new(Selection {
                provider_id: "fake".into(),
                model_id: "scripted".into(),
            }))
            .policy(Arc::new(permission))
            .extension_factory(Arc::new(StandardToolsFactory::new(p, Arc::new(roots))?))
            .shared_extension(Arc::new(crate::Marker), Scope::Global)
            .build()?;
        for (workspace, root) in [("a", a.path()), ("b", b.path())] {
            let session = SessionId::new();
            let ctx = ExecutionContext::new(
                SessionContext::new(session.clone(), workspace, root.to_str().unwrap()),
                Selection {
                    provider_id: "fake".into(),
                    model_id: "scripted".into(),
                },
            );
            let options = AdmissionOptions {
                key: AdmissionKey::new(ControlId::new().0)?,
                fingerprint: InputFingerprint::new("a".repeat(64))?,
                behavior_fingerprint: agent.behavior_fingerprint().clone(),
            };
            let Admission::Started { handle, .. } = agent
                .prompt_keyed_in(&ctx, "policies", options, None)
                .await?
            else {
                return Err("fresh admission replayed".into());
            };
            handle.done().await?;
            let records = results(&*store, &session).await?;
            assert_eq!(
                records[0].status,
                if mode == 2 {
                    ToolCallStatus::Completed
                } else {
                    ToolCallStatus::Failed
                }
            );
            if mode == 2 {
                assert_eq!(value(&records[0])["stdout"], "1234567890123456");
                assert_eq!(value(&records[0])["stdout_truncated"], true);
            }
            assert_eq!(value(&records[1])["content"], "owned");
            assert_eq!(
                records[2].status,
                if mode == 1 {
                    ToolCallStatus::Failed
                } else {
                    ToolCallStatus::Completed
                }
            );
            if mode == 3 {
                assert!(!root.join("forbidden").exists());
            }
            if mode >= 2 {
                assert_eq!(records[3].status, ToolCallStatus::Failed);
                assert_eq!(value(&records[3])["error"]["category"], "validation");
            }
        }
        agent.close_extensions().await?;
    }
    println!(
        "{}",
        json!({"check":"factory_allowlist_restriction_host_deny_shell_cap_and_url_policy","ok":true})
    );
    Ok(())
}
