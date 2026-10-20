use super::*;
pub(super) async fn restart(sqlite: bool) -> Result<(), Error> {
    use std::os::unix::fs::MetadataExt;
    let root = tempfile::tempdir()?;
    let database = tempfile::tempdir()?;
    let url = database.path().join("state.sqlite");
    let store: Arc<dyn Store> = if sqlite {
        SqliteStore::migrate(&url).await?;
        let store = SqliteStore::connect(&url).await?;
        Arc::new(store)
    } else {
        Arc::new(MemoryStore::new())
    };
    let mut batch = turn("file_write", json!({"path":"effect","content":"once"}));
    batch.pop();
    batch.extend(turn("user_interact", json!({"question":"First?"})));
    batch.pop();
    batch.extend(turn("user_interact", json!({"question":"Second?"})));
    let provider = Arc::new(FakeProvider::scripted(vec![batch, done()]));
    let agent = host(root.path(), store.clone(), provider.clone(), policy())?;
    let snapshot = paused(&agent).await?;
    assert_eq!(snapshot.interactions.len(), 2);
    let inode = std::fs::metadata(root.path().join("effect"))?.ino();
    let first = agent
        .answer_tool_interaction(answer(
            &snapshot,
            0,
            ToolInteractionReply::Answer {
                value: json!("one"),
            },
        ))
        .await?;
    assert!(
        agent
            .continue_tool(continuation(&snapshot, first.revision))
            .await
            .is_err()
    );
    let mut fresh = agent
        .tool_continuation_snapshot(&snapshot.session_id, &snapshot.run_id)
        .await?
        .unwrap();
    fresh.revision = first.revision;
    let accepted = agent
        .answer_tool_interaction(answer(
            &fresh,
            1,
            ToolInteractionReply::Answer {
                value: json!("two"),
            },
        ))
        .await?;
    agent.close_extensions().await?;
    drop(agent);
    let store = if sqlite {
        drop(store);
        Arc::new(SqliteStore::connect(&url).await?) as Arc<dyn Store>
    } else {
        store
    };
    let rebuilt = host(root.path(), store.clone(), provider, policy())?;
    finish(&rebuilt, &snapshot, accepted.revision).await?;
    assert_eq!(std::fs::metadata(root.path().join("effect"))?.ino(), inode);
    assert_eq!(std::fs::read_to_string(root.path().join("effect"))?, "once");
    let records = results(&*store, &snapshot.session_id).await?;
    assert_eq!(records.iter().filter(|r| r.name == "file_write").count(), 1);
    assert!(
        records
            .iter()
            .all(|r| r.status == ToolCallStatus::Completed)
    );
    rebuilt.close_extensions().await?;
    check(if sqlite {
        "sqlite_reopen_parallel_resume_effect_once"
    } else {
        "memory_restart_parallel_resume_effect_once"
    });
    Ok(())
}
