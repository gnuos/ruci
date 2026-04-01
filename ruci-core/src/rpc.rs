//! RPC module
//!
//! tarpc server implementation using tarpc 0.37 API

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use futures::{future, prelude::*};
use tarpc::context::Context;
use tarpc::server::{self, Channel};
use tokio_serde::formats::Json;

use crate::config::{Config, RpcMode};
use crate::db::Repository;
use crate::error::Result;
use crate::executor::{Executor, Job};
use crate::queue::{JobQueue, QueueRequest};
use crate::storage::Storage;
use ruci_protocol::{
    ArtifactInfo, DaemonStatus, ErrorCode, JobInfo, JobSubmitResponse, QueueResponse, RuciRpc,
    RunInfo, RunStatus,
};

/// RPC Server implementation
pub struct RpcServer {
    config: Arc<Config>,
    socket_path: PathBuf,
    db: Arc<dyn Repository>,
    queue: Arc<JobQueue>,
    executor: Arc<dyn Executor>,
    storage: Arc<dyn Storage>,
}

impl RpcServer {
    pub fn new(
        config: Arc<Config>,
        socket_path: PathBuf,
        db: Arc<dyn Repository>,
        queue: Arc<JobQueue>,
        executor: Arc<dyn Executor>,
        storage: Arc<dyn Storage>,
    ) -> Self {
        Self {
            config,
            socket_path,
            db,
            queue,
            executor,
            storage,
        }
    }

    /// Start the RPC server
    pub async fn serve(&self) -> Result<()> {
        let addr = format!("{}:{}", self.config.server.host, self.config.server.port);

        match self.config.server.rpc_mode {
            RpcMode::Tcp => self.serve_tcp(&addr).await,
            RpcMode::Unix => {
                let socket_name = self.socket_path.to_string_lossy();
                std::fs::remove_file(&*socket_name).ok();
                // For Unix socket, use tcp for now
                self.serve_tcp(&addr).await
            }
        }
    }

    async fn serve_tcp(&self, addr: &str) -> Result<()> {
        let mut listener =
            tarpc::serde_transport::tcp::listen(addr, || Json::<_, _>::default()).await?;
        listener.config_mut().max_frame_length(usize::MAX);

        tracing::info!("RPC server listening on {}", addr);

        let server = Arc::new(RuciRpcImpl {
            config: self.config.clone(),
            db: self.db.clone(),
            queue: self.queue.clone(),
            executor: self.executor.clone(),
            storage: self.storage.clone(),
        });

        let incoming = listener.filter_map(|r| future::ready(r.ok()));
        incoming
            .map(server::BaseChannel::with_defaults)
            .map(|channel| {
                let server = (*server).clone();
                channel
                    .execute(server.serve())
                    .for_each(|response| async move {
                        tokio::spawn(response);
                    })
            })
            .buffer_unordered(10)
            .for_each(|_| async {})
            .await;

        Ok(())
    }
}

impl Clone for RpcServer {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            socket_path: self.socket_path.clone(),
            db: self.db.clone(),
            queue: self.queue.clone(),
            executor: self.executor.clone(),
            storage: self.storage.clone(),
        }
    }
}

/// RPC handler implementation
#[derive(Clone)]
struct RuciRpcImpl {
    config: Arc<Config>,
    db: Arc<dyn Repository>,
    queue: Arc<JobQueue>,
    executor: Arc<dyn Executor>,
    storage: Arc<dyn Storage>,
}

