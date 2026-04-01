//! Protocol types for RPC
//!
//! These types are used in the RPC protocol

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

use super::{ArtifactInfo, JobInfo, RunInfo, RunStatus};

/// Job row from database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRow {
    pub id: String,
    pub original_name: String,
    pub name: String,
    pub submitted_at: String,
    pub config_yaml: String,
    pub content_hash: String,
}

impl From<JobRow> for JobInfo {
    fn from(row: JobRow) -> Self {
        JobInfo {
            id: row.id,
            name: row.name,
            original_name: row.original_name,
            submitted_at: row.submitted_at.parse().unwrap_or_else(|_| Utc::now()),
        }
    }
}

/// Run row from database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRow {
    pub id: String,
    pub job_id: String,
    pub job_name: String,
    pub build_num: i64,
    pub status: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub exit_code: Option<i32>,
    pub params: Option<String>,
}

impl From<RunRow> for RunInfo {
    fn from(row: RunRow) -> Self {
        RunInfo {
            id: row.id,
            job_id: row.job_id,
            job_name: row.job_name,
            build_num: row.build_num as u64,
            status: match row.status.as_str() {
                "QUEUED" => RunStatus::Queued,
                "RUNNING" => RunStatus::Running,
                "SUCCESS" => RunStatus::Success,
                "FAILED" => RunStatus::Failed,
                "ABORTED" => RunStatus::Aborted,
                _ => RunStatus::Failed,
            },
            started_at: row.started_at.and_then(|s| s.parse().ok()),
            finished_at: row.finished_at.and_then(|s| s.parse().ok()),
            exit_code: row.exit_code,
        }
    }
}
