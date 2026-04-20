//! Database module
//!
//! Provides database abstraction through the Repository pattern.
//! Supports multiple database backends (SQLite, PostgreSQL, MySQL).

pub mod mysql;
pub mod postgres;
pub mod repository;
pub mod sqlite;

pub use mysql::MysqlRepository;
pub use postgres::PostgresRepository;
pub use repository::{
    ArtifactRepository, JobRepository, Repository, RunRepository, SessionInfo,
    SessionRepository, TriggerRepository, UserRepository, VcsCredentialInfo,
    VcsCredentialRepository, WebhookEvent, WebhookFilter, WebhookRepository, WebhookSource,
    WebhookTriggerInfo,
};
pub use sqlite::SqliteRepository;

use crate::error::Result;
use std::sync::Arc;
use url::Url;

#[derive(Debug)]
pub enum DatabaseKind {
    Sqlite,
    Postgres,
    MySql,
}

impl DatabaseKind {
    pub fn from_url(url: &str) -> Result<Self> {
        let parsed = Url::parse(url).map_err(|e| {
            crate::error::Error::Database(crate::error::DbError::Connection(format!(
                "Invalid URL format: {}",
                e
            )))
        })?;

        match parsed.scheme() {
            "sqlite" => Ok(DatabaseKind::Sqlite),
            "postgres" | "postgresql" => Ok(DatabaseKind::Postgres),
            "mysql" => Ok(DatabaseKind::MySql),
            _ => Err(crate::error::Error::Database(
                crate::error::DbError::Connection(format!(
                    "Unsupported database scheme: {}",
                    parsed.scheme()
                )),
            )),
        }
    }
}

/// Create a repository based on the database URL scheme
///
/// - `path/to/db.sqlite` or `:memory:` -> SqliteRepository (file path)
/// - `postgres://user:pass@host/db` -> PostgreSQLRepository
/// - `mysql://user:pass@host/db` -> MySQLRepository
pub async fn create_repository(database_url: &str) -> Result<Arc<dyn Repository>> {
    if database_url.starts_with("sqlite://")
        || database_url.starts_with("postgres://")
        || database_url.starts_with("postgresql://")
        || database_url.starts_with("mysql://")
    {
        let kind = DatabaseKind::from_url(database_url)?;
        match kind {
            DatabaseKind::Sqlite => {
                let parsed = Url::parse(database_url).map_err(|e| {
                    crate::error::Error::Database(crate::error::DbError::Connection(format!(
                        "Invalid SQLite URL: {}",
                        e
                    )))
                })?;
                let file_path = parsed.path();
                if file_path.is_empty() || file_path == "/" {
                    return Err(crate::error::Error::Database(
                        crate::error::DbError::Connection("Empty SQLite file path".to_string()),
                    ));
                }
                let repo = SqliteRepository::new(file_path).await?;
                Ok(Arc::new(repo))
            }
            DatabaseKind::Postgres => {
                let repo = PostgresRepository::new(database_url).await?;
                Ok(Arc::new(repo))
            }
            DatabaseKind::MySql => {
                let repo = MysqlRepository::new(database_url).await?;
                Ok(Arc::new(repo))
            }
        }
    } else {
        let repo = SqliteRepository::new(database_url).await?;
        Ok(Arc::new(repo))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_kind_from_url() {
        assert!(matches!(
            DatabaseKind::from_url("sqlite://test.db"),
            Ok(DatabaseKind::Sqlite)
        ));
        assert!(matches!(
            DatabaseKind::from_url("postgres://user:pass@localhost/db"),
            Ok(DatabaseKind::Postgres)
        ));
        assert!(matches!(
            DatabaseKind::from_url("postgresql://localhost/db"),
            Ok(DatabaseKind::Postgres)
        ));
        assert!(matches!(
            DatabaseKind::from_url("mysql://localhost/db"),
            Ok(DatabaseKind::MySql)
        ));

        let result = DatabaseKind::from_url("invalid://test");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unsupported"));

        let result = DatabaseKind::from_url("not-a-url");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid URL"));
    }

    #[tokio::test]
    async fn test_create_repository_sqlite_path() {
        let repo = create_repository(":memory:").await.unwrap();
        let _ = repo;
    }

    #[tokio::test]
    async fn test_create_repository_sqlite_url() {
        let repo = create_repository("sqlite://test_url.db").await.unwrap();
        let _ = repo;
        let _ = std::fs::remove_file("test_url.db");
    }
}