impl RuciRpcImpl {
    async fn do_queue_job(&self, job_id: String, params: HashMap<String, String>) -> QueueResponse {
        let _job = match self.db.get_job(&job_id).await {
            Ok(Some(j)) => j,
            Ok(None) => {
                return QueueResponse::error(
                    ErrorCode::JobNotFound,
                    format!("Job not found: {}", job_id),
                );
            }
            Err(e) => {
                tracing::error!("Database error getting job: {}", e);
                return QueueResponse::error(
                    ErrorCode::DatabaseError,
                    format!("Database error: {}", e),
                );
            }
        };

        let build_num = self.db.next_build_num(&job_id).await.unwrap_or(1);
        let run_id = uuid::Uuid::new_v4().to_string();

        // Serialize params to JSON for persistence
        let params_json = serde_json::to_string(&params).ok();

        if let Err(e) = self
            .db
            .insert_run(
                &run_id,
                &job_id,
                build_num as i64,
                "QUEUED",
                params_json.as_deref(),
            )
            .await
        {
            tracing::error!("Failed to insert run: {}", e);
            return QueueResponse::error(
                ErrorCode::DatabaseError,
                format!("Failed to insert run: {}", e),
            );
        }

        let req = QueueRequest {
            job_id,
            params,
            run_id: run_id.clone(),
            build_num: build_num as u64,
        };

        if let Err(e) = self.queue.enqueue(req).await {
            tracing::error!("Failed to enqueue: {}", e);
            return QueueResponse::error(ErrorCode::Internal, format!("Failed to enqueue: {}", e));
        }

        QueueResponse::success(run_id, build_num as u64)
    }
}

impl RuciRpc for RuciRpcImpl {
    async fn queue_job(
        self,
        _: Context,
        job_id: String,
        params: HashMap<String, String>,
    ) -> QueueResponse {
        self.do_queue_job(job_id, params).await
    }

    async fn abort_job(self, _: Context, run_id: String) {
        if let Err(e) = self.db.update_run_status(&run_id, "ABORTED", None).await {
            tracing::error!("Failed to abort job: {}", e);
        }
        if let Err(e) = self.executor.abort(&run_id).await {
            tracing::error!("Failed to abort executor: {}", e);
        }
    }

    async fn list_jobs(self, _: Context) -> Vec<JobInfo> {
        self.db.list_jobs().await.unwrap_or_default()
    }

    async fn get_job(self, _: Context, job_id: String) -> Option<JobInfo> {
        self.db.get_job(&job_id).await.unwrap_or(None)
    }

    async fn submit_job(self, _: Context, yaml_content: String) -> JobSubmitResponse {
        let job = match Job::parse(&yaml_content) {
            Ok(j) => j,
            Err(e) => {
                tracing::error!("Failed to parse job: {}", e);
                return JobSubmitResponse::error(
                    ErrorCode::InvalidParams,
                    format!("Invalid YAML: {}", e),
                );
            }
        };

        let job_id = Config::short_hash(&yaml_content);
        let job_path = format!("{}/{}.yaml", self.config.paths.jobs_dir, job_id);
        if let Err(e) = std::fs::write(&job_path, &yaml_content) {
            tracing::error!("Failed to write job file: {}", e);
            return JobSubmitResponse::error(
                ErrorCode::Internal,
                format!("Failed to write job file: {}", e),
            );
        }

        let job_info = JobInfo {
            id: job_id.clone(),
            name: job.name.clone(),
            original_name: ".ruci.yml".to_string(),
            submitted_at: chrono::Utc::now(),
        };
        if let Err(e) = self.db.insert_job(&job_info).await {
            tracing::error!("Failed to insert job: {}", e);
            return JobSubmitResponse::error(
                ErrorCode::DatabaseError,
                format!("Failed to save job: {}", e),
            );
        }

        let build_num = self.db.next_build_num(&job_id).await.unwrap_or(1);
        let run_id = uuid::Uuid::new_v4().to_string();

        if let Err(e) = self
            .db
            .insert_run(&run_id, &job_id, build_num as i64, "QUEUED", Some("{}"))
            .await
        {
            tracing::error!("Failed to insert run: {}", e);
            return JobSubmitResponse::error(
                ErrorCode::DatabaseError,
                format!("Failed to create run: {}", e),
            );
        }

        let req = QueueRequest {
            job_id: job_id.clone(),
            params: HashMap::new(),
            run_id: run_id.clone(),
            build_num: build_num as u64,
        };

        if let Err(e) = self.queue.enqueue(req).await {
            tracing::error!("Failed to enqueue job: {}", e);
            return JobSubmitResponse::error(
                ErrorCode::Internal,
                format!("Failed to enqueue job: {}", e),
            );
        }

        JobSubmitResponse::success(job_id, run_id, build_num as u64)
    }

