//! SQLite repository implementation
//!
//! Uses sqlx for async database operations with SQLite backend.

use async_trait::async_trait;
use sqlx::{sqlite::SqliteConnectOptions, FromRow, Pool, Sqlite, SqlitePool};
use std::collections::HashMap;
use std::str::FromStr;

use crate::error::{DbError, Result};
use ruci_protocol::{ArtifactInfo, JobInfo, RunInfo, RunStatus};

use super::repository::{
    ArtifactRepository, JobRepository, Repository, RunRepository, SessionInfo, SessionRepository,
    TriggerInfo, TriggerRepository, UserInfo, UserRepository, VcsCredentialInfo,
    VcsCredentialRepository, WebhookFilter, WebhookRepository, WebhookSource, WebhookTriggerInfo,
};

/// SQLite repository implementation
#[derive(Clone)]
pub struct SqliteRepository {
    pool: Pool<Sqlite>,
}

impl SqliteRepository {
    /// Create a new SQLite repository
    pub async fn new(database_url: &str) -> Result<Self> {
        let options = SqliteConnectOptions::from_str(database_url)
            .map_err(|e| DbError::Connection(e.to_string()))?
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(options)
            .await
            .map_err(|e| DbError::Connection(e.to_string()))?;

        Ok(Self { pool })
    }

