//! Integration tests for ruci-core
//!
//! Tests the end-to-end flow: job creation, run lifecycle, log storage, VCS credentials.

use std::collections::HashMap;
use std::sync::Arc;

use ruci_core::db::create_repository;
use ruci_core::db::Repository;
use ruci_core::executor::{BashExecutor, ExecutionContext, Executor, Job};
use ruci_protocol::{JobInfo, RunStatus};

/// Helper to create an in-memory SQLite repository with migrations applied.
async fn setup_db() -> Arc<dyn Repository> {
    let repo = create_repository("sqlite::memory:").await.unwrap();
    repo.migrate().await.unwrap();
    repo
}

/// Helper to create a test job.
fn test_job(id: &str, name: &str) -> JobInfo {
    JobInfo {
        id: id.to_string(),
        name: name.to_string(),
        original_name: ".ruci.yml".to_string(),
        submitted_at: chrono::Utc::now(),
    }
}

// ─────────────────────────────────────────────────────────────────
// Job lifecycle
// ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_job_insert_and_retrieve_preserves_submitted_at() {
    let db = setup_db().await;
    let job = test_job("job-1", "build-project");

    db.insert_job(&job).await.unwrap();
    let retrieved = db.get_job("job-1").await.unwrap().unwrap();

    assert_eq!(retrieved.id, "job-1");
    assert_eq!(retrieved.name, "build-project");

    // submitted_at should be close to now, not epoch 0
    let now = chrono::Utc::now();
    let diff = now.signed_duration_since(retrieved.submitted_at);
    assert!(
        diff.num_seconds() < 5,
        "submitted_at should be recent, got diff={}s",
        diff.num_seconds()
    );
}

#[tokio::test]
async fn test_list_jobs_returns_all() {
    let db = setup_db().await;

    for i in 0..5 {
        db.insert_job(&test_job(&format!("job-{}", i), &format!("job-{}", i)))
            .await
            .unwrap();
    }

    let jobs = db.list_jobs().await.unwrap();
    assert_eq!(jobs.len(), 5);
}

#[tokio::test]
async fn test_next_build_num_increments() {
    let db = setup_db().await;
    db.insert_job(&test_job("job-1", "test")).await.unwrap();

    let n1 = db.next_build_num("job-1").await.unwrap();
    assert_eq!(n1, 1);

    db.insert_run("run-1", "job-1", 1, "QUEUED", None)
        .await
        .unwrap();

    let n2 = db.next_build_num("job-1").await.unwrap();
    assert_eq!(n2, 2);
}

// ─────────────────────────────────────────────────────────────────
// Run lifecycle
// ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_run_full_lifecycle_with_exit_code() {
    let db = setup_db().await;
    db.insert_job(&test_job("job-1", "test")).await.unwrap();

    // Queue
    db.insert_run("run-1", "job-1", 1, "QUEUED", None)
        .await
        .unwrap();

    let run = db.get_run("run-1").await.unwrap().unwrap();
    assert_eq!(run.status, RunStatus::Queued);
    assert!(run.exit_code.is_none());

    // Start running
    db.update_run_status("run-1", "RUNNING", None)
        .await
        .unwrap();

    let run = db.get_run("run-1").await.unwrap().unwrap();
    assert_eq!(run.status, RunStatus::Running);
    assert!(run.exit_code.is_none());

    // Complete with exit code 0
    db.update_run_status("run-1", "SUCCESS", Some(0))
        .await
        .unwrap();

    let run = db.get_run("run-1").await.unwrap().unwrap();
    assert_eq!(run.status, RunStatus::Success);
    assert_eq!(run.exit_code, Some(0));
}

#[tokio::test]
async fn test_run_failure_exit_code() {
    let db = setup_db().await;
    db.insert_job(&test_job("job-1", "test")).await.unwrap();
    db.insert_run("run-1", "job-1", 1, "QUEUED", None)
        .await
        .unwrap();
    db.update_run_status("run-1", "RUNNING", None)
        .await
        .unwrap();

    // Fail with exit code 1
    db.update_run_status("run-1", "FAILED", Some(1))
        .await
        .unwrap();

    let run = db.get_run("run-1").await.unwrap().unwrap();
    assert_eq!(run.status, RunStatus::Failed);
    assert_eq!(run.exit_code, Some(1));
}

