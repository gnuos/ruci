//! Repository trait definitions
//!
//! This module defines the abstract interfaces for data access.
//! Concrete implementations (SQLite, PostgreSQL, etc.) implement these traits.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::Result;
use ruci_protocol::{ArtifactInfo, JobInfo, RunInfo};

/// User information for authentication
#[derive(Debug, Clone)]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    pub password_hash: String,
    pub role: String,
    pub created_at: String,
    pub last_login_at: Option<String>,
}

/// Trigger information for scheduled jobs
#[derive(Debug, Clone)]
pub struct TriggerInfo {
    pub name: String,
    pub cron: String,
    pub job_id: String,
    pub enabled: bool,
}

/// Webhook event types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookEvent {
    Push,
    TagPush,
    PullRequest,
    MergeRequest,
    Note,
    Issue,
    Release,
}

impl std::fmt::Display for WebhookEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebhookEvent::Push => write!(f, "push"),
            WebhookEvent::TagPush => write!(f, "tag_push"),
            WebhookEvent::PullRequest => write!(f, "pull_request"),
            WebhookEvent::MergeRequest => write!(f, "merge_request"),
            WebhookEvent::Note => write!(f, "note"),
            WebhookEvent::Issue => write!(f, "issue"),
            WebhookEvent::Release => write!(f, "release"),
        }
    }
}

/// Webhook source platforms
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WebhookSource {
    Github,
    Gitlab,
    Gogs,
}

impl std::fmt::Display for WebhookSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebhookSource::Github => write!(f, "github"),
            WebhookSource::Gitlab => write!(f, "gitlab"),
            WebhookSource::Gogs => write!(f, "gogs"),
        }
    }
}

/// Webhook filter configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookFilter {
    /// Repository name pattern (supports * glob)
    pub repository: Option<String>,
    /// Branch patterns (supports * glob)
    pub branches: Vec<String>,
    /// Events to trigger on
    pub events: Vec<WebhookEvent>,
}

/// Webhook trigger information
#[derive(Debug, Clone)]
pub struct WebhookTriggerInfo {
    pub name: String,
    pub job_id: String,
    pub enabled: bool,
    pub secret: String,
    pub source: WebhookSource,
    pub filter: WebhookFilter,
    /// Associated VCS credential ID (optional)
    pub credential_id: Option<String>,
}

/// VCS credential information
#[derive(Debug, Clone)]
pub struct VcsCredentialInfo {
    pub id: String,
    pub name: String,
    pub vcs_type: WebhookSource,
    pub username: Option<String>,
    /// Encrypted credential (token or password)
    pub credential: String,
    pub created_at: String,
}

/// Repository trait for User operations
#[async_trait]
pub trait UserRepository: Send + Sync {
    /// Insert a new user
    async fn insert_user(&self, user: &UserInfo) -> Result<()>;

    /// Get a user by username
    async fn get_user_by_username(&self, username: &str) -> Result<Option<UserInfo>>;

    /// Update last login timestamp
    async fn update_last_login(&self, user_id: &str) -> Result<()>;

    /// List all users
    async fn list_users(&self) -> Result<Vec<UserInfo>>;
}

/// Repository trait for Job operations
#[async_trait]
pub trait JobRepository: Send + Sync {
    /// Insert a new job
    async fn insert_job(&self, job: &JobInfo) -> Result<()>;

    /// Get a job by ID
    async fn get_job(&self, job_id: &str) -> Result<Option<JobInfo>>;

    /// List all jobs
    async fn list_jobs(&self) -> Result<Vec<JobInfo>>;

    /// Get next build number for a job
    async fn next_build_num(&self, job_id: &str) -> Result<i64>;
}

/// Repository trait for Run operations
#[async_trait]
pub trait RunRepository: Send + Sync {
    /// Insert a new run
    async fn insert_run(
        &self,
        id: &str,
        job_id: &str,
        build_num: i64,
        status: &str,
        params: Option<&str>,
    ) -> Result<()>;

    /// Update run status
    async fn update_run_status(
        &self,
        run_id: &str,
        status: &str,
        exit_code: Option<i32>,
    ) -> Result<()>;

    /// Get a run by ID
    async fn get_run(&self, run_id: &str) -> Result<Option<RunInfo>>;

    /// List runs by status
    async fn list_runs_by_status(&self, status: &str) -> Result<Vec<RunInfo>>;

    /// Get run params by run ID (for queue recovery)
    async fn get_run_params(&self, run_id: &str) -> Result<HashMap<String, String>>;
}

/// Repository trait for Artifact operations
#[async_trait]
pub trait ArtifactRepository: Send + Sync {
    /// Insert an artifact
    async fn insert_artifact(
        &self,
        id: &str,
        run_id: &str,
        name: &str,
        size: i64,
        checksum: &str,
        storage_path: &str,
    ) -> Result<()>;

    /// Get artifact by ID
    async fn get_artifact(&self, artifact_id: &str) -> Result<Option<ArtifactInfo>>;

    /// List artifacts for a run
    async fn list_artifacts(&self, run_id: &str) -> Result<Vec<ArtifactInfo>>;
}

/// Repository trait for Trigger operations
#[async_trait]
pub trait TriggerRepository: Send + Sync {
    /// Insert or update a trigger
    async fn upsert_trigger(&self, trigger: &TriggerInfo) -> Result<()>;

    /// Get a trigger by name
    async fn get_trigger(&self, name: &str) -> Result<Option<TriggerInfo>>;

    /// List all triggers
    async fn list_triggers(&self) -> Result<Vec<TriggerInfo>>;

    /// Update trigger enabled status
    async fn set_trigger_enabled(&self, name: &str, enabled: bool) -> Result<()>;
}

/// Repository trait for Webhook operations
#[async_trait]
pub trait WebhookRepository: Send + Sync {
    /// Insert or update a webhook trigger
    async fn upsert_webhook_trigger(&self, webhook: &WebhookTriggerInfo) -> Result<()>;

    /// Get a webhook trigger by name
    async fn get_webhook_trigger(&self, name: &str) -> Result<Option<WebhookTriggerInfo>>;

    /// List all webhook triggers
    async fn list_webhook_triggers(&self) -> Result<Vec<WebhookTriggerInfo>>;

    /// List webhook triggers by source
    async fn list_webhook_triggers_by_source(
        &self,
        source: &WebhookSource,
    ) -> Result<Vec<WebhookTriggerInfo>>;

    /// Update webhook trigger enabled status
    async fn set_webhook_trigger_enabled(&self, name: &str, enabled: bool) -> Result<()>;

    /// Delete a webhook trigger
    async fn delete_webhook_trigger(&self, name: &str) -> Result<()>;
}

/// Repository trait for VCS Credential operations
#[async_trait]
pub trait VcsCredentialRepository: Send + Sync {
    /// Insert or update a VCS credential
    async fn upsert_credential(&self, cred: &VcsCredentialInfo) -> Result<()>;

    /// Get a credential by ID
    async fn get_credential(&self, id: &str) -> Result<Option<VcsCredentialInfo>>;

    /// List all credentials
    async fn list_credentials(&self) -> Result<Vec<VcsCredentialInfo>>;

    /// Delete a credential
    async fn delete_credential(&self, id: &str) -> Result<()>;
}

/// Combined repository for all entities
#[async_trait]
pub trait Repository:
    JobRepository
    + RunRepository
    + ArtifactRepository
    + UserRepository
    + TriggerRepository
    + WebhookRepository
    + VcsCredentialRepository
    + Send
    + Sync
{
    /// Run database migrations
    async fn migrate(&self) -> Result<()>;
}