    async fn list_queued(self, _: Context) -> Vec<RunInfo> {
        self.db
            .list_runs_by_status("QUEUED")
            .await
            .unwrap_or_default()
    }

    async fn list_running(self, _: Context) -> Vec<RunInfo> {
        self.db
            .list_runs_by_status("RUNNING")
            .await
            .unwrap_or_default()
    }

    async fn get_run(self, _: Context, run_id: String) -> Option<RunInfo> {
        self.db.get_run(&run_id).await.unwrap_or(None)
    }

    async fn get_run_log(self, _: Context, run_id: String) -> String {
        format!("Log for run {} not yet implemented", run_id)
    }

    async fn upload_artifact(self, _: Context, run_id: String, local_path: String) -> ArtifactInfo {
        let path = std::path::Path::new(&local_path);
        let key = format!(
            "{}/{}",
            run_id,
            path.file_name()
                .unwrap_or_default()
                .to_str()
                .unwrap_or("artifact")
        );

        match self.storage.upload(&key, path).await {
            Ok(handle) => {
                let artifact_id = uuid::Uuid::new_v4().to_string();
                if let Err(e) = self
                    .db
                    .insert_artifact(
                        &artifact_id,
                        &run_id,
                        path.file_name()
                            .unwrap_or_default()
                            .to_str()
                            .unwrap_or("artifact"),
                        handle.size as i64,
                        &handle.checksum,
                        &handle.key,
                    )
                    .await
                {
                    tracing::error!("Failed to insert artifact: {}", e);
                }

                ArtifactInfo {
                    id: artifact_id,
                    run_id,
                    name: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                    size: handle.size,
                    checksum: handle.checksum,
                    storage_path: handle.key,
                }
            }
            Err(e) => {
                tracing::error!("Failed to upload artifact: {}", e);
                ArtifactInfo {
                    id: String::new(),
                    run_id,
                    name: String::new(),
                    size: 0,
                    checksum: String::new(),
                    storage_path: String::new(),
                }
            }
        }
    }

    async fn download_artifact(self, _: Context, artifact_id: String) -> Vec<u8> {
        match self.db.get_artifact(&artifact_id).await.unwrap_or(None) {
            Some(artifact) => self
                .storage
                .download(&artifact.storage_path)
                .await
                .unwrap_or_default(),
            None => Vec::new(),
        }
    }

    async fn list_artifacts(self, _: Context, run_id: String) -> Vec<ArtifactInfo> {
        self.db.list_artifacts(&run_id).await.unwrap_or_default()
    }

