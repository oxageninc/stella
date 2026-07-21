//! Witness-isolation and failed-adoption regression tests.

use super::*;

#[tokio::test]
async fn authored_witness_with_one_candidate_uses_one_disposable_snapshot() {
    let provider = ScriptedProvider::new(vec![
        text_result("single"),
        text_result("TEST_COMMAND: cargo test authority_witness"),
        text_result("done"),
    ]);
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let workspace = FakeWorkspace::new(0, vec![false, false, true], Ok(vec![]), log.clone())
        .with_repo_status(SeqRepoStatus::new(vec![
            vec![],
            vec![("tests/authority_witness.rs", "sha256:test")],
        ]));
    let port = FakeWorkspacePort::new(vec![Ok(workspace)], log.clone());

    let (outcome, _, _) = run_isolated(
        &provider,
        &port,
        PipelineConfig {
            candidates: Some(1),
            ..PipelineConfig::default()
        },
        "Fix the failing test",
    )
    .await;
    let outcome = outcome.expect("run succeeds inside the snapshot");
    assert_eq!(outcome.status, PipelineStatus::Completed);
    assert_eq!(
        *log.lock().unwrap(),
        vec!["create", "adopt:0", "remove:0"],
        "authoring, worker verification, and adoption share one workspace"
    );
}

#[tokio::test]
async fn authored_witness_isolation_failure_aborts_before_authoring() {
    let provider = ScriptedProvider::new(vec![text_result("single")]);
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let port = FakeWorkspacePort::new(
        vec![Err(WorkspaceError::Snapshot {
            reason: "no isolated worktree".into(),
        })],
        log.clone(),
    );

    let (outcome, events, _) = run_isolated(
        &provider,
        &port,
        PipelineConfig::default(),
        "Fix the failing test",
    )
    .await;
    let outcome = outcome.expect("isolation failure is a truthful abort");
    assert!(matches!(outcome.status, PipelineStatus::Aborted { .. }));
    assert!(!stages(&events).contains(&StageKind::Witness));
    assert_eq!(*log.lock().unwrap(), vec!["create"]);
}

#[tokio::test]
async fn tracked_production_edit_by_witness_author_aborts_without_adoption() {
    let provider = ScriptedProvider::new(vec![
        text_result("single"),
        text_result("TEST_COMMAND: cargo test authority_witness"),
    ]);
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let status = SeqRepoStatus::new(vec![
        vec![],
        vec![("tests/authority_witness.rs", "sha256:test")],
    ])
    .with_tracked(vec![vec![], vec![("src/lib.rs", "sha256:mutated")]]);
    let workspace = FakeWorkspace::new(0, vec![], Ok(vec![]), log.clone()).with_repo_status(status);
    let port = FakeWorkspacePort::new(vec![Ok(workspace)], log.clone());

    let (outcome, _, _) = run_isolated(
        &provider,
        &port,
        PipelineConfig::default(),
        "Fix the failing test",
    )
    .await;
    let outcome = outcome.expect("author mutation is an aborted candidate");
    assert!(matches!(outcome.status, PipelineStatus::Aborted { .. }));
    let log = log.lock().unwrap().clone();
    assert!(!log.iter().any(|entry| entry.starts_with("adopt:")));
    assert!(log.contains(&"remove:0".to_string()));
}

#[tokio::test]
async fn failed_final_verification_never_adopts_and_removes_all_candidates() {
    let provider = ScriptedProvider::new(vec![
        text_result("single"),
        text_result("candidate zero"),
        text_result("candidate one"),
    ]);
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let port = FakeWorkspacePort::new(
        vec![
            Ok(FakeWorkspace::new(
                0,
                vec![false, false],
                Ok(vec![]),
                log.clone(),
            )),
            Ok(FakeWorkspace::new(
                1,
                vec![false, false],
                Ok(vec![]),
                log.clone(),
            )),
        ],
        log.clone(),
    );

    let (outcome, _, _) =
        run_isolated(&provider, &port, isolated_config(2), "Fix the failing test").await;
    let outcome = outcome.expect("red verification is a terminal outcome");
    assert!(matches!(
        outcome.status,
        PipelineStatus::VerificationFailed { .. }
    ));
    let log = log.lock().unwrap().clone();
    assert!(
        !log.iter().any(|entry| entry.starts_with("adopt:")),
        "{log:?}"
    );
    assert!(log.contains(&"remove:0".to_string()));
    assert!(log.contains(&"remove:1".to_string()));
}
