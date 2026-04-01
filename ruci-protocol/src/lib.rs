//! Ruci RPC Protocol Definitions
//!
//! This crate defines the RPC interface using tarpc and serde serialization.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

pub mod protocol_types {
    pub use super::{ArtifactInfo, JobInfo, RunInfo, RunStatus};
}

/// Unique identifier for a job
pub type JobId = String;

/// Unique identifier for a run
pub type RunId = String;

/// Unique identifier for an artifact
pub type ArtifactId = String;

/// Build number for a job
pub type BuildNum = u64;

/// Response for queue operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueResponse {
    pub run_id: RunId,
    pub build_num: BuildNum,
    pub status: RunStatus,
    /// Error code if the operation failed (maps to ErrorCode enum value)
    pub error_code: Option<u8>,
    /// Error message if the operation failed
    pub error_message: Option<String>,
}

impl QueueResponse {
    /// Create a successful queue response
    pub fn success(run_id: RunId, build_num: BuildNum) -> Self {
        Self {
            run_id,
            build_num,
            status: RunStatus::Queued,
            error_code: None,
            error_message: None,
        }
    }

    /// Create a failed queue response
    pub fn error(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            run_id: RunId::new(),
            build_num: 0,
            status: RunStatus::Failed,
            error_code: Some(code as u8),
            error_message: Some(message.into()),
        }
    }
}

/// Response for job submission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobSubmitResponse {
    pub job_id: JobId,
    pub run_id: RunId,
    pub build_num: BuildNum,
    /// Error code if the operation failed (maps to ErrorCode enum value)
    pub error_code: Option<u8>,
    /// Error message if the operation failed
    pub error_message: Option<String>,
}

impl JobSubmitResponse {
    /// Create a successful job submit response
    pub fn success(job_id: JobId, run_id: RunId, build_num: BuildNum) -> Self {
        Self {
            job_id,
            run_id,
            build_num,
            error_code: None,
            error_message: None,
        }
    }

    /// Create a failed job submit response
    pub fn error(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            job_id: JobId::new(),
            run_id: RunId::new(),
            build_num: 0,
            error_code: Some(code as u8),
            error_message: Some(message.into()),
        }
    }
}

/// Information about a registered job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobInfo {
    pub id: JobId,
    pub name: String,
    pub original_name: String,
    pub submitted_at: chrono::DateTime<chrono::Utc>,
}

/// Information about a run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunInfo {
    pub id: RunId,
    pub job_id: JobId,
    pub job_name: String,
    pub build_num: BuildNum,
    pub status: RunStatus,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub exit_code: Option<i32>,
}

/// Run execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum RunStatus {
    Queued,
    Running,
    Success,
    Failed,
    Aborted,
}

impl fmt::Display for RunStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunStatus::Queued => write!(f, "QUEUED"),
            RunStatus::Running => write!(f, "RUNNING"),
            RunStatus::Success => write!(f, "SUCCESS"),
            RunStatus::Failed => write!(f, "FAILED"),
            RunStatus::Aborted => write!(f, "ABORTED"),
        }
    }
}

/// Information about an artifact
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactInfo {
    pub id: ArtifactId,
    pub run_id: RunId,
    pub name: String,
    pub size: u64,
    pub checksum: String,
    pub storage_path: String,
}

/// Ruci RPC Service Definition
#[tarpc::service]
pub trait RuciRpc {
    // Job Management
    async fn queue_job(job_id: JobId, params: HashMap<String, String>) -> QueueResponse;
    async fn abort_job(run_id: RunId);
    async fn list_jobs() -> Vec<JobInfo>;
    async fn get_job(job_id: JobId) -> Option<JobInfo>;

    // Job Submission (Travis CI style)
    async fn submit_job(yaml_content: String) -> JobSubmitResponse;

    // Run Status
    async fn list_queued() -> Vec<RunInfo>;
    async fn list_running() -> Vec<RunInfo>;
    async fn get_run(run_id: RunId) -> Option<RunInfo>;
    async fn get_run_log(run_id: RunId) -> String;

    // Artifact Management
    async fn upload_artifact(run_id: RunId, local_path: String) -> ArtifactInfo;
    async fn download_artifact(artifact_id: ArtifactId) -> Vec<u8>;
    async fn list_artifacts(run_id: RunId) -> Vec<ArtifactInfo>;

    // Daemon Control
    async fn status() -> DaemonStatus;
}

/// Daemon status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub version: String,
    pub uptime_seconds: u64,
    pub jobs_queued: usize,
    pub jobs_running: usize,
    pub jobs_total: usize,
    pub runs_total: usize,
}

