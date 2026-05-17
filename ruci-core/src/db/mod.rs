//! Database module
//!
//! Provides database abstraction through the Repository pattern.
//! Supports multiple database backends (SQLite, PostgreSQL, MySQL).

#[cfg(feature = "mysql")]
pub mod mysql;
#[cfg(feature = "postgres")]
pub mod postgres;
pub mod repository;
pub mod sqlite;

#[cfg(feature = "mysql")]
pub use mysql::MysqlRepository;
#[cfg(feature = "postgres")]
pub use postgres::PostgresRepository;
pub use repository::{
    ApiTokenInfo, ApiTokenRepository, ArtifactRepository, JobRepository, Repository, RunRepository,
    SessionInfo, SessionRepository, TriggerInfo, TriggerRepository, UserRepository,
    VcsCredentialInfo, VcsCredentialRepository, WebhookEvent, WebhookFilter, WebhookRepository,
    WebhookSource, WebhookTriggerInfo,
};
pub use sqlite::SqliteRepository;

use crate::error::Result;
use std::sync::Arc;
use url::Url;

#[derive(Debug)]
pub enum DatabaseKind {
    Sqlite,
    #[cfg(feature = "postgres")]
    Postgres,
    #[cfg(feature = "mysql")]
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
            #[cfg(feature = "postgres")]
            "postgres" | "postgresql" => Ok(DatabaseKind::Postgres),
            #[cfg(feature = "mysql")]
            "mysql" => Ok(DatabaseKind::MySql),
            scheme => {
                #[allow(unused_mut)]
                let mut hint = String::new();
                #[cfg(not(feature = "postgres"))]
                if scheme == "postgres" || scheme == "postgresql" {
                    hint = " (compile with 'postgres' feature to enable)".to_string();
                }
                #[cfg(not(feature = "mysql"))]
                if scheme == "mysql" {
                    hint = " (compile with 'mysql' feature to enable)".to_string();
                }
                Err(crate::error::Error::Database(
                    crate::error::DbError::Connection(format!(
                        "Unsupported database scheme: {}{}",
                        scheme, hint
                    )),
                ))
            }
        }
    }
}

/// Create a repository based on the database URL scheme
///
/// - `sqlite://path/to/db.sqlite` or `:memory:` -> SqliteRepository
/// - `postgres://user:pass@host/db` -> PostgreSQLRepository (requires `postgres` feature)
/// - `mysql://user:pass@host/db` -> MySQLRepository (requires `mysql` feature)
pub async fn create_repository(database_url: &str) -> Result<Arc<dyn Repository>> {
    if database_url == ":memory:" {
        let repo = SqliteRepository::new(database_url).await?;
        Ok(Arc::new(repo))
    } else if database_url.starts_with("sqlite://") {
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
    } else {
        let kind = DatabaseKind::from_url(database_url)?;
        match kind {
            DatabaseKind::Sqlite => {
                let repo = SqliteRepository::new(database_url).await?;
                Ok(Arc::new(repo))
            }
            #[cfg(feature = "postgres")]
            DatabaseKind::Postgres => {
                let repo = PostgresRepository::new(database_url).await?;
                Ok(Arc::new(repo))
            }
            #[cfg(feature = "mysql")]
            DatabaseKind::MySql => {
                let repo = MysqlRepository::new(database_url).await?;
                Ok(Arc::new(repo))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_kind_from_url() {
        assert!(matches!(
            DatabaseKind::from_url("sqlite:///test.db"),
            Ok(DatabaseKind::Sqlite)
        ));

        let result = DatabaseKind::from_url("invalid://test");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unsupported"));

        let result = DatabaseKind::from_url("not-a-url");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid URL"));
    }

    #[cfg(feature = "postgres")]
    #[test]
    fn test_database_kind_from_url_postgres() {
        assert!(matches!(
            DatabaseKind::from_url("postgres://user:pass@localhost/db"),
            Ok(DatabaseKind::Postgres)
        ));
        assert!(matches!(
            DatabaseKind::from_url("postgresql://localhost/db"),
            Ok(DatabaseKind::Postgres)
        ));
    }

    #[cfg(feature = "mysql")]
    #[test]
    fn test_database_kind_from_url_mysql() {
        assert!(matches!(
            DatabaseKind::from_url("mysql://localhost/db"),
            Ok(DatabaseKind::MySql)
        ));
    }

    #[tokio::test]
    async fn test_create_repository_sqlite_path() {
        let repo = create_repository(":memory:").await.unwrap();
        let _ = repo;
    }

    #[tokio::test]
    async fn test_create_repository_sqlite_url() {
        let repo = create_repository("sqlite:///tmp/test_url.db")
            .await
            .unwrap();
        let _ = repo;
        let _ = std::fs::remove_file("/tmp/test_url.db");
    }
}