    async fn status(self, _: Context) -> DaemonStatus {
        DaemonStatus {
            version: env!("CARGO_PKG_VERSION").to_string(),
            uptime_seconds: 0,
            jobs_queued: self.queue.len(),
            jobs_running: 0,
            jobs_total: self.db.list_jobs().await.unwrap_or_default().len(),
            runs_total: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Repository;
    use crate::executor::{ExecutionContext, ExecutionResult, Executor};
    use crate::storage::Storage;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// Mock executor for testing
    struct MockExecutor;

    #[async_trait]
    impl Executor for MockExecutor {
        async fn execute(
            &self,
            _ctx: &ExecutionContext,
            _job: &Job,
        ) -> crate::error::Result<ExecutionResult> {
            Ok(ExecutionResult {
                exit_code: 0,
                logs: String::new(),
                artifacts: vec![],
            })
        }

        async fn abort(&self, _run_id: &str) -> crate::error::Result<()> {
            Ok(())
        }
    }

    /// Mock storage for testing
    struct MockStorage;

    #[async_trait]
    impl Storage for MockStorage {
        async fn upload(
            &self,
            _key: &str,
            _path: &std::path::Path,
        ) -> crate::error::Result<crate::storage::StorageHandle> {
            Ok(crate::storage::StorageHandle {
                key: "mock-key".to_string(),
                size: 100,
                checksum: "abc123".to_string(),
            })
        }

        async fn download(&self, _key: &str) -> crate::error::Result<Vec<u8>> {
            Ok(vec![1, 2, 3])
        }

        async fn delete(&self, _key: &str) -> crate::error::Result<()> {
            Ok(())
        }

        async fn exists(&self, _key: &str) -> bool {
            true
        }

        fn url(&self, _key: &str) -> Option<String> {
            Some("http://mock.url".to_string())
        }
    }

    fn create_test_config() -> Config {
        Config::default()
    }

    async fn create_mock_db() -> Arc<dyn Repository> {
        // Create an in-memory SQLite repo for testing
        let repo = crate::db::sqlite::SqliteRepository::new("sqlite::memory:")
            .await
            .unwrap();
        repo.migrate().await.unwrap();
        Arc::new(repo)
    }

    #[tokio::test]
    async fn test_rpc_server_new() {
        let config = Arc::new(create_test_config());
        let db = create_mock_db().await;
        let queue = Arc::new(JobQueue::new());
        let executor: Arc<dyn Executor> = Arc::new(MockExecutor);
        let storage: Arc<dyn Storage> = Arc::new(MockStorage);
        let socket_path = PathBuf::from("/tmp/test.sock");

        let server = RpcServer::new(config, socket_path.clone(), db, queue, executor, storage);

        assert_eq!(server.socket_path, socket_path);
    }

    #[tokio::test]
    async fn test_rpc_server_clone() {
        let config = Arc::new(create_test_config());
        let db = create_mock_db().await;
        let queue = Arc::new(JobQueue::new());
        let executor: Arc<dyn Executor> = Arc::new(MockExecutor);
        let storage: Arc<dyn Storage> = Arc::new(MockStorage);
        let socket_path = PathBuf::from("/tmp/test.sock");

        let server = RpcServer::new(
            config.clone(),
            socket_path.clone(),
            db.clone(),
            queue.clone(),
            executor.clone(),
            storage.clone(),
        );

        let cloned = server.clone();
        assert_eq!(cloned.socket_path, socket_path);
    }

    #[test]
    fn test_queue_response_creation() {
        let response = QueueResponse::success("run-123".to_string(), 1);

        assert_eq!(response.run_id, "run-123");
        assert_eq!(response.build_num, 1);
        assert_eq!(response.status, RunStatus::Queued);
        assert!(response.error_code.is_none());
        assert!(response.error_message.is_none());
    }

    #[test]
    fn test_queue_response_failed() {
        use ruci_protocol::ErrorCode;
        let response = QueueResponse::error(ErrorCode::JobNotFound, "Job not found");

        assert!(response.run_id.is_empty());
        assert_eq!(response.build_num, 0);
        assert_eq!(response.status, RunStatus::Failed);
        assert_eq!(response.error_code, Some(ErrorCode::JobNotFound as u8));
    }

    #[test]
    fn test_job_submit_response_creation() {
        let response = JobSubmitResponse::success("job-abc".to_string(), "run-123".to_string(), 5);

        assert_eq!(response.job_id, "job-abc");
        assert_eq!(response.run_id, "run-123");
        assert_eq!(response.build_num, 5);
        assert!(response.error_code.is_none());
        assert!(response.error_message.is_none());
    }

    #[test]
    fn test_daemon_status_creation() {
        let status = DaemonStatus {
            version: "1.0.0".to_string(),
            uptime_seconds: 3600,
            jobs_queued: 10,
            jobs_running: 2,
            jobs_total: 100,
            runs_total: 500,
        };

        assert_eq!(status.version, "1.0.0");
        assert_eq!(status.uptime_seconds, 3600);
        assert_eq!(status.jobs_queued, 10);
        assert_eq!(status.jobs_running, 2);
        assert_eq!(status.jobs_total, 100);
        assert_eq!(status.runs_total, 500);
    }

    #[test]
    fn test_artifact_info_creation() {
        let artifact = ArtifactInfo {
            id: "art-123".to_string(),
            run_id: "run-456".to_string(),
            name: "binary".to_string(),
            size: 1024,
            checksum: "abc123".to_string(),
            storage_path: "/path/to/artifact".to_string(),
        };

        assert_eq!(artifact.id, "art-123");
        assert_eq!(artifact.run_id, "run-456");
        assert_eq!(artifact.name, "binary");
        assert_eq!(artifact.size, 1024);
        assert_eq!(artifact.checksum, "abc123");
        assert_eq!(artifact.storage_path, "/path/to/artifact");
    }

    #[test]
    fn test_run_info_creation() {
        let run_info = RunInfo {
            id: "run-789".to_string(),
            job_id: "job-abc".to_string(),
            job_name: "Test Job".to_string(),
            build_num: 3,
            status: RunStatus::Running,
            started_at: Some(chrono::Utc::now()),
            finished_at: None,
            exit_code: None,
        };

        assert_eq!(run_info.id, "run-789");
        assert_eq!(run_info.job_id, "job-abc");
        assert_eq!(run_info.build_num, 3);
        assert_eq!(run_info.status, RunStatus::Running);
        assert!(run_info.exit_code.is_none());
    }

    #[test]
    fn test_run_status_display() {
        assert_eq!(format!("{}", RunStatus::Queued), "QUEUED");
        assert_eq!(format!("{}", RunStatus::Running), "RUNNING");
        assert_eq!(format!("{}", RunStatus::Success), "SUCCESS");
        assert_eq!(format!("{}", RunStatus::Failed), "FAILED");
        assert_eq!(format!("{}", RunStatus::Aborted), "ABORTED");
    }

    #[tokio::test]
    async fn test_mock_executor_execute() {
        let executor = MockExecutor;
        let ctx = ExecutionContext {
            run_id: "run-test".to_string(),
            job_id: "job-test".to_string(),
            build_num: 1,
            work_dir: PathBuf::from("/tmp"),
            env: HashMap::new(),
            params: HashMap::new(),
        };
        let job = Job {
            name: "test-job".to_string(),
            context: "default".to_string(),
            timeout: 3600,
            env: HashMap::new(),
            steps: vec![],
            vcs: None,
            checkout: false,
        };

        let result = executor.execute(&ctx, &job).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().exit_code, 0);
    }

    #[tokio::test]
    async fn test_mock_executor_abort() {
        let executor = MockExecutor;
        let result = executor.abort("run-123").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_storage_upload() {
        let storage = MockStorage;
        let path = PathBuf::from("/tmp/test.file");
        let result = storage.upload("key", &path).await;
        assert!(result.is_ok());
        let handle = result.unwrap();
        assert_eq!(handle.key, "mock-key");
        assert_eq!(handle.size, 100);
    }

    #[tokio::test]
    async fn test_mock_storage_download() {
        let storage = MockStorage;
        let result = storage.download("key").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_mock_storage_delete() {
        let storage = MockStorage;
        let result = storage.delete("key").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_storage_exists() {
        let storage = MockStorage;
        let result = storage.exists("key").await;
        assert!(result);
    }

    #[test]
    fn test_mock_storage_url() {
        let storage = MockStorage;
        let url = storage.url("key");
        assert_eq!(url, Some("http://mock.url".to_string()));
    }

    #[test]
    fn test_job_info_creation() {
        let job_info = JobInfo {
            id: "job-123".to_string(),
            name: "Test Job".to_string(),
            original_name: ".ruci.yml".to_string(),
            submitted_at: chrono::Utc::now(),
        };

        assert_eq!(job_info.id, "job-123");
        assert_eq!(job_info.name, "Test Job");
        assert_eq!(job_info.original_name, ".ruci.yml");
    }
}
