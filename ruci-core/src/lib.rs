//! Ruci Core Library
//!
//! Core components for the Ruci CD system

pub mod archive;
pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod executor;
pub mod metrics;
pub mod queue;
pub mod rpc;
pub mod storage;
pub mod trigger;
pub mod vcs;

use archive::ArchiveManager;
use auth::AuthService;
use config::Config;
use db::Repository;
use metrics::Metrics;
use queue::{JobQueue, QueueRequest};
use storage::Storage;

use std::sync::Arc;

/// Ruci application context
/// Holds all shared state for the daemon
pub struct AppContext {
    pub config: Config,
    pub db: Arc<dyn Repository>,
    pub queue: Arc<JobQueue>,
    pub storage: Arc<dyn Storage>,
    pub metrics: Arc<Metrics>,
    pub archive: Arc<ArchiveManager>,
    pub auth: Arc<AuthService>,
}

impl AppContext {
    pub async fn new(config: Config) -> Result<Self> {
        tracing::debug!("Initializing AppContext");

        // Ensure directories exist
        config.ensure_dirs()?;
        tracing::debug!(
            "Directories ensured: jobs_dir={}, run_dir={}, archive_dir={}",
            config.paths.jobs_dir,
            config.paths.run_dir,
            config.paths.archive_dir
        );

        // Initialize database
        tracing::info!("Connecting to database: {}", config.database.url);
        let db = match db::create_repository(&config.database.url).await {
            Ok(repo) => {
                tracing::info!("Database connection established");
                repo
            }
            Err(e) => {
                tracing::error!("Failed to connect to database: {}", e);
                return Err(e);
            }
        };

        // Run database migrations
        tracing::info!("Running database migrations...");
        if let Err(e) = db.migrate().await {
            tracing::error!("Failed to run migrations: {}", e);
            return Err(e);
        }
        tracing::info!("Database migrations completed");

        // Initialize storage
        tracing::info!("Initializing storage backend: {:?}", config.storage);
        let storage = match storage::create_storage(&config.storage).await {
            Ok(s) => {
                tracing::info!("Storage backend initialized successfully");
                s
            }
            Err(e) => {
                tracing::error!("Failed to initialize storage: {}", e);
                return Err(e);
            }
        };

        // Initialize queue
        tracing::debug!("Initializing job queue");
        let queue = Arc::new(JobQueue::new());
        tracing::info!("Job queue initialized");

        // Recover queued jobs from database
        tracing::info!("Recovering queued jobs from database...");
        match db.list_runs_by_status("QUEUED").await {
            Ok(queued_runs) => {
                let mut recovered_count = 0;
                for run_info in queued_runs {
                    // Get params from database
                    let params = db.get_run_params(&run_info.id).await.unwrap_or_default();
                    let req = QueueRequest {
                        job_id: run_info.job_id.clone(),
                        params,
                        run_id: run_info.id.clone(),
                        build_num: run_info.build_num,
                    };
                    if let Err(e) = queue.enqueue(req).await {
                        tracing::error!("Failed to re-enqueue run {}: {}", run_info.id, e);
                    } else {
                        recovered_count += 1;
                    }
                }
                tracing::info!("Recovered {} queued jobs from database", recovered_count);
            }
            Err(e) => {
                tracing::warn!("Failed to recover queued jobs from database: {}", e);
            }
        }

        // Initialize metrics
        tracing::debug!("Initializing metrics");
        let metrics = Arc::new(Metrics::new());

        // Initialize archive manager
        tracing::debug!(
            "Initializing archive manager: archive_dir={}",
            config.paths.archive_dir
        );
        let archive = Arc::new(ArchiveManager::new(
            &config.paths.archive_dir,
            config.archive.clone(),
        ));

        // Initialize authentication service
        tracing::debug!("Initializing authentication service");
        let auth = Arc::new(AuthService::new(db.clone()));

        // Initialize admin user if configured
        if config.web.enabled {
            match auth
                .init_admin_user(&config.web.admin_username, &config.web.admin_password)
                .await
            {
                Ok(_) => {
                    tracing::info!("Admin user initialized or already exists");
                }
                Err(e) => {
                    tracing::warn!("Failed to initialize admin user: {}", e);
                }
            }
        }

        // Run startup cleanup if enabled
        if config.cleanup.enabled {
            tracing::info!("Running startup archive cleanup...");
            match archive
                .cleanup_old_archives(config.archive.max_age_days)
                .await
            {
                Ok(count) => {
                    if count > 0 {
                        tracing::info!("Cleaned up {} old archives at startup", count);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to cleanup old archives at startup: {}", e);
                }
            }
        }

        tracing::info!("AppContext initialization complete");
        Ok(Self {
            config,
            db,
            queue,
            storage: Arc::from(storage),
            metrics,
            archive,
            auth,
        })
    }
}

pub use error::{Error, Result};
