//! Rucid - Ruci CI Daemon
//!
//! Main entry point for the CI daemon

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::Json, routing::get, routing::post, Router};
use clap::{Parser, Subcommand};
use prometheus_client::encoding::text::encode;
use tokio::signal::unix::SignalKind;
use tokio::sync::{Mutex, Semaphore};
use tower_http::cors::{Any, CorsLayer};

use ruci_core::{
    config::Config,
    executor::{BashExecutor, ExecutionContext, Executor, Job},
    rpc::RpcServer,
    storage,
    vcs::{self, GitOperations},
    AppContext,
};

mod web;

/// Graceful shutdown coordinator for coordinating job termination
struct GracefulShutdown {
    /// Flag set when shutdown is requested
    should_stop: Arc<AtomicBool>,
    /// Set of run_ids currently being executed
    running_jobs: Arc<Mutex<HashSet<String>>>,
}

impl GracefulShutdown {
    fn new() -> Self {
        Self {
            should_stop: Arc::new(AtomicBool::new(false)),
            running_jobs: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Signal that shutdown should begin
    fn signal_stop(&self) {
        self.should_stop.store(true, Ordering::SeqCst);
        tracing::info!("Shutdown signal received");
    }

    /// Check if shutdown is requested
    fn should_stop(&self) -> bool {
        self.should_stop.load(Ordering::SeqCst)
    }

    /// Mark a job as running
    async fn mark_running(&self, run_id: &str) {
        let mut jobs = self.running_jobs.lock().await;
        jobs.insert(run_id.to_string());
        tracing::debug!(run_id=%run_id, "Job marked as running");
    }

    /// Mark a job as finished
    async fn mark_finished(&self, run_id: &str) {
        let mut jobs = self.running_jobs.lock().await;
        jobs.remove(run_id);
        tracing::debug!(run_id=%run_id, "Job marked as finished");
    }

    /// Get list of currently running job IDs
    async fn get_running_jobs(&self) -> Vec<String> {
        let jobs = self.running_jobs.lock().await;
        jobs.iter().cloned().collect()
    }

    /// Wait for all running jobs to complete (with timeout)
    async fn wait_for_jobs(&self, timeout_secs: u64) -> bool {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(timeout_secs);

        while start.elapsed() < timeout {
            let jobs = self.running_jobs.lock().await;
            if jobs.is_empty() {
                tracing::info!("All jobs completed, proceeding with shutdown");
                return true;
            }
            drop(jobs);

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        tracing::warn!(
            "Timeout waiting for jobs to complete, {} still running",
            self.running_jobs.lock().await.len()
        );
        false
    }
}

#[derive(Parser)]
#[command(name = "rucid")]
#[command(about = "Ruci CI Daemon")]
struct Cli {
    #[arg(short, long, help = "Config file path")]
    config: Option<String>,

    #[arg(long, help = "PID file path (for systemd)")]
    pid_file: Option<PathBuf>,

    #[arg(long, help = "RPC socket path")]
    socket_path: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Start,
    Stop,
    Restart,
}

async fn status_handler(State(_state): State<web::handlers::AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "running",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn health_handler(State(state): State<web::handlers::AppState>) -> Json<serde_json::Value> {
    let mut healthy = true;
    let mut details = serde_json::json!({});

    // Check database connection
    match state.context.db.list_jobs().await {
        Ok(_) => {
            details["database"] = serde_json::json!({
                "status": "connected"
            });
        }
        Err(e) => {
            healthy = false;
            details["database"] = serde_json::json!({
                "status": "error",
                "message": e.to_string()
            });
        }
    }

    // Check queue status
    let queue_len = state.context.queue.len();
    details["queue"] = serde_json::json!({
        "status": "running",
        "pending_jobs": queue_len
    });

    Json(serde_json::json!({
        "status": if healthy { "healthy" } else { "unhealthy" },
        "version": env!("CARGO_PKG_VERSION"),
        "checks": details
    }))
}

async fn metrics_handler(
    State(state): State<web::handlers::AppState>,
) -> Result<String, StatusCode> {
    let mut buffer = String::new();
    encode(&mut buffer, &state.context.metrics.registry)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(buffer)
}

async fn list_jobs_handler(
    State(state): State<web::handlers::AppState>,
) -> Json<serde_json::Value> {
    let jobs = state.context.db.list_jobs().await.unwrap_or_default();
    Json(serde_json::json!({ "jobs": jobs }))
}

async fn list_runs_handler(
    State(state): State<web::handlers::AppState>,
) -> Json<serde_json::Value> {
    let queued = state
        .context
        .db
        .list_runs_by_status("QUEUED")
        .await
        .unwrap_or_default();
    let running = state
        .context
        .db
        .list_runs_by_status("RUNNING")
        .await
        .unwrap_or_default();
    Json(serde_json::json!({
        "queued": queued,
        "running": running,
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse CLI
    let cli = Cli::parse();

    // Load configuration
    let config = if let Some(path) = cli.config {
        Config::load(&path)?
    } else {
        Config::load_auto()?
    };

    // Initialize logging
    init_logging(&config.logging);

    tracing::info!("Starting rucid v{}", env!("CARGO_PKG_VERSION"));

    // Write PID file if specified
    if let Some(ref pid_file) = cli.pid_file {
        if let Some(parent) = pid_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(pid_file, std::process::id().to_string())?;
    }

    // Handle commands
    match cli.command {
        Some(Commands::Stop) => {
            tracing::info!("Stop command received");
            return Ok(());
        }
        Some(Commands::Restart) => {
            tracing::info!("Restart command received");
        }
        _ => {}
    }

    // Determine socket path: CLI arg > config
    let socket_path = cli
        .socket_path
        .unwrap_or_else(|| config.server.socket_path());

    // Main server loop - supports hot reload via SIGHUP
    loop {
        let should_reload = run_server(config.clone(), socket_path.clone()).await?;

        if !should_reload {
            // Normal shutdown
            break;
        }
        // SIGHUP received - reload config and restart
        tracing::info!("Hot reload: reloading configuration...");

        // Reload config from original path
        if let Some(ref path) = config.config_path {
            match Config::load(path) {
                Ok(new_config) => {
                    tracing::info!("Configuration reloaded successfully");
                    // Update logging if changed
                    init_logging(&new_config.logging);
                    // Continue loop with new config
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to reload configuration: {}, continuing with current config",
                        e
                    );
                    break;
                }
            }
        } else {
            tracing::warn!("No config file path available for hot reload");
            break;
        }
    }

    tracing::info!("Rucid stopped");
    Ok(())
}

async fn run_server(config: Config, socket_path: PathBuf) -> anyhow::Result<bool> {
    tracing::info!("Starting rucid services...");

    // Create app context
    let context = Arc::new(AppContext::new(config.clone()).await?);

    // Create executor
    let executor: Arc<dyn Executor> = Arc::new(BashExecutor::new(Arc::new(config.clone())));

    // Create storage
    let storage = storage::create_storage(&config.storage).await?;

    // Create RPC server
    let rpc_server = RpcServer::new(
        Arc::new(config.clone()),
        socket_path.clone(),
        context.db.clone(),
        context.queue.clone(),
        executor.clone(),
        Arc::from(storage),
    );

    // Create trigger scheduler if triggers are configured
    let mut trigger_scheduler: Option<ruci_core::trigger::TriggerScheduler> = None;
    if !config.triggers.is_empty() {
        tracing::info!(
            "Initializing trigger scheduler with {} triggers",
            config.triggers.len()
        );

        match ruci_core::trigger::TriggerScheduler::new(
            Arc::new(config.triggers.clone()),
            context.db.clone(),
            context.queue.clone(),
            config.paths.jobs_dir.clone(),
        )
        .await
        {
            Ok(scheduler) => {
                trigger_scheduler = Some(scheduler);
            }
            Err(e) => {
                tracing::error!("Failed to create trigger scheduler: {}", e);
            }
        };

        if let Some(ref mut sched) = trigger_scheduler {
            match sched.start().await {
                Ok(_) => {
                    tracing::info!("Trigger scheduler started successfully");
                }
                Err(e) => {
                    tracing::error!("Failed to start trigger scheduler: {}", e);
                    trigger_scheduler = None;
                }
            }
        }
    } else {
        tracing::info!("No triggers configured, skipping trigger scheduler");
    };

    // Clone config for tasks
    let config_clone = config.clone();
    let config_for_web = config.clone();
    let context_clone = context.clone();

    // Spawn RPC server task
    let rpc_handle = tokio::spawn(async move {
        if let Err(e) = rpc_server.serve().await {
            tracing::error!("RPC server error: {}", e);
        }
    });

    // Spawn web UI task
    let web_handle = {
        let context_web = context.clone();

        tokio::spawn(async move {
            use web::handlers;

            let cors = CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any);

            let app_state = handlers::AppState {
                context: context_web,
            };

            let app = Router::new()
                // API routes
                .route("/api/status", get(status_handler))
                .route("/api/jobs", get(list_jobs_handler))
                .route("/api/runs", get(list_runs_handler))
                .route("/api/triggers", get(handlers::api_triggers_handler))
                // Webhook endpoints (GitHub, GitLab, Gogs)
                .route(
                    "/api/webhooks/:source",
                    post(web::webhooks::webhook_handler),
                )
                // Health and metrics
                .route("/health", get(health_handler))
                .route("/metrics", get(metrics_handler))
                // Web UI routes
                .route("/ui/login", get(handlers::login_page))
                .route("/ui/login", post(handlers::login_handler))
                .route("/ui/logout", post(handlers::logout_handler))
                .route("/ui/jobs", get(handlers::jobs_page))
                .route("/ui/runs", get(handlers::runs_page))
                .route("/ui/runs/:run_id", get(handlers::run_detail_page))
                .route("/ui/queue", get(handlers::queue_page))
                .route("/ui/triggers", get(handlers::triggers_page))
                .route("/ui/webhooks", get(handlers::webhooks_page))
                .route(
                    "/api/triggers/:name/enable",
                    post(handlers::trigger_enable_handler),
                )
                .route(
                    "/api/triggers/:name/disable",
                    post(handlers::trigger_disable_handler),
                )
                .route("/api/webhooks", post(handlers::webhook_create_handler))
                .route(
                    "/api/webhooks/:name/enable",
                    post(handlers::webhook_enable_handler),
                )
                .route(
                    "/api/webhooks/:name/disable",
                    post(handlers::webhook_disable_handler),
                )
                .route(
                    "/api/webhooks/:name/delete",
                    post(handlers::webhook_delete_handler),
                )
                // SSE log stream
                .route("/stream/logs/:run_id", get(handlers::log_stream_handler))
                .layer(cors)
                .with_state(app_state);

            let addr = format!(
                "{}:{}",
                config_for_web.server.web_host, config_for_web.server.web_port
            );
            tracing::info!("Web UI listening on http://{}", addr);

            let listener = tokio::net::TcpListener::bind(&addr).await?;
            axum::serve(listener, app).await
        })
    };

    // Create semaphores for each context to limit concurrent job execution
    let context_semaphores: HashMap<String, Arc<Semaphore>> = config
        .contexts
        .iter()
        .map(|(name, ctx)| {
            tracing::info!(
                context=%name,
                max_parallel=%ctx.max_parallel,
                "Creating concurrency semaphore for context"
            );
            (name.clone(), Arc::new(Semaphore::new(ctx.max_parallel)))
        })
        .collect();

    // Create graceful shutdown coordinator
    let shutdown = Arc::new(GracefulShutdown::new());

    // Spawn job queue consumer
    let consumer_handle = {
        let queue = context_clone.queue.clone();
        let db = context_clone.db.clone();
        let config_consumer = config_clone.clone();
        let executor_consumer = executor.clone();
        let semaphores = context_semaphores.clone();
        let shutdown = shutdown.clone();

        tokio::spawn(async move {
            while let Some(req) = queue.dequeue().await {
                // Check if shutdown is requested - stop accepting new jobs
                if shutdown.should_stop() {
                    tracing::info!("Shutdown in progress, re-queueing remaining jobs and stopping");
                    // Update job status back to QUEUED so it can be recovered
                    db.update_run_status(&req.run_id, "QUEUED", None).await.ok();
                    break;
                }

                tracing::info!("Processing job: {} run: {}", req.job_id, req.run_id);

                // Mark job as running for shutdown tracking
                shutdown.mark_running(&req.run_id).await;

                // Load job config first to determine which context semaphore to use
                let job_path = format!("{}/{}.yaml", config_consumer.paths.jobs_dir, req.job_id);
                let yaml_content = match std::fs::read_to_string(&job_path) {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!("Failed to read job file: {}", e);
                        db.update_run_status(&req.run_id, "FAILED", Some(-1))
                            .await
                            .ok();
                        shutdown.mark_finished(&req.run_id).await;
                        continue;
                    }
                };

                let job = match Job::parse(&yaml_content) {
                    Ok(j) => j,
                    Err(e) => {
                        tracing::error!("Failed to parse job: {}", e);
                        db.update_run_status(&req.run_id, "FAILED", Some(-1))
                            .await
                            .ok();
                        shutdown.mark_finished(&req.run_id).await;
                        continue;
                    }
                };

                // Get the semaphore for this job's context
                let semaphore = match semaphores.get(&job.context) {
                    Some(sem) => sem.clone(),
                    None => {
                        tracing::error!(
                            "Context '{}' not found for job '{}'",
                            job.context,
                            req.job_id
                        );
                        db.update_run_status(&req.run_id, "FAILED", Some(-1))
                            .await
                            .ok();
                        shutdown.mark_finished(&req.run_id).await;
                        continue;
                    }
                };

                // Acquire a permit (will wait if max_parallel jobs are already running for this context)
                let permit = semaphore.acquire().await.expect("Semaphore closed");
                tracing::debug!(
                    job_id=%req.job_id,
                    context=%job.context,
                    "Acquired context permit, starting job execution"
                );

                // Update status to running
                if let Err(e) = db.update_run_status(&req.run_id, "RUNNING", None).await {
                    tracing::error!("Failed to update run status: {}", e);
                    drop(permit);
                    shutdown.mark_finished(&req.run_id).await;
                    continue;
                }

                // Create execution context
                let ctx = ExecutionContext {
                    run_id: req.run_id.clone(),
                    job_id: req.job_id.clone(),
                    build_num: req.build_num,
                    work_dir: format!(
                        "{}/{}/{}",
                        config_consumer.paths.run_dir, req.job_id, req.run_id
                    )
                    .into(),
                    env: job.env.clone(),
                    params: req.params.clone(),
                };

                // Perform VCS checkout if job has VCS configured and checkout is enabled
                if job.checkout {
                    if let Some(ref vcs_info) = job.vcs {
                        let git_ops =
                            GitOperations::new(vcs_info.credential_id.as_ref().map(|_| {
                                // TODO: Fetch SSH key path from credential storage
                                "/tmp/ssh_key".to_string()
                            }));
                        match vcs::checkout(vcs_info, &ctx.work_dir, &ctx.params, &git_ops).await {
                            Ok(_) => {
                                tracing::info!(job_id = %req.job_id, "VCS checkout completed");
                            }
                            Err(e) => {
                                tracing::error!(job_id = %req.job_id, error = %e, "VCS checkout failed");
                                db.update_run_status(&req.run_id, "FAILED", Some(-1))
                                    .await
                                    .ok();
                                shutdown.mark_finished(&req.run_id).await;
                                continue;
                            }
                        }
                    } else if let (Some(vcs_url), Some(vcs_branch)) =
                        (req.params.get("vcs_url"), req.params.get("vcs_branch"))
                    {
                        // VCS params passed from webhook but no job-level VCS config
                        // Create a minimal VcsInfo from webhook params
                        let vcs_info = vcs::VcsInfo {
                            url: vcs_url.clone(),
                            repository: String::new(),
                            branch: vcs_branch.clone(),
                            commit: req.params.get("vcs_commit").cloned(),
                            submodules: false,
                            credential_id: None,
                        };
                        let git_ops = GitOperations::new(None);
                        match vcs::checkout(&vcs_info, &ctx.work_dir, &ctx.params, &git_ops).await {
                            Ok(_) => {
                                tracing::info!(job_id = %req.job_id, "VCS checkout from webhook params completed");
                            }
                            Err(e) => {
                                tracing::error!(job_id = %req.job_id, error = %e, "VCS checkout from webhook params failed");
                                db.update_run_status(&req.run_id, "FAILED", Some(-1))
                                    .await
                                    .ok();
                                shutdown.mark_finished(&req.run_id).await;
                                continue;
                            }
                        }
                    }
                }

                // Execute
                let result = executor_consumer.execute(&ctx, &job).await;

                // Release permit (job is done)
                drop(permit);

                // Mark job as finished
                shutdown.mark_finished(&req.run_id).await;

                // Update final status
                let (status, exit_code) = match result {
                    Ok(r) => {
                        tracing::info!("Job completed with exit code: {}", r.exit_code);
                        if r.exit_code == 0 {
                            ("SUCCESS", Some(0))
                        } else {
                            ("FAILED", Some(r.exit_code))
                        }
                    }
                    Err(e) => {
                        tracing::error!("Job execution failed: {}", e);
                        ("FAILED", Some(-1))
                    }
                };

                db.update_run_status(&req.run_id, status, exit_code)
                    .await
                    .ok();
            }
        })
    };

    // Wait for shutdown signal (SIGTERM, SIGINT) or SIGHUP for config reload
    let reason = wait_shutdown_signal().await;

    match reason {
        ShutdownReason::Reload => {
            tracing::info!(
                "Received SIGHUP - config reload requested, initiating graceful reload..."
            );
            // Abort current tasks and return to main to restart with new config
        }
        ShutdownReason::Terminate => {
            tracing::info!("Received shutdown signal, stopping...");
        }
    }

    // Graceful shutdown sequence
    // Step 1: Signal shutdown to stop accepting new jobs
    shutdown.signal_stop();

    // Get db reference for shutdown
    let db = context_clone.db.clone();

    // Step 2: Wait for running jobs to complete (with 30 second timeout)
    let jobs_completed = shutdown.wait_for_jobs(30).await;

    if !jobs_completed {
        // Step 3: Timeout - force abort running jobs
        tracing::warn!("Timeout waiting for jobs, forcing abort of remaining jobs");
        let running_jobs = shutdown.get_running_jobs().await;
        for run_id in &running_jobs {
            tracing::info!(run_id=%run_id, "Aborting job due to shutdown timeout");
            executor.abort(run_id).await.ok();
            // Update DB status to ABORTED
            db.update_run_status(run_id, "ABORTED", Some(-1)).await.ok();
        }
    } else {
        // Step 3b: All jobs completed normally, but some may still be RUNNING in DB
        // Update any remaining RUNNING jobs to ABORTED (they were interrupted)
        let running_jobs = shutdown.get_running_jobs().await;
        for run_id in running_jobs {
            tracing::info!(run_id=%run_id, "Marking interrupted job as ABORTED");
            executor.abort(&run_id).await.ok();
            db.update_run_status(&run_id, "ABORTED", Some(-1))
                .await
                .ok();
        }
    }

    // Step 4: Abort remaining tasks
    rpc_handle.abort();
    web_handle.abort();
    consumer_handle.abort();

    // Shutdown trigger scheduler
    if let Some(ref mut scheduler) = trigger_scheduler {
        if let Err(e) = scheduler.shutdown().await {
            tracing::error!("Error shutting down trigger scheduler: {}", e);
        } else {
            tracing::info!("Trigger scheduler shut down successfully");
        }
    }

    tracing::info!("Rucid stopped");
    Ok(reason == ShutdownReason::Reload)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShutdownReason {
    Terminate,
    Reload,
}

async fn wait_shutdown_signal() -> ShutdownReason {
    let mut sigterm = tokio::signal::unix::signal(SignalKind::terminate()).unwrap();
    let mut sigint = tokio::signal::unix::signal(SignalKind::interrupt()).unwrap();
    let mut sighup = tokio::signal::unix::signal(SignalKind::hangup()).unwrap();

    tokio::select! {
        _ = sigterm.recv() => {
            tracing::info!("Received SIGTERM");
            ShutdownReason::Terminate
        }
        _ = sigint.recv() => {
            tracing::info!("Received SIGINT (Ctrl+C)");
            ShutdownReason::Terminate
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Received Ctrl+C");
            ShutdownReason::Terminate
        }
        _ = sighup.recv() => {
            tracing::info!("Received SIGHUP - config reload requested");
            ShutdownReason::Reload
        }
    }
}

fn init_logging(config: &ruci_core::config::LoggingConfig) {
    use tracing_subscriber::{fmt, layer::SubscriberExt, prelude::*, EnvFilter};

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.level));

    if let Some(ref file_config) = config.file {
        // File logging with daily rotation
        let file_appender = tracing_appender::rolling::daily(&file_config.dir, "rucid");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        // Leak the guard to keep it alive for the entire program duration
        Box::leak(Box::new(guard));

        match config.format.as_str() {
            "json" => {
                tracing_subscriber::registry()
                    .with(filter)
                    .with(fmt::layer().json().with_writer(non_blocking))
                    .init();
            }
            _ => {
                tracing_subscriber::registry()
                    .with(filter)
                    .with(fmt::layer().pretty().with_writer(non_blocking))
                    .init();
            }
        }
    } else {
        // Console only logging
        match config.format.as_str() {
            "json" => {
                tracing_subscriber::registry()
                    .with(filter)
                    .with(fmt::layer().json())
                    .init();
            }
            _ => {
                tracing_subscriber::registry()
                    .with(filter)
                    .with(fmt::layer().pretty())
                    .init();
            }
        }
    }
}