#[tokio::test]
async fn test_run_abort_exit_code() {
    let db = setup_db().await;
    db.insert_job(&test_job("job-1", "test")).await.unwrap();
    db.insert_run("run-1", "job-1", 1, "QUEUED", None)
        .await
        .unwrap();
    db.update_run_status("run-1", "RUNNING", None)
        .await
        .unwrap();

    // Abort
    db.update_run_status("run-1", "ABORTED", Some(-1))
        .await
        .unwrap();

    let run = db.get_run("run-1").await.unwrap().unwrap();
    assert_eq!(run.status, RunStatus::Aborted);
    assert_eq!(run.exit_code, Some(-1));
}

#[tokio::test]
async fn test_list_runs_by_status() {
    let db = setup_db().await;
    db.insert_job(&test_job("job-1", "test")).await.unwrap();

    // Create runs in different states
    db.insert_run("run-q1", "job-1", 1, "QUEUED", None)
        .await
        .unwrap();
    db.insert_run("run-q2", "job-1", 2, "QUEUED", None)
        .await
        .unwrap();
    db.insert_run("run-r1", "job-1", 3, "RUNNING", None)
        .await
        .unwrap();

    let queued = db.list_runs_by_status("QUEUED").await.unwrap();
    assert_eq!(queued.len(), 2);

    let running = db.list_runs_by_status("RUNNING").await.unwrap();
    assert_eq!(running.len(), 1);
}

#[tokio::test]
async fn test_run_params_persistence() {
    let db = setup_db().await;
    db.insert_job(&test_job("job-1", "test")).await.unwrap();

    let params_json = r#"{"branch":"main","env":"prod"}"#;
    db.insert_run("run-1", "job-1", 1, "QUEUED", Some(params_json))
        .await
        .unwrap();

    let params = db.get_run_params("run-1").await.unwrap();
    assert_eq!(params.get("branch").unwrap(), "main");
    assert_eq!(params.get("env").unwrap(), "prod");
}

// ─────────────────────────────────────────────────────────────────
// Log file operations
// ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_log_file_write_and_read() {
    let tmp = tempfile::tempdir().unwrap();
    let run_dir = tmp.path().to_str().unwrap();
    let log_dir = format!("{}/job-1/run-1", run_dir);
    let log_path = format!("{}/output.log", log_dir);

    std::fs::create_dir_all(&log_dir).unwrap();
    let log_content = "Step 1 output\nStep 2 output\nJob completed successfully\n";
    std::fs::write(&log_path, log_content).unwrap();

    let read_back = std::fs::read_to_string(&log_path).unwrap();
    assert_eq!(read_back, log_content);
}

// ─────────────────────────────────────────────────────────────────
// Executor
// ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_bash_executor_basic() {
    let config = Arc::new(ruci_core::config::Config::default());
    let executor = BashExecutor::new(config);

    let ctx = ExecutionContext {
        run_id: "run-1".to_string(),
        job_id: "job-1".to_string(),
        build_num: 1,
        work_dir: std::path::PathBuf::from("/tmp"),
        env: HashMap::new(),
        params: HashMap::new(),
    };

    let job = Job::parse(
        r#"
name: test-job
steps:
  - name: echo
    command: echo hello
"#,
    )
    .unwrap();

    let result = executor.execute(&ctx, &job).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.logs.contains("hello"));
}

#[tokio::test]
async fn test_bash_executor_failing_step() {
    let config = Arc::new(ruci_core::config::Config::default());
    let executor = BashExecutor::new(config);

    let ctx = ExecutionContext {
        run_id: "run-1".to_string(),
        job_id: "job-1".to_string(),
        build_num: 1,
        work_dir: std::path::PathBuf::from("/tmp"),
        env: HashMap::new(),
        params: HashMap::new(),
    };

    let job = Job::parse(
        r#"
name: test-job
steps:
  - name: fail
    command: exit 42
"#,
    )
    .unwrap();

    let result = executor.execute(&ctx, &job).await.unwrap();
    assert_eq!(result.exit_code, 42);
}