    /// Run database migrations
    pub async fn migrate(&self) -> Result<()> {
        // For SQLite, we use inline migrations
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS jobs (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                filename TEXT NOT NULL,
                content TEXT NOT NULL,
                hash TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS runs (
                id TEXT PRIMARY KEY,
                job_id TEXT NOT NULL,
                build_num INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'QUEUED',
                params TEXT,
                started_at TEXT,
                finished_at TEXT,
                exit_code INTEGER,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                FOREIGN KEY (job_id) REFERENCES jobs(id)
            );

            CREATE TABLE IF NOT EXISTS artifacts (
                id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL,
                name TEXT NOT NULL,
                size INTEGER NOT NULL,
                checksum TEXT NOT NULL,
                storage_path TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
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
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                last_login_at TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_users_username ON users(username);

            CREATE TABLE IF NOT EXISTS triggers (
                name TEXT PRIMARY KEY,
                cron TEXT NOT NULL,
                job_id TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS webhook_triggers (
                name TEXT PRIMARY KEY,
                job_id TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                secret TEXT NOT NULL,
                source TEXT NOT NULL,
                filter TEXT NOT NULL,
                credential_id TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_webhook_triggers_source ON webhook_triggers(source);

            CREATE TABLE IF NOT EXISTS vcs_credentials (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                vcs_type TEXT NOT NULL,
                username TEXT,
                credential TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_vcs_credentials_name ON vcs_credentials(name);

            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                username TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                expires_at TEXT NOT NULL
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
impl JobRepository for SqliteRepository {
    async fn insert_job(&self, job: &JobInfo) -> Result<()> {
        sqlx::query("INSERT INTO jobs (id, name, filename, content, hash) VALUES (?, ?, ?, ?, ?)")
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
            "SELECT id, name, filename, content, hash, created_at FROM jobs WHERE id = ?",
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
            sqlx::query_as("SELECT MAX(build_num) FROM runs WHERE job_id = ?")
                .bind(job_id)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(row.0.unwrap_or(0) + 1)
    }
}

#[async_trait]
impl RunRepository for SqliteRepository {
    async fn insert_run(
        &self,
        id: &str,
        job_id: &str,
        build_num: i64,
        status: &str,
        params: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO runs (id, job_id, build_num, status, params) VALUES (?, ?, ?, ?, ?)",
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
                "UPDATE runs SET status = ?, exit_code = ?, finished_at = datetime('now') WHERE id = ?",
            )
            .bind(status)
            .bind(exit_code)
            .bind(run_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;
        } else if is_running {
            sqlx::query(
                "UPDATE runs SET status = ?, exit_code = ?, started_at = datetime('now'), finished_at = NULL WHERE id = ?",
            )
            .bind(status)
            .bind(exit_code)
            .bind(run_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;
        } else {
            sqlx::query(
                "UPDATE runs SET status = ?, exit_code = ?, finished_at = NULL WHERE id = ?",
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
            WHERE r.id = ?
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
            WHERE r.status = ?
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
        let row: Option<RunParamsRow> = sqlx::query_as("SELECT params FROM runs WHERE id = ?")
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
impl ArtifactRepository for SqliteRepository {
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
            "INSERT INTO artifacts (id, run_id, name, size, checksum, storage_path) VALUES (?, ?, ?, ?, ?, ?)",
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
            "SELECT id, run_id, name, size, checksum, storage_path, created_at FROM artifacts WHERE id = ?",
        )
        .bind(artifact_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(row.map(|r| r.into()))
    }

    async fn list_artifacts(&self, run_id: &str) -> Result<Vec<ArtifactInfo>> {
        let rows: Vec<ArtifactRow> = sqlx::query_as(
            "SELECT id, run_id, name, size, checksum, storage_path, created_at FROM artifacts WHERE run_id = ?",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }
}

#[async_trait]
impl UserRepository for SqliteRepository {
    async fn insert_user(&self, user: &UserInfo) -> Result<()> {
        sqlx::query("INSERT INTO users (id, username, password_hash, role) VALUES (?, ?, ?, ?)")
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
            "SELECT id, username, password_hash, role, created_at, last_login_at FROM users WHERE username = ?",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(row.map(|r| r.into()))
    }

    async fn update_last_login(&self, user_id: &str) -> Result<()> {
        sqlx::query("UPDATE users SET last_login_at = datetime('now') WHERE id = ?")
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
    created_at: String,
}

#[async_trait]
impl TriggerRepository for SqliteRepository {
    async fn upsert_trigger(&self, trigger: &TriggerInfo) -> Result<()> {
        sqlx::query(
            "INSERT INTO triggers (name, cron, job_id, enabled) VALUES (?, ?, ?, ?) ON CONFLICT(name) DO UPDATE SET cron = excluded.cron, job_id = excluded.job_id, enabled = excluded.enabled",
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
            "SELECT name, cron, job_id, enabled, created_at FROM triggers WHERE name = ?",
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
        sqlx::query("UPDATE triggers SET enabled = ? WHERE name = ?")
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
    created_at: String,
}

#[async_trait]
impl WebhookRepository for SqliteRepository {
    async fn upsert_webhook_trigger(&self, webhook: &WebhookTriggerInfo) -> Result<()> {
        let filter_json = serde_json::to_string(&webhook.filter)
            .map_err(|e| DbError::Query(format!("Failed to serialize filter: {}", e)))?;

        sqlx::query(
            "INSERT INTO webhook_triggers (name, job_id, enabled, secret, source, filter, credential_id) VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(name) DO UPDATE SET job_id = excluded.job_id, enabled = excluded.enabled, secret = excluded.secret, source = excluded.source, filter = excluded.filter, credential_id = excluded.credential_id",
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
            "SELECT name, job_id, enabled, secret, source, filter, credential_id, created_at FROM webhook_triggers WHERE name = ?",
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
            "SELECT name, job_id, enabled, secret, source, filter, credential_id, created_at FROM webhook_triggers WHERE source = ?",
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
        sqlx::query("UPDATE webhook_triggers SET enabled = ? WHERE name = ?")
            .bind(enabled as i32)
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    async fn delete_webhook_trigger(&self, name: &str) -> Result<()> {
        sqlx::query("DELETE FROM webhook_triggers WHERE name = ?")
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
    created_at: String,
}

#[async_trait]
impl VcsCredentialRepository for SqliteRepository {
    async fn upsert_credential(&self, cred: &VcsCredentialInfo) -> Result<()> {
        sqlx::query(
            "INSERT INTO vcs_credentials (id, name, vcs_type, username, credential) VALUES (?, ?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET name = excluded.name, vcs_type = excluded.vcs_type, username = excluded.username, credential = excluded.credential",
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
            "SELECT id, name, vcs_type, username, credential, created_at FROM vcs_credentials WHERE id = ?",
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
                    created_at: r.created_at,
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
                    created_at: r.created_at,
                })
            })
            .collect()
    }

    async fn delete_credential(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM vcs_credentials WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }
}

#[async_trait]
impl SessionRepository for SqliteRepository {
    async fn insert_session(&self, session: &SessionInfo) -> Result<()> {
        sqlx::query(
            "INSERT INTO sessions (id, user_id, username, created_at, expires_at) VALUES (?, ?, ?, ?, ?)",
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
            "SELECT id, user_id, username, created_at, expires_at FROM sessions WHERE id = ?",
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
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(())
    }

    async fn delete_expired_sessions(&self) -> Result<u64> {
        let result = sqlx::query("DELETE FROM sessions WHERE expires_at < datetime('now')")
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        Ok(result.rows_affected())
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

// Implement combined Repository trait for SqliteRepository
#[async_trait]
impl Repository for SqliteRepository {
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
    created_at: String,
}

impl From<JobRow> for JobInfo {
    fn from(row: JobRow) -> Self {
        let submitted_at =
            chrono::NaiveDateTime::parse_from_str(&row.created_at, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|ndt| ndt.and_utc())
                .unwrap_or_else(|| chrono::DateTime::from_timestamp(0, 0).unwrap());
        JobInfo {
            id: row.id,
            name: row.name.clone(),
            original_name: row.filename,
            submitted_at,
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
    started_at: Option<String>,
    finished_at: Option<String>,
    exit_code: Option<i32>,
    created_at: String,
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

#[derive(FromRow)]
#[allow(dead_code)]
struct ArtifactRow {
    id: String,
    run_id: String,
    name: String,
    size: i64,
    checksum: String,
    storage_path: String,
    created_at: String,
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
    created_at: String,
    last_login_at: Option<String>,
}

impl From<UserRow> for UserInfo {
    fn from(row: UserRow) -> Self {
        UserInfo {
            id: row.id,
            username: row.username,
            password_hash: row.password_hash,
            role: row.role,
            created_at: row.created_at,
            last_login_at: row.last_login_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruci_protocol::JobInfo;

    async fn create_test_repo() -> SqliteRepository {
        let repo = SqliteRepository::new("sqlite::memory:").await.unwrap();
        repo.migrate().await.unwrap();
        repo
    }

    // ═══════════════════════════════════════════════════════════════
    // JobRepository tests
    // ═══════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_insert_and_get_job() {
        let repo = create_test_repo().await;
        let job = JobInfo {
            id: "job-1".to_string(),
            name: "test-job".to_string(),
            original_name: ".ruci.yml".to_string(),
            submitted_at: chrono::Utc::now(),
        };

        repo.insert_job(&job).await.unwrap();
        let retrieved = repo.get_job("job-1").await.unwrap();

        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.id, "job-1");
        assert_eq!(retrieved.name, "test-job");
    }

    #[tokio::test]
    async fn test_get_nonexistent_job() {
        let repo = create_test_repo().await;
        let retrieved = repo.get_job("nonexistent").await.unwrap();
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_list_jobs() {
        let repo = create_test_repo().await;

        for i in 0..3 {
            let job = JobInfo {
                id: format!("job-{}", i),
                name: format!("job-{}", i),
                original_name: ".ruci.yml".to_string(),
                submitted_at: chrono::Utc::now(),
            };
            repo.insert_job(&job).await.unwrap();
        }

        let jobs = repo.list_jobs().await.unwrap();
        assert_eq!(jobs.len(), 3);
    }

    #[tokio::test]
    async fn test_list_jobs_empty() {
        let repo = create_test_repo().await;
        let jobs = repo.list_jobs().await.unwrap();
        assert!(jobs.is_empty());
    }

    #[tokio::test]
    async fn test_next_build_num_first_run() {
        let repo = create_test_repo().await;
        let job = JobInfo {
            id: "job-build".to_string(),
            name: "build-job".to_string(),
            original_name: ".ruci.yml".to_string(),
            submitted_at: chrono::Utc::now(),
        };
        repo.insert_job(&job).await.unwrap();

        let build_num = repo.next_build_num("job-build").await.unwrap();
        assert_eq!(build_num, 1);
    }

    // ═══════════════════════════════════════════════════════════════
    // RunRepository tests
    // ═══════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_insert_run() {
        let repo = create_test_repo().await;

        // Create a job first
        let job = JobInfo {
            id: "job-run".to_string(),
            name: "run-job".to_string(),
            original_name: ".ruci.yml".to_string(),
            submitted_at: chrono::Utc::now(),
        };
        repo.insert_job(&job).await.unwrap();

        // Insert run
        repo.insert_run("run-1", "job-run", 1, "QUEUED", None)
            .await
            .unwrap();

        let retrieved = repo.get_run("run-1").await.unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.id, "run-1");
        assert_eq!(retrieved.job_id, "job-run");
        assert_eq!(retrieved.build_num, 1);
        assert_eq!(retrieved.status, RunStatus::Queued);
    }

    #[tokio::test]
    async fn test_update_run_status() {
        let repo = create_test_repo().await;

        let job = JobInfo {
            id: "job-update".to_string(),
            name: "update-job".to_string(),
            original_name: ".ruci.yml".to_string(),
            submitted_at: chrono::Utc::now(),
        };
        repo.insert_job(&job).await.unwrap();

        repo.insert_run("run-update", "job-update", 1, "QUEUED", None)
            .await
            .unwrap();

        repo.update_run_status("run-update", "RUNNING", None)
            .await
            .unwrap();

        let retrieved = repo.get_run("run-update").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().status, RunStatus::Running);
    }

    #[tokio::test]
    async fn test_update_run_status_with_exit_code() {
        let repo = create_test_repo().await;

        let job = JobInfo {
            id: "job-exit".to_string(),
            name: "exit-job".to_string(),
            original_name: ".ruci.yml".to_string(),
            submitted_at: chrono::Utc::now(),
        };
        repo.insert_job(&job).await.unwrap();

        repo.insert_run("run-exit", "job-exit", 1, "QUEUED", None)
            .await
            .unwrap();

        repo.update_run_status("run-exit", "SUCCESS", Some(0))
            .await
            .unwrap();

        let retrieved = repo.get_run("run-exit").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().status, RunStatus::Success);
    }

    #[tokio::test]
    async fn test_list_runs_by_status() {
        let repo = create_test_repo().await;

        let job = JobInfo {
            id: "job-list".to_string(),
            name: "list-job".to_string(),
            original_name: ".ruci.yml".to_string(),
            submitted_at: chrono::Utc::now(),
        };
        repo.insert_job(&job).await.unwrap();

        repo.insert_run("run-1", "job-list", 1, "QUEUED", None)
            .await
            .unwrap();
        repo.insert_run("run-2", "job-list", 2, "RUNNING", None)
            .await
            .unwrap();
        repo.insert_run("run-3", "job-list", 3, "SUCCESS", None)
            .await
            .unwrap();

        let queued = repo.list_runs_by_status("QUEUED").await.unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].id, "run-1");

        let running = repo.list_runs_by_status("RUNNING").await.unwrap();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].id, "run-2");

        let success = repo.list_runs_by_status("SUCCESS").await.unwrap();
        assert_eq!(success.len(), 1);
        assert_eq!(success[0].id, "run-3");
    }

    #[tokio::test]
    async fn test_get_nonexistent_run() {
        let repo = create_test_repo().await;
        let retrieved = repo.get_run("nonexistent").await.unwrap();
        assert!(retrieved.is_none());
    }

    // ═══════════════════════════════════════════════════════════════
    // ArtifactRepository tests
    // ═══════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_insert_and_get_artifact() {
        let repo = create_test_repo().await;

        let job = JobInfo {
            id: "job-artifact".to_string(),
            name: "artifact-job".to_string(),
            original_name: ".ruci.yml".to_string(),
            submitted_at: chrono::Utc::now(),
        };
        repo.insert_job(&job).await.unwrap();

        repo.insert_run("run-artifact", "job-artifact", 1, "SUCCESS", None)
            .await
            .unwrap();

        repo.insert_artifact(
            "artifact-1",
            "run-artifact",
            "binary",
            1024,
            "abc123",
            "/var/lib/ruci/archive/binary",
        )
        .await
        .unwrap();

        let retrieved = repo.get_artifact("artifact-1").await.unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.id, "artifact-1");
        assert_eq!(retrieved.name, "binary");
        assert_eq!(retrieved.size, 1024);
        assert_eq!(retrieved.checksum, "abc123");
    }

    #[tokio::test]
    async fn test_list_artifacts() {
        let repo = create_test_repo().await;

        let job = JobInfo {
            id: "job-list-artifacts".to_string(),
            name: "list-artifacts-job".to_string(),
            original_name: ".ruci.yml".to_string(),
            submitted_at: chrono::Utc::now(),
        };
        repo.insert_job(&job).await.unwrap();

        repo.insert_run(
            "run-list-artifacts",
            "job-list-artifacts",
            1,
            "SUCCESS",
            None,
        )
        .await
        .unwrap();

        repo.insert_artifact("a1", "run-list-artifacts", "file1", 100, "c1", "/path/1")
            .await
            .unwrap();
        repo.insert_artifact("a2", "run-list-artifacts", "file2", 200, "c2", "/path/2")
            .await
            .unwrap();

        let artifacts = repo.list_artifacts("run-list-artifacts").await.unwrap();
        assert_eq!(artifacts.len(), 2);
    }

    #[tokio::test]
    async fn test_list_artifacts_empty() {
        let repo = create_test_repo().await;

        let job = JobInfo {
            id: "job-no-artifacts".to_string(),
            name: "no-artifacts-job".to_string(),
            original_name: ".ruci.yml".to_string(),
            submitted_at: chrono::Utc::now(),
        };
        repo.insert_job(&job).await.unwrap();

        repo.insert_run("run-no-artifacts", "job-no-artifacts", 1, "SUCCESS", None)
            .await
            .unwrap();

        let artifacts = repo.list_artifacts("run-no-artifacts").await.unwrap();
        assert!(artifacts.is_empty());
    }

    #[tokio::test]
    async fn test_get_nonexistent_artifact() {
        let repo = create_test_repo().await;
        let retrieved = repo.get_artifact("nonexistent").await.unwrap();
        assert!(retrieved.is_none());
    }

    // ═══════════════════════════════════════════════════════════════
    // Repository tests
    // ═══════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_repository_migrate() {
        let repo = create_test_repo().await;
        // migrate should be callable and succeed
        repo.migrate().await.unwrap();
    }

    #[tokio::test]
    async fn test_close() {
        let repo = create_test_repo().await;
        repo.close().await;
        // Close should not panic
    }

    // ═══════════════════════════════════════════════════════════════
    // Edge case tests
    // ═══════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_next_build_num_multiple_runs() {
        let repo = create_test_repo().await;

        let job = JobInfo {
            id: "job-multi-run".to_string(),
            name: "multi-run-job".to_string(),
            original_name: ".ruci.yml".to_string(),
            submitted_at: chrono::Utc::now(),
        };
        repo.insert_job(&job).await.unwrap();

        // Insert multiple runs
        repo.insert_run("run-1", "job-multi-run", 1, "QUEUED", None)
            .await
            .unwrap();
        repo.insert_run("run-2", "job-multi-run", 2, "RUNNING", None)
            .await
            .unwrap();
        repo.insert_run("run-3", "job-multi-run", 3, "SUCCESS", None)
            .await
            .unwrap();

        let build_num = repo.next_build_num("job-multi-run").await.unwrap();
        assert_eq!(build_num, 4); // Next should be 4
    }

    #[tokio::test]
    async fn test_update_run_status_unknown_status() {
        let repo = create_test_repo().await;

        let job = JobInfo {
            id: "job-unknown-status".to_string(),
            name: "unknown-status-job".to_string(),
            original_name: ".ruci.yml".to_string(),
            submitted_at: chrono::Utc::now(),
        };
        repo.insert_job(&job).await.unwrap();

        repo.insert_run("run-unknown", "job-unknown-status", 1, "QUEUED", None)
            .await
            .unwrap();

        // Update with some arbitrary status
        repo.update_run_status("run-unknown", "CUSTOM_STATUS", Some(0))
            .await
            .unwrap();

        // RunInfo from unknown status should map to Failed
        let retrieved = repo.get_run("run-unknown").await.unwrap();
        assert!(retrieved.is_some());
        let run_info = retrieved.unwrap();
        assert_eq!(run_info.status, RunStatus::Failed);
    }

    #[tokio::test]
    async fn test_insert_run_for_nonexistent_job() {
        let repo = create_test_repo().await;

        // Try to insert a run without a corresponding job
        // This will violate foreign key constraint in real DB,
        // but SQLite might not enforce it strictly with the current schema
        let result = repo
            .insert_run("orphan-run", "nonexistent-job", 1, "QUEUED", None)
            .await;

        // Depending on FK enforcement, this might succeed or fail
        // The key is it shouldn't panic
        if result.is_err() {
            // If it fails due to FK constraint, that's expected behavior
            let err_msg = result.unwrap_err().to_string();
            assert!(err_msg.contains("foreign key") || err_msg.contains("constraint"));
        }
    }

    #[tokio::test]
    async fn test_list_runs_by_status_none_found() {
        let repo = create_test_repo().await;

        let job = JobInfo {
            id: "job-no-queued".to_string(),
            name: "no-queued-job".to_string(),
            original_name: ".ruci.yml".to_string(),
            submitted_at: chrono::Utc::now(),
        };
        repo.insert_job(&job).await.unwrap();

        repo.insert_run("run-1", "job-no-queued", 1, "SUCCESS", None)
            .await
            .unwrap();

        let queued = repo.list_runs_by_status("QUEUED").await.unwrap();
        assert!(queued.is_empty());

        let running = repo.list_runs_by_status("RUNNING").await.unwrap();
        assert!(running.is_empty());
    }

    #[tokio::test]
    async fn test_multiple_artifacts_for_same_run() {
        let repo = create_test_repo().await;

        let job = JobInfo {
            id: "job-multi-artifact".to_string(),
            name: "multi-artifact-job".to_string(),
            original_name: ".ruci.yml".to_string(),
            submitted_at: chrono::Utc::now(),
        };
        repo.insert_job(&job).await.unwrap();

        repo.insert_run(
            "run-multi-artifact",
            "job-multi-artifact",
            1,
            "SUCCESS",
            None,
        )
        .await
        .unwrap();

        // Insert multiple artifacts
        repo.insert_artifact("a1", "run-multi-artifact", "file1.txt", 100, "c1", "/p1")
            .await
            .unwrap();
        repo.insert_artifact("a2", "run-multi-artifact", "file2.txt", 200, "c2", "/p2")
            .await
            .unwrap();
        repo.insert_artifact("a3", "run-multi-artifact", "file3.txt", 300, "c3", "/p3")
            .await
            .unwrap();

        let artifacts = repo.list_artifacts("run-multi-artifact").await.unwrap();
        assert_eq!(artifacts.len(), 3);

        // Verify we can retrieve each individually
        let a1 = repo.get_artifact("a1").await.unwrap();
        assert!(a1.is_some());
        assert_eq!(a1.unwrap().name, "file1.txt");
    }
}