/// Error codes for RPC errors
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ErrorCode {
    JobNotFound = 0x01,
    RunNotFound = 0x02,
    ArtifactNotFound = 0x03,
    InvalidParams = 0x04,
    QueueFull = 0x05,
    JobRunning = 0x06,
    StorageError = 0x07,
    DatabaseError = 0x08,
    Internal = 0xFF,
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorCode::JobNotFound => write!(f, "JOB_NOT_FOUND"),
            ErrorCode::RunNotFound => write!(f, "RUN_NOT_FOUND"),
            ErrorCode::ArtifactNotFound => write!(f, "ARTIFACT_NOT_FOUND"),
            ErrorCode::InvalidParams => write!(f, "INVALID_PARAMS"),
            ErrorCode::QueueFull => write!(f, "QUEUE_FULL"),
            ErrorCode::JobRunning => write!(f, "JOB_RUNNING"),
            ErrorCode::StorageError => write!(f, "STORAGE_ERROR"),
            ErrorCode::DatabaseError => write!(f, "DATABASE_ERROR"),
            ErrorCode::Internal => write!(f, "INTERNAL"),
        }
    }
}

/// RPC Error types
#[derive(Debug, Clone, thiserror::Error)]
#[error("RPC error: {code}: {message}")]
pub struct RuciRpcError {
    pub code: ErrorCode,
    pub message: String,
}