// ─────────────────────────────────────────────────────────────────
// VCS Credentials
// ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_vcs_credential_crud() {
    let db = setup_db().await;

    use ruci_core::db::{VcsCredentialInfo, WebhookSource};

    let cred = VcsCredentialInfo {
        id: "cred-1".to_string(),
        name: "My SSH Key".to_string(),
        vcs_type: WebhookSource::Github,
        username: None,
        credential: "/home/user/.ssh/id_rsa".to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    // Insert
    db.upsert_credential(&cred).await.unwrap();

    // Retrieve
    let retrieved = db.get_credential("cred-1").await.unwrap().unwrap();
    assert_eq!(retrieved.name, "My SSH Key");
    assert_eq!(retrieved.credential, "/home/user/.ssh/id_rsa");

    // List
    let all = db.list_credentials().await.unwrap();
    assert_eq!(all.len(), 1);

    // Delete
    db.delete_credential("cred-1").await.unwrap();
    let deleted = db.get_credential("cred-1").await.unwrap();
    assert!(deleted.is_none());
}

// ─────────────────────────────────────────────────────────────────
// User authentication
// ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_user_crud() {
    let db = setup_db().await;

    use ruci_core::db::repository::UserInfo;

    let user = UserInfo {
        id: "user-1".to_string(),
        username: "admin".to_string(),
        password_hash: "hashed_password".to_string(),
        role: "admin".to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        last_login_at: None,
    };

    db.insert_user(&user).await.unwrap();

    let retrieved = db.get_user_by_username("admin").await.unwrap().unwrap();
    assert_eq!(retrieved.username, "admin");
    assert_eq!(retrieved.role, "admin");
}

// ─────────────────────────────────────────────────────────────────
// Trigger
// ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_trigger_crud() {
    let db = setup_db().await;
    db.insert_job(&test_job("job-1", "test")).await.unwrap();

    use ruci_core::db::repository::TriggerInfo;

    let trigger = TriggerInfo {
        name: "nightly-build".to_string(),
        cron: "0 0 2 * * *".to_string(),
        job_id: "job-1".to_string(),
        enabled: true,
    };

    db.upsert_trigger(&trigger).await.unwrap();

    let retrieved = db.get_trigger("nightly-build").await.unwrap().unwrap();
    assert_eq!(retrieved.cron, "0 0 2 * * *");
    assert!(retrieved.enabled);

    // Toggle
    db.set_trigger_enabled("nightly-build", false)
        .await
        .unwrap();
    let retrieved = db.get_trigger("nightly-build").await.unwrap().unwrap();
    assert!(!retrieved.enabled);
}

// ─────────────────────────────────────────────────────────────────
// Webhook
// ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_webhook_trigger_crud() {
    let db = setup_db().await;
    db.insert_job(&test_job("job-1", "test")).await.unwrap();

    use ruci_core::db::{WebhookEvent, WebhookFilter, WebhookSource, WebhookTriggerInfo};

    let webhook = WebhookTriggerInfo {
        name: "github-push".to_string(),
        job_id: "job-1".to_string(),
        enabled: true,
        secret: "my-secret".to_string(),
        source: WebhookSource::Github,
        filter: WebhookFilter {
            repository: Some("owner/repo".to_string()),
            branches: vec!["main".to_string(), "develop".to_string()],
            events: vec![WebhookEvent::Push],
        },
        credential_id: None,
    };

    db.upsert_webhook_trigger(&webhook).await.unwrap();

    let retrieved = db
        .get_webhook_trigger("github-push")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retrieved.source, WebhookSource::Github);
    assert_eq!(retrieved.filter.branches.len(), 2);
    assert!(retrieved.enabled);

    // Toggle
    db.set_webhook_trigger_enabled("github-push", false)
        .await
        .unwrap();
    let retrieved = db
        .get_webhook_trigger("github-push")
        .await
        .unwrap()
        .unwrap();
    assert!(!retrieved.enabled);

    // Delete
    db.delete_webhook_trigger("github-push").await.unwrap();
    let deleted = db.get_webhook_trigger("github-push").await.unwrap();
    assert!(deleted.is_none());
}
