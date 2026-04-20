//! PostgreSQL repository implementation
//!
//! Uses sqlx for async database operations with PostgreSQL backend.

use async_trait::async_trait;
use sqlx::{FromRow, PgPool, Pool, Postgres};
use std::collections::HashMap;

use crate::error::{DbError, Result};
use ruci_protocol::{ArtifactInfo, JobInfo, RunInfo, RunStatus};

use super::repository::{
    ArtifactRepository, JobRepository, Repository, RunRepository, SessionInfo, SessionRepository,
    TriggerInfo, TriggerRepository, UserInfo, UserRepository, VcsCredentialInfo,
    VcsCredentialRepository, WebhookFilter, WebhookRepository, WebhookSource, WebhookTriggerInfo,
};

/// PostgreSQL repository implementation
#[derive(Clone)]
pub struct PostgresRepository {
    pool: Pool<Postgres>,
}

impl PostgresRepository {
    /// Create a new PostgreSQL repository
    pub async fn new(database_url: &str) -> Result<Self> {
        let pool = PgPool::connect(database_url)
            .await
            .map_err(|e| DbError::Connection(e.to_string()))?;

        Ok(Self { pool })
    }

    /// Run database migrations
    pub async fn migrate(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS jobs (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                filename TEXT NOT NULL,
                content TEXT NOT NULL,
                hash TEXT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );

            CREATE TABLE IF NOT EXISTS runs (
                id TEXT PRIMARY KEY,
                job_id TEXT NOT NULL,
                build_num INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'QUEUED',
                params TEXT,
                exit_code INTEGER,
                started_at TIMESTAMPTZ,
                finished_at TIMESTAMPTZ,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                FOREIGN KEY (job_id) REFERENCES jobs(id)
            );

            CREATE TABLE IF NOT EXISTS artifacts (
                id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL,
                name TEXT NOT NULL,
                size BIGINT NOT NULL,
                checksum TEXT NOT NULL,
                storage_path TEXT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                FOREIGN KEY (run_id) REFERENCES runs(id)
            );

            CREATE INDEX IF NOT EXISTS idx_runs_job_id ON runs(job_id);
            CREATE INDEX IF NOT EXISTS idx_runs_status ON runs(status);
            CREATE INDEX IF NOT EXISTS idx_artifacts_run_id ON artifacts(run_id);

            CREATE TABLE IF NOT EXISTS users (
                id TEXT PRIMARY KEY,
                username TEXT NOT NULL UNIQUE,
                password_hash TEXT NOT NULL,
                role TEXT NOT NULL DEFAULT 'user',
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                last_login_at TIMESTAMPTZ
            );

            CREATE INDEX IF NOT EXISTS idx_users_username ON users(username);

            CREATE TABLE IF NOT EXISTS triggers (
                name TEXT PRIMARY KEY,
                cron TEXT NOT NULL,
                job_id TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );

            CREATE TABLE IF NOT EXISTS webhook_triggers (
                name TEXT PRIMARY KEY,
                job_id TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                secret TEXT NOT NULL,
                source TEXT NOT NULL,
                filter TEXT NOT NULL,
                credential_id TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );

            CREATE INDEX IF NOT EXISTS idx_webhook_triggers_source ON webhook_triggers(source);

            CREATE TABLE IF NOT EXISTS vcs_credentials (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                vcs_type TEXT NOT NULL,
                username TEXT,
                credential TEXT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );

            CREATE INDEX IF NOT EXISTS idx_vcs_credentials_name ON vcs_credentials(name);

            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                username TEXT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                expires_at TIMESTAMPTZ NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_sessions_expires_at ON sessions(expires_at);
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    /// Close the database connection pool
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

#[async_trait]
impl JobRepository for PostgresRepository {
    async fn insert_job(&self, job: &JobInfo) -> Result<()> {
        sqlx::query(
            "INSERT INTO jobs (id, name, filename, content, hash) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&job.id)
        .bind(&job.name)
        .bind(".ruci.yml")
        .bind("") // content not stored in this struct
        .bind(&job.id) // hash uses id as placeholder
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    async fn get_job(&self, job_id: &str) -> Result<Option<JobInfo>> {
        let row: Option<JobRow> = sqlx::query_as(
            "SELECT id, name, filename, content, hash, created_at FROM jobs WHERE id = $1",
        )
        .bind(job_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(row.map(|r| r.into()))
    }

    async fn list_jobs(&self) -> Result<Vec<JobInfo>> {
        let rows: Vec<JobRow> = sqlx::query_as(
            "SELECT id, name, filename, content, hash, created_at FROM jobs ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn next_build_num(&self, job_id: &str) -> Result<i64> {
        let row: (Option<i64>,) =
            sqlx::query_as("SELECT MAX(build_num) FROM runs WHERE job_id = $1")
                .bind(job_id)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(row.0.unwrap_or(0) + 1)
    }
}

#[async_trait]
impl RunRepository for PostgresRepository {
    async fn insert_run(
        &self,
        id: &str,
        job_id: &str,
        build_num: i64,
        status: &str,
        params: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO runs (id, job_id, build_num, status, params) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(id)
        .bind(job_id)
        .bind(build_num)
        .bind(status)
        .bind(params)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    async fn update_run_status(
        &self,
        run_id: &str,
        status: &str,
        exit_code: Option<i32>,
    ) -> Result<()> {
        let is_terminal = matches!(status, "SUCCESS" | "FAILED" | "ABORTED");
        let is_running = status == "RUNNING";

        if is_terminal {
            sqlx::query(
                "UPDATE runs SET status = $1, exit_code = $2, finished_at = NOW() WHERE id = $3",
            )
            .bind(status)
            .bind(exit_code)
            .bind(run_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;
        } else if is_running {
            sqlx::query(
                "UPDATE runs SET status = $1, exit_code = $2, started_at = NOW(), finished_at = NULL WHERE id = $3",
            )
            .bind(status)
            .bind(exit_code)
            .bind(run_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;
        } else {
            sqlx::query(
                "UPDATE runs SET status = $1, exit_code = $2, finished_at = NULL WHERE id = $3",
            )
            .bind(status)
            .bind(exit_code)
            .bind(run_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;
        }

        Ok(())
    }

    async fn get_run(&self, run_id: &str) -> Result<Option<RunInfo>> {
        let row: Option<RunRow> = sqlx::query_as(
            r#"
            SELECT r.id, r.job_id, j.name as job_name, r.build_num, r.status,
                   r.started_at, r.finished_at, r.exit_code, r.created_at
            FROM runs r
            JOIN jobs j ON r.job_id = j.id
            WHERE r.id = $1
            "#,
        )
        .bind(run_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(row.map(|r| r.into()))
    }

    async fn list_runs_by_status(&self, status: &str) -> Result<Vec<RunInfo>> {
        let rows: Vec<RunRow> = sqlx::query_as(
            r#"
            SELECT r.id, r.job_id, j.name as job_name, r.build_num, r.status,
                   r.started_at, r.finished_at, r.exit_code, r.created_at
            FROM runs r
            JOIN jobs j ON r.job_id = j.id
            WHERE r.status = $1
            ORDER BY r.build_num ASC
            "#,
        )
        .bind(status)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn get_run_params(&self, run_id: &str) -> Result<HashMap<String, String>> {
        let row: Option<RunParamsRow> = sqlx::query_as("SELECT params FROM runs WHERE id = $1")
            .bind(run_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        match row {
            Some(r) => match r.params {
                Some(p) if !p.is_empty() => {
                    let params: HashMap<String, String> = serde_json::from_str(&p)
                        .map_err(|e| DbError::Query(format!("Failed to parse params: {}", e)))?;
                    Ok(params)
                }
                _ => Ok(HashMap::new()),
            },
            None => Ok(HashMap::new()),
        }
    }
}

#[derive(FromRow)]
#[allow(dead_code)]
struct RunParamsRow {
    params: Option<String>,
}

#[async_trait]
impl ArtifactRepository for PostgresRepository {
    async fn insert_artifact(
        &self,
        id: &str,
        run_id: &str,
        name: &str,
        size: i64,
        checksum: &str,
        storage_path: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO artifacts (id, run_id, name, size, checksum, storage_path) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(id)
        .bind(run_id)
        .bind(name)
        .bind(size)
        .bind(checksum)
        .bind(storage_path)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    async fn get_artifact(&self, artifact_id: &str) -> Result<Option<ArtifactInfo>> {
        let row: Option<ArtifactRow> = sqlx::query_as(
            "SELECT id, run_id, name, size, checksum, storage_path, created_at FROM artifacts WHERE id = $1",
        )
        .bind(artifact_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(row.map(|r| r.into()))
    }

    async fn list_artifacts(&self, run_id: &str) -> Result<Vec<ArtifactInfo>> {
        let rows: Vec<ArtifactRow> = sqlx::query_as(
            "SELECT id, run_id, name, size, checksum, storage_path, created_at FROM artifacts WHERE run_id = $1",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }
}

#[async_trait]
impl UserRepository for PostgresRepository {
    async fn insert_user(&self, user: &UserInfo) -> Result<()> {
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, role) VALUES ($1, $2, $3, $4)",
        )
        .bind(&user.id)
        .bind(&user.username)
        .bind(&user.password_hash)
        .bind(&user.role)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    async fn get_user_by_username(&self, username: &str) -> Result<Option<UserInfo>> {
        let row: Option<UserRow> = sqlx::query_as(
            "SELECT id, username, password_hash, role, created_at, last_login_at FROM users WHERE username = $1",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(row.map(|r| r.into()))
    }

    async fn update_last_login(&self, user_id: &str) -> Result<()> {
        sqlx::query("UPDATE users SET last_login_at = NOW() WHERE id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    async fn list_users(&self) -> Result<Vec<UserInfo>> {
        let rows: Vec<UserRow> = sqlx::query_as(
            "SELECT id, username, password_hash, role, created_at, last_login_at FROM users",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }
}

#[derive(FromRow)]
#[allow(dead_code)]
struct TriggerRow {
    name: String,
    cron: String,
    job_id: String,
    enabled: i32,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[async_trait]
impl TriggerRepository for PostgresRepository {
    async fn upsert_trigger(&self, trigger: &TriggerInfo) -> Result<()> {
        sqlx::query(
            "INSERT INTO triggers (name, cron, job_id, enabled) VALUES ($1, $2, $3, $4) ON CONFLICT(name) DO UPDATE SET cron = excluded.cron, job_id = excluded.job_id, enabled = excluded.enabled",
        )
        .bind(&trigger.name)
        .bind(&trigger.cron)
        .bind(&trigger.job_id)
        .bind(trigger.enabled as i32)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    async fn get_trigger(&self, name: &str) -> Result<Option<TriggerInfo>> {
        let row: Option<TriggerRow> = sqlx::query_as(
            "SELECT name, cron, job_id, enabled, created_at FROM triggers WHERE name = $1",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(row.map(|r| r.into()))
    }

    async fn list_triggers(&self) -> Result<Vec<TriggerInfo>> {
        let rows: Vec<TriggerRow> =
            sqlx::query_as("SELECT name, cron, job_id, enabled, created_at FROM triggers")
                .fetch_all(&self.pool)
                .await
                .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn set_trigger_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        sqlx::query("UPDATE triggers SET enabled = $1 WHERE name = $2")
            .bind(enabled as i32)
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }
}

#[derive(FromRow)]
#[allow(dead_code)]
struct WebhookRow {
    name: String,
    job_id: String,
    enabled: i32,
    secret: String,
    source: String,
    filter: String,
    credential_id: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[async_trait]
impl WebhookRepository for PostgresRepository {
    async fn upsert_webhook_trigger(&self, webhook: &WebhookTriggerInfo) -> Result<()> {
        let filter_json = serde_json::to_string(&webhook.filter)
            .map_err(|e| DbError::Query(format!("Failed to serialize filter: {}", e)))?;

        sqlx::query(
            "INSERT INTO webhook_triggers (name, job_id, enabled, secret, source, filter, credential_id) VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT(name) DO UPDATE SET job_id = excluded.job_id, enabled = excluded.enabled, secret = excluded.secret, source = excluded.source, filter = excluded.filter, credential_id = excluded.credential_id",
        )
        .bind(&webhook.name)
        .bind(&webhook.job_id)
        .bind(webhook.enabled as i32)
        .bind(&webhook.secret)
        .bind(webhook.source.to_string())
        .bind(&filter_json)
        .bind(&webhook.credential_id)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    async fn get_webhook_trigger(&self, name: &str) -> Result<Option<WebhookTriggerInfo>> {
        let row: Option<WebhookRow> = sqlx::query_as(
            "SELECT name, job_id, enabled, secret, source, filter, credential_id, created_at FROM webhook_triggers WHERE name = $1",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        match row {
            Some(r) => {
                let filter: WebhookFilter = serde_json::from_str(&r.filter)
                    .map_err(|e| DbError::Query(format!("Failed to parse filter: {}", e)))?;
                let source = match r.source.as_str() {
                    "github" => WebhookSource::Github,
                    "gitlab" => WebhookSource::Gitlab,
                    "gogs" => WebhookSource::Gogs,
                    _ => return Err(DbError::Query(format!("Unknown source: {}", r.source)).into()),
                };
                Ok(Some(WebhookTriggerInfo {
                    name: r.name,
                    job_id: r.job_id,
                    enabled: r.enabled != 0,
                    secret: r.secret,
                    source,
                    filter,
                    credential_id: r.credential_id,
                }))
            }
            None => Ok(None),
        }
    }

    async fn list_webhook_triggers(&self) -> Result<Vec<WebhookTriggerInfo>> {
        let rows: Vec<WebhookRow> = sqlx::query_as(
            "SELECT name, job_id, enabled, secret, source, filter, credential_id, created_at FROM webhook_triggers",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        rows.into_iter()
            .map(|r| {
                let filter: WebhookFilter = serde_json::from_str(&r.filter)
                    .map_err(|e| DbError::Query(format!("Failed to parse filter: {}", e)))?;
                let source = match r.source.as_str() {
                    "github" => WebhookSource::Github,
                    "gitlab" => WebhookSource::Gitlab,
                    "gogs" => WebhookSource::Gogs,
                    _ => return Err(DbError::Query(format!("Unknown source: {}", r.source)).into()),
                };
                Ok(WebhookTriggerInfo {
                    name: r.name,
                    job_id: r.job_id,
                    enabled: r.enabled != 0,
                    secret: r.secret,
                    source,
                    filter,
                    credential_id: r.credential_id,
                })
            })
            .collect()
    }

    async fn list_webhook_triggers_by_source(
        &self,
        source: &WebhookSource,
    ) -> Result<Vec<WebhookTriggerInfo>> {
        let rows: Vec<WebhookRow> = sqlx::query_as(
            "SELECT name, job_id, enabled, secret, source, filter, credential_id, created_at FROM webhook_triggers WHERE source = $1",
        )
        .bind(source.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        rows.into_iter()
            .map(|r| {
                let filter: WebhookFilter = serde_json::from_str(&r.filter)
                    .map_err(|e| DbError::Query(format!("Failed to parse filter: {}", e)))?;
                let source = match r.source.as_str() {
                    "github" => WebhookSource::Github,
                    "gitlab" => WebhookSource::Gitlab,
                    "gogs" => WebhookSource::Gogs,
                    _ => return Err(DbError::Query(format!("Unknown source: {}", r.source)).into()),
                };
                Ok(WebhookTriggerInfo {
                    name: r.name,
                    job_id: r.job_id,
                    enabled: r.enabled != 0,
                    secret: r.secret,
                    source,
                    filter,
                    credential_id: r.credential_id,
                })
            })
            .collect()
    }

    async fn set_webhook_trigger_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        sqlx::query("UPDATE webhook_triggers SET enabled = $1 WHERE name = $2")
            .bind(enabled as i32)
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    async fn delete_webhook_trigger(&self, name: &str) -> Result<()> {
        sqlx::query("DELETE FROM webhook_triggers WHERE name = $1")
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }
}

#[derive(FromRow)]
#[allow(dead_code)]
struct VcsCredentialRow {
    id: String,
    name: String,
    vcs_type: String,
    username: Option<String>,
    credential: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[async_trait]
impl VcsCredentialRepository for PostgresRepository {
    async fn upsert_credential(&self, cred: &VcsCredentialInfo) -> Result<()> {
        sqlx::query(
            "INSERT INTO vcs_credentials (id, name, vcs_type, username, credential) VALUES ($1, $2, $3, $4, $5) ON CONFLICT(id) DO UPDATE SET name = excluded.name, vcs_type = excluded.vcs_type, username = excluded.username, credential = excluded.credential",
        )
        .bind(&cred.id)
        .bind(&cred.name)
        .bind(cred.vcs_type.to_string())
        .bind(&cred.username)
        .bind(&cred.credential)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    async fn get_credential(&self, id: &str) -> Result<Option<VcsCredentialInfo>> {
        let row: Option<VcsCredentialRow> = sqlx::query_as(
            "SELECT id, name, vcs_type, username, credential, created_at FROM vcs_credentials WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        match row {
            Some(r) => {
                let vcs_type = match r.vcs_type.as_str() {
                    "github" => WebhookSource::Github,
                    "gitlab" => WebhookSource::Gitlab,
                    "gogs" => WebhookSource::Gogs,
                    _ => {
                        return Err(
                            DbError::Query(format!("Unknown vcs_type: {}", r.vcs_type)).into()
                        )
                    }
                };
                Ok(Some(VcsCredentialInfo {
                    id: r.id,
                    name: r.name,
                    vcs_type,
                    username: r.username,
                    credential: r.credential,
                    created_at: r.created_at.to_rfc3339(),
                }))
            }
            None => Ok(None),
        }
    }

    async fn list_credentials(&self) -> Result<Vec<VcsCredentialInfo>> {
        let rows: Vec<VcsCredentialRow> = sqlx::query_as(
            "SELECT id, name, vcs_type, username, credential, created_at FROM vcs_credentials",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        rows.into_iter()
            .map(|r| {
                let vcs_type = match r.vcs_type.as_str() {
                    "github" => WebhookSource::Github,
                    "gitlab" => WebhookSource::Gitlab,
                    "gogs" => WebhookSource::Gogs,
                    _ => {
                        return Err(
                            DbError::Query(format!("Unknown vcs_type: {}", r.vcs_type)).into()
                        )
                    }
                };
                Ok(VcsCredentialInfo {
                    id: r.id,
                    name: r.name,
                    vcs_type,
                    username: r.username,
                    credential: r.credential,
                    created_at: r.created_at.to_rfc3339(),
                })
            })
            .collect()
    }

    async fn delete_credential(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM vcs_credentials WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }
}

impl From<TriggerRow> for TriggerInfo {
    fn from(row: TriggerRow) -> Self {
        TriggerInfo {
            name: row.name,
            cron: row.cron,
            job_id: row.job_id,
            enabled: row.enabled != 0,
        }
    }
}

#[async_trait]
impl SessionRepository for PostgresRepository {
    async fn insert_session(&self, session: &SessionInfo) -> Result<()> {
        sqlx::query(
            "INSERT INTO sessions (id, user_id, username, created_at, expires_at) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&session.id)
        .bind(&session.user_id)
        .bind(&session.username)
        .bind(&session.created_at)
        .bind(&session.expires_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    async fn get_session(&self, session_id: &str) -> Result<Option<SessionInfo>> {
        let row: Option<(String, String, String, String, String)> = sqlx::query_as(
            "SELECT id, user_id, username, created_at, expires_at FROM sessions WHERE id = $1",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(row.map(|r| SessionInfo {
            id: r.0,
            user_id: r.1,
            username: r.2,
            created_at: r.3,
            expires_at: r.4,
        }))
    }

    async fn delete_session(&self, session_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(session_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    async fn delete_expired_sessions(&self) -> Result<u64> {
        let result = sqlx::query("DELETE FROM sessions WHERE expires_at < NOW()")
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(result.rows_affected())
    }
}

// Implement combined Repository trait for PostgresRepository
#[async_trait]
impl Repository for PostgresRepository {
    async fn migrate(&self) -> Result<()> {
        self.migrate().await
    }
}

// Helper row types for sqlx
#[derive(FromRow)]
#[allow(dead_code)]
struct JobRow {
    id: String,
    name: String,
    filename: String,
    content: String,
    hash: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl From<JobRow> for JobInfo {
    fn from(row: JobRow) -> Self {
        JobInfo {
            id: row.id,
            name: row.name.clone(),
            original_name: row.filename,
            submitted_at: row.created_at,
        }
    }
}

#[derive(FromRow)]
#[allow(dead_code)]
struct RunRow {
    id: String,
    job_id: String,
    job_name: String,
    build_num: i64,
    status: String,
    started_at: Option<chrono::DateTime<chrono::Utc>>,
    finished_at: Option<chrono::DateTime<chrono::Utc>>,
    exit_code: Option<i32>,
    created_at: chrono::DateTime<chrono::Utc>,
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
            started_at: row.started_at,
            finished_at: row.finished_at,
            exit_code: row.exit_code,
        }
    }
}

#[derive(FromRow)]
#[allow(dead_code)]
struct ArtifactRow {
    id: String,
    run_id: String,
    name: String,
    size: i64,
    checksum: String,
    storage_path: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl From<ArtifactRow> for ArtifactInfo {
    fn from(row: ArtifactRow) -> Self {
        ArtifactInfo {
            id: row.id,
            run_id: row.run_id,
            name: row.name,
            size: row.size as u64,
            checksum: row.checksum,
            storage_path: row.storage_path,
        }
    }
}

#[derive(FromRow)]
#[allow(dead_code)]
struct UserRow {
    id: String,
    username: String,
    password_hash: String,
    role: String,
    created_at: chrono::DateTime<chrono::Utc>,
    last_login_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<UserRow> for UserInfo {
    fn from(row: UserRow) -> Self {
        UserInfo {
            id: row.id,
            username: row.username,
            password_hash: row.password_hash,
            role: row.role,
            created_at: row.created_at.to_string(),
            last_login_at: row.last_login_at.map(|dt| dt.to_string()),
        }
    }
}
