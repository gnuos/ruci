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
    ArtifactRepository, JobRepository, Repository, RunRepository, TriggerRepository,
    UserRepository, VcsCredentialInfo, VcsCredentialRepository, WebhookEvent, WebhookFilter,
    WebhookRepository, WebhookSource, WebhookTriggerInfo,
};
pub use sqlite::SqliteRepository;

use crate::error::Result;
use std::sync::Arc;

/// Create a repository based on the database URL scheme
///
/// - `sqlite://path/to/db.sqlite` -> SqliteRepository
/// - `postgres://user:pass@host/db` -> PostgreSQLRepository
/// - `mysql://user:pass@host/db` -> MySQLRepository
pub async fn create_repository(database_url: &str) -> Result<Arc<dyn Repository>> {
    if database_url.starts_with("sqlite://") {
        let repo = SqliteRepository::new(database_url).await?;
        Ok(Arc::new(repo))
    } else if database_url.starts_with("postgres://") || database_url.starts_with("postgresql://") {
        let repo = PostgresRepository::new(database_url).await?;
        Ok(Arc::new(repo))
    } else if database_url.starts_with("mysql://") {
        let repo = MysqlRepository::new(database_url).await?;
        Ok(Arc::new(repo))
    } else {
        Err(crate::error::Error::Database(
            crate::error::DbError::Connection(format!(
                "Unsupported database scheme: {}",
                database_url
            )),
        ))
    }
}