impl RuciRpcError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn job_not_found(id: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::JobNotFound,
            format!("Job not found: {}", id.into()),
        )
    }

    pub fn run_not_found(id: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::RunNotFound,
            format!("Run not found: {}", id.into()),
        )
    }

    pub fn artifact_not_found(id: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::ArtifactNotFound,
            format!("Artifact not found: {}", id.into()),
        )
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_status_display() {
        assert_eq!(RunStatus::Queued.to_string(), "QUEUED");
        assert_eq!(RunStatus::Running.to_string(), "RUNNING");
        assert_eq!(RunStatus::Success.to_string(), "SUCCESS");
        assert_eq!(RunStatus::Failed.to_string(), "FAILED");
        assert_eq!(RunStatus::Aborted.to_string(), "ABORTED");
    }

    #[test]
    fn test_run_status_serde() {
        let status = RunStatus::Success;
        let serialized = serde_json::to_string(&status).unwrap();
        assert_eq!(serialized, "\"SUCCESS\"");

        let deserialized: RunStatus = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, RunStatus::Success);
    }

    #[test]
    fn test_error_code_display() {
        assert_eq!(ErrorCode::JobNotFound.to_string(), "JOB_NOT_FOUND");
        assert_eq!(ErrorCode::RunNotFound.to_string(), "RUN_NOT_FOUND");
        assert_eq!(
            ErrorCode::ArtifactNotFound.to_string(),
            "ARTIFACT_NOT_FOUND"
        );
        assert_eq!(ErrorCode::InvalidParams.to_string(), "INVALID_PARAMS");
        assert_eq!(ErrorCode::QueueFull.to_string(), "QUEUE_FULL");
        assert_eq!(ErrorCode::JobRunning.to_string(), "JOB_RUNNING");
        assert_eq!(ErrorCode::StorageError.to_string(), "STORAGE_ERROR");
        assert_eq!(ErrorCode::DatabaseError.to_string(), "DATABASE_ERROR");
        assert_eq!(ErrorCode::Internal.to_string(), "INTERNAL");
    }

    #[test]
    fn test_ruci_rpc_error_new() {
        let error = RuciRpcError::new(ErrorCode::JobNotFound, "Job 123 not found");
        assert_eq!(error.code, ErrorCode::JobNotFound);
        assert_eq!(error.message, "Job 123 not found");
        assert_eq!(
            error.to_string(),
            "RPC error: JOB_NOT_FOUND: Job 123 not found"
        );
    }

    #[test]
    fn test_ruci_rpc_error_helpers() {
        let job_err = RuciRpcError::job_not_found("job-456");
        assert_eq!(job_err.code, ErrorCode::JobNotFound);
        assert!(job_err.message.contains("job-456"));

        let run_err = RuciRpcError::run_not_found("run-789");
        assert_eq!(run_err.code, ErrorCode::RunNotFound);
        assert!(run_err.message.contains("run-789"));

        let artifact_err = RuciRpcError::artifact_not_found("artifact-abc");
        assert_eq!(artifact_err.code, ErrorCode::ArtifactNotFound);
        assert!(artifact_err.message.contains("artifact-abc"));

        let internal_err = RuciRpcError::internal("Something went wrong");
        assert_eq!(internal_err.code, ErrorCode::Internal);
    }

    #[test]
    fn test_job_info_serde() {
        let job = JobInfo {
            id: "job-123".to_string(),
            name: "test-job".to_string(),
            original_name: ".ruci.yml".to_string(),
            submitted_at: chrono::DateTime::from_timestamp(1234567890, 0).unwrap(),
        };

        let serialized = serde_json::to_string(&job).unwrap();
        let deserialized: JobInfo = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.id, job.id);
        assert_eq!(deserialized.name, job.name);
        assert_eq!(deserialized.original_name, job.original_name);
    }

    #[test]
    fn test_run_info_serde() {
        let run = RunInfo {
            id: "run-456".to_string(),
            job_id: "job-123".to_string(),
            job_name: "test-job".to_string(),
            build_num: 42,
            status: RunStatus::Running,
            started_at: Some(chrono::DateTime::from_timestamp(1234567890, 0).unwrap()),
            finished_at: None,
            exit_code: None,
        };

        let serialized = serde_json::to_string(&run).unwrap();
        let deserialized: RunInfo = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.id, run.id);
        assert_eq!(deserialized.build_num, run.build_num);
        assert_eq!(deserialized.status, RunStatus::Running);
    }

    #[test]
    fn test_artifact_info_serde() {
        let artifact = ArtifactInfo {
            id: "artifact-789".to_string(),
            run_id: "run-456".to_string(),
            name: "binary".to_string(),
            size: 1024,
            checksum: "abc123".to_string(),
            storage_path: "/artifacts/run-456/binary".to_string(),
        };

        let serialized = serde_json::to_string(&artifact).unwrap();
        let deserialized: ArtifactInfo = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.id, artifact.id);
        assert_eq!(deserialized.size, 1024);
        assert_eq!(deserialized.checksum, "abc123");
    }

    #[test]
    fn test_daemon_status_serde() {
        let status = DaemonStatus {
            version: "1.0.0".to_string(),
            uptime_seconds: 3600,
            jobs_queued: 5,
            jobs_running: 2,
            jobs_total: 100,
            runs_total: 500,
        };

        let serialized = serde_json::to_string(&status).unwrap();
        let deserialized: DaemonStatus = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.version, "1.0.0");
        assert_eq!(deserialized.uptime_seconds, 3600);
        assert_eq!(deserialized.jobs_queued, 5);
        assert_eq!(deserialized.jobs_running, 2);
    }

    #[test]
    fn test_queue_response_serde() {
        let response = QueueResponse::success("run-123".to_string(), 5);

        let serialized = serde_json::to_string(&response).unwrap();
        let deserialized: QueueResponse = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.run_id, "run-123");
        assert_eq!(deserialized.build_num, 5);
        assert_eq!(deserialized.status, RunStatus::Queued);
        assert!(deserialized.error_code.is_none());
        assert!(deserialized.error_message.is_none());
    }

    #[test]
    fn test_job_submit_response_serde() {
        let response = JobSubmitResponse::success("job-456".to_string(), "run-789".to_string(), 1);

        let serialized = serde_json::to_string(&response).unwrap();
        let deserialized: JobSubmitResponse = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.job_id, "job-456");
        assert_eq!(deserialized.run_id, "run-789");
        assert_eq!(deserialized.build_num, 1);
        assert!(deserialized.error_code.is_none());
        assert!(deserialized.error_message.is_none());
    }

    #[test]
    fn test_queue_response_error() {
        let response = QueueResponse::error(ErrorCode::JobNotFound, "Job not found: test-job");

        assert!(response.run_id.is_empty());
        assert_eq!(response.build_num, 0);
        assert_eq!(response.status, RunStatus::Failed);
        assert_eq!(response.error_code, Some(ErrorCode::JobNotFound as u8));
        assert!(response.error_message.is_some());
    }

    #[test]
    fn test_job_submit_response_error() {
        let response = JobSubmitResponse::error(ErrorCode::InvalidParams, "Invalid YAML syntax");

        assert!(response.job_id.is_empty());
        assert!(response.run_id.is_empty());
        assert_eq!(response.build_num, 0);
        assert_eq!(response.error_code, Some(ErrorCode::InvalidParams as u8));
        assert!(response.error_message.is_some());
    }

    #[test]
    fn test_type_aliases() {
        let job_id: JobId = "test-job".to_string();
        let run_id: RunId = "test-run".to_string();
        let artifact_id: ArtifactId = "test-artifact".to_string();
        let build_num: BuildNum = 42;

        assert_eq!(job_id, "test-job");
        assert_eq!(run_id, "test-run");
        assert_eq!(artifact_id, "test-artifact");
        assert_eq!(build_num, 42);
    }
}
