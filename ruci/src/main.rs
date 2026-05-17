//! Ruci CLI - Command Line Interface
//!
//! Client for the Ruci CD system

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

use ruci_protocol::RunStatus;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CliConfig {
    server: Option<String>,
    token: Option<String>,
}

impl CliConfig {
    fn load(config_path: Option<&str>) -> Self {
        let path = config_path
            .map(PathBuf::from)
            .or_else(|| std::env::var("RUCI_CONFIG").ok().map(PathBuf::from))
            .or_else(|| dirs::config_dir().map(|p| p.join("ruci").join("config.yaml")))
            .or_else(|| dirs::home_dir().map(|p| p.join(".ruci.yaml")));

        if let Some(p) = path {
            if p.exists() {
                if let Ok(content) = std::fs::read_to_string(&p) {
                    if let Ok(config) = yaml_serde::from_str::<CliConfig>(&content) {
                        return config;
                    }
                }
            }
        }
        Self::default()
    }

    fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("ruci")
            .join("config.yaml")
    }

    fn save(&self) -> anyhow::Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let yaml = yaml_serde::to_string(self)
            .map_err(|e| anyhow::anyhow!("Failed to serialize config: {}", e))?;
        std::fs::write(&path, yaml)?;
        Ok(())
    }
}

mod dirs {
    use std::path::PathBuf;

    pub fn config_dir() -> Option<PathBuf> {
        std::env::var("XDG_CONFIG_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|p| p.join(".config")))
    }

    pub fn home_dir() -> Option<PathBuf> {
        std::env::var("HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| std::env::var("USERPROFILE").ok().map(PathBuf::from))
    }
}

#[derive(Parser)]
#[command(name = "ruci")]
#[command(about = "Ruci Client", long_about = None)]
struct Cli {
    #[arg(short, long, help = "RPC server address")]
    server: Option<String>,

    #[arg(short, long, help = "API token for authentication")]
    token: Option<String>,

    #[arg(short, long, help = "Config file path")]
    config: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Job management
    Job {
        #[command(subcommand)]
        action: JobAction,
    },

    /// Run management
    Run {
        #[command(subcommand)]
        action: RunAction,
    },

    /// Trigger management
    Trigger {
        #[command(subcommand)]
        action: TriggerAction,
    },

    /// Submit a job (register + queue immediately)
    Submit {
        #[arg(short, long, help = "Job file path")]
        file: Option<String>,

        #[arg(short, long, help = "Wait for completion")]
        wait: bool,

        #[arg(short = 'F', long, help = "Follow log output")]
        follow: bool,

        #[arg(long, help = "YAML content (for CI environment)")]
        yaml: Option<String>,
    },

    /// Show daemon status
    Status,

    /// Validate a job configuration file
    Validate {
        #[arg(help = "Job file path")]
        file: Option<String>,
    },

    /// API token management
    Token {
        #[command(subcommand)]
        action: TokenAction,
    },

    /// CLI config management
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Generate shell completions
    Completions(CompletionsCmd),
}

#[derive(Subcommand)]
enum JobAction {
    /// List all jobs
    List,

    /// Get job details
    Get {
        #[arg(help = "Job ID")]
        job_id: String,
    },

    /// Register a job from YAML file (without queuing)
    Create {
        #[arg(help = "Job file path")]
        file: String,
    },

    /// Update a job's name
    Update {
        #[arg(help = "Job ID")]
        job_id: String,

        #[arg(help = "New job name")]
        name: String,
    },

    /// Delete a job
    Delete {
        #[arg(help = "Job ID")]
        job_id: String,
    },

    /// Queue a job for execution
    Queue {
        #[arg(help = "Job ID")]
        job_id: String,

        #[arg(long, help = "Parameters as KEY=VALUE")]
        params: Vec<String>,
    },

    /// List runs for a job
    Runs {
        #[arg(help = "Job ID")]
        job_id: String,
    },
}

#[derive(Subcommand)]
enum RunAction {
    /// List runs
    List {
        #[arg(
            long,
            help = "Filter by status (queued/running/success/failed/aborted)"
        )]
        status: Option<String>,
    },

    /// Get run details
    Get {
        #[arg(help = "Run ID")]
        run_id: String,
    },

    /// Show run log
    Log {
        #[arg(help = "Run ID")]
        run_id: String,
    },

    /// Watch a run until completion
    Watch {
        #[arg(help = "Run ID")]
        run_id: String,
    },

    /// Abort a running job
    Abort {
        #[arg(help = "Run ID")]
        run_id: String,
    },

    /// Manage artifacts
    Artifact {
        #[command(subcommand)]
        action: ArtifactAction,
    },
}

#[derive(Subcommand)]
enum ArtifactAction {
    Upload {
        #[arg(help = "Run ID")]
        run_id: String,

        #[arg(help = "Local file path")]
        path: String,
    },
    List {
        #[arg(help = "Run ID")]
        run_id: String,
    },
    Download {
        #[arg(help = "Artifact ID")]
        artifact_id: String,
    },
}

#[derive(Subcommand)]
enum TriggerAction {
    /// List all triggers
    List,

    /// Create a new trigger
    Create {
        #[arg(help = "Trigger name")]
        name: String,

        #[arg(help = "Cron expression (6-field: second minute hour day month weekday)")]
        cron: String,

        #[arg(help = "Job ID to trigger")]
        job_id: String,
    },

    /// Delete a trigger
    Delete {
        #[arg(help = "Trigger name")]
        name: String,
    },

    /// Enable a trigger
    Enable {
        #[arg(help = "Trigger name")]
        name: String,
    },

    /// Disable a trigger
    Disable {
        #[arg(help = "Trigger name")]
        name: String,
    },
}

#[derive(Subcommand)]
enum TokenAction {
    /// Generate a new API token
    Generate {
        #[arg(short, long, help = "Token name/description")]
        name: String,

        #[arg(
            short,
            long,
            help = "Permissions (comma-separated: read,write)",
            default_value = "read,write"
        )]
        permissions: String,

        #[arg(short, long, help = "Expiration (e.g. 30d, 1y)")]
        expires: Option<String>,
    },

    /// List all API tokens
    List,

    /// Revoke an API token
    Revoke {
        #[arg(help = "Token ID")]
        token_id: i64,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Initialize CLI config interactively
    Init,

    /// Show current config
    Show,

    /// Set a config value
    Set {
        #[arg(help = "Config key (server, token)")]
        key: String,

        #[arg(help = "Config value")]
        value: String,
    },
}

#[derive(Args)]
struct CompletionsCmd {
    #[arg(value_enum, default_value = "bash")]
    shell: Shell,
}

#[derive(ValueEnum, Clone)]
#[allow(clippy::enum_variant_names)]
enum Shell {
    Bash,
    Zsh,
    Fish,
    PowerShell,
    Elvish,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let cli_config = CliConfig::load(cli.config.as_deref());

    let server_override = cli.server.clone();
    let token_override = cli.token.clone();

    match cli.command {
        Commands::Job { action } => {
            handle_job(&cli_config, server_override, token_override, action).await?
        }
        Commands::Run { action } => {
            handle_run(&cli_config, server_override, token_override, action).await?
        }
        Commands::Trigger { action } => {
            handle_trigger(&cli_config, server_override, token_override, action).await?
        }
        Commands::Submit {
            file,
            wait,
            follow,
            yaml,
        } => {
            handle_submit(
                &cli_config,
                server_override,
                token_override,
                file,
                wait,
                follow,
                yaml,
            )
            .await?
        }
        Commands::Status => handle_status(&cli_config, server_override, token_override).await?,
        Commands::Validate { file } => handle_validate(file),
        Commands::Token { action } => {
            handle_token(&cli_config, server_override, token_override, action).await?
        }
        Commands::Config { action } => handle_config(action, &cli_config),
        Commands::Completions(cmd) => handle_completions(cmd),
    }

    Ok(())
}

fn resolve_server(cli_config: &CliConfig, override_val: Option<String>) -> String {
    override_val
        .or_else(|| cli_config.server.clone())
        .or_else(|| std::env::var("RUCI_SERVER").ok())
        .unwrap_or_else(|| "127.0.0.1:7741".to_string())
}

fn resolve_token(cli_config: &CliConfig, override_val: Option<String>) -> Option<String> {
    override_val
        .or_else(|| cli_config.token.clone())
        .or_else(|| std::env::var("RUCI_TOKEN").ok())
}

// ─────────────────────────────────────────────────────────────────
// Job handlers
// ─────────────────────────────────────────────────────────────────

async fn handle_job(
    cli_config: &CliConfig,
    server: Option<String>,
    token: Option<String>,
    action: JobAction,
) -> anyhow::Result<()> {
    let addr = resolve_server(cli_config, server);
    let tok = resolve_token(cli_config, token);
    let client = connect_and_auth(&addr, tok.as_deref()).await?;

    match action {
        JobAction::List => {
            let jobs = client.list_jobs(tarpc::context::current()).await?;
            if jobs.is_empty() {
                println!("No jobs registered.");
                return Ok(());
            }
            println!("{:<18} {:<20} SOURCE", "ID", "NAME");
            println!("{}", "-".repeat(60));
            for job in jobs {
                println!("{:<18} {:<20} {}", job.id, job.name, job.original_name);
            }
        }
        JobAction::Get { job_id } => {
            if let Some(job) = client.get_job(tarpc::context::current(), job_id).await? {
                println!("Job: {}", job.name);
                println!("  ID:         {}", job.id);
                println!("  Source:     {}", job.original_name);
                println!("  Submitted:  {}", job.submitted_at);
            } else {
                eprintln!("Job not found");
                std::process::exit(1);
            }
        }
        JobAction::Create { file } => {
            let yaml_content = std::fs::read_to_string(&file)?;
            let job = ruci_core::executor::Job::parse(&yaml_content)?;

            let resp = client
                .register_job(tarpc::context::current(), yaml_content)
                .await?;
            if let Some(err_msg) = resp.error_message {
                eprintln!("Error: {}", err_msg);
                std::process::exit(1);
            }
            println!("Created job: {} ({})", job.name, resp.job_id);
        }
        JobAction::Update { job_id, name } => {
            let success = client
                .update_job(tarpc::context::current(), job_id.clone(), name.clone())
                .await?;
            if success {
                println!("Updated job {}: name = \"{}\"", job_id, name);
            } else {
                eprintln!("Failed to update job {}", job_id);
                std::process::exit(1);
            }
        }
        JobAction::Delete { job_id } => {
            let success = client
                .delete_job(tarpc::context::current(), job_id.clone())
                .await?;
            if success {
                println!("Deleted job: {}", job_id);
            } else {
                eprintln!("Failed to delete job {}", job_id);
                std::process::exit(1);
            }
        }
        JobAction::Queue { job_id, params } => {
            let params: HashMap<_, _> = params
                .iter()
                .filter_map(|p| {
                    let mut parts = p.splitn(2, '=');
                    match (parts.next(), parts.next()) {
                        (Some(k), Some(v)) => Some((k.to_string(), v.to_string())),
                        _ => None,
                    }
                })
                .collect();

            let resp = client
                .queue_job(tarpc::context::current(), job_id, params)
                .await?;
            if let Some(err_msg) = resp.error_message {
                eprintln!("Error: {}", err_msg);
                std::process::exit(1);
            }
            println!(
                "Queued: run_id={} build_num={} status={}",
                resp.run_id, resp.build_num, resp.status
            );
        }
        JobAction::Runs { job_id } => {
            let runs = client
                .list_runs_by_job(tarpc::context::current(), job_id.clone())
                .await?;
            if runs.is_empty() {
                println!("No runs found for job {}", job_id);
                return Ok(());
            }
            print_run_table(&runs);
        }
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────
// Run handlers
// ─────────────────────────────────────────────────────────────────

async fn handle_run(
    cli_config: &CliConfig,
    server: Option<String>,
    token: Option<String>,
    action: RunAction,
) -> anyhow::Result<()> {
    let addr = resolve_server(cli_config, server);
    let tok = resolve_token(cli_config, token);
    let client = connect_and_auth(&addr, tok.as_deref()).await?;

    match action {
        RunAction::List { status } => {
            let runs = if let Some(ref s) = status {
                let status_upper = s.to_uppercase();
                client
                    .list_runs_by_status(tarpc::context::current(), status_upper)
                    .await?
            } else {
                client.list_runs(tarpc::context::current()).await?
            };

            if runs.is_empty() {
                println!("No runs found.");
                return Ok(());
            }
            print_run_table(&runs);
        }
        RunAction::Get { run_id } => {
            if let Some(run) = client.get_run(tarpc::context::current(), run_id).await? {
                println!("Run: {}", run.id);
                println!("  Job:        {} ({})", run.job_name, run.job_id);
                println!("  Build:      #{}", run.build_num);
                println!("  Status:     {}", run.status);
                if let Some(started) = run.started_at {
                    println!("  Started:    {}", started);
                }
                if let Some(finished) = run.finished_at {
                    println!("  Finished:   {}", finished);
                }
                if let Some(code) = run.exit_code {
                    println!("  Exit Code:  {}", code);
                }
            } else {
                eprintln!("Run not found");
                std::process::exit(1);
            }
        }
        RunAction::Log { run_id } => {
            let log = client
                .get_run_log(tarpc::context::current(), run_id)
                .await?;
            print!("{}", log);
        }
        RunAction::Watch { run_id } => loop {
            if let Some(run) = client
                .get_run(tarpc::context::current(), run_id.clone())
                .await?
            {
                println!("Status: {}", run.status);
                if matches!(
                    run.status,
                    RunStatus::Success | RunStatus::Failed | RunStatus::Aborted
                ) {
                    break;
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        },
        RunAction::Abort { run_id } => {
            client
                .abort_job(tarpc::context::current(), run_id.clone())
                .await?;
            println!("Aborted run: {}", run_id);
        }
        RunAction::Artifact { action } => match action {
            ArtifactAction::Upload { run_id, path } => {
                let artifact = client
                    .upload_artifact(tarpc::context::current(), run_id, path)
                    .await?;
                println!("Uploaded: {}", artifact.name);
            }
            ArtifactAction::List { run_id } => {
                let artifacts = client
                    .list_artifacts(tarpc::context::current(), run_id)
                    .await?;
                if artifacts.is_empty() {
                    println!("No artifacts found.");
                    return Ok(());
                }
                for a in artifacts {
                    println!("  {} ({} bytes)", a.name, a.size);
                }
            }
            ArtifactAction::Download { artifact_id } => {
                let data = client
                    .download_artifact(tarpc::context::current(), artifact_id)
                    .await?;
                std::io::stdout().write_all(&data)?;
            }
        },
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────
// Trigger handlers
// ─────────────────────────────────────────────────────────────────

async fn handle_trigger(
    cli_config: &CliConfig,
    server: Option<String>,
    token: Option<String>,
    action: TriggerAction,
) -> anyhow::Result<()> {
    let addr = resolve_server(cli_config, server);
    let tok = resolve_token(cli_config, token);
    let client = connect_and_auth(&addr, tok.as_deref()).await?;

    match action {
        TriggerAction::List => {
            let triggers = client.list_triggers(tarpc::context::current()).await?;
            if triggers.is_empty() {
                println!("No triggers configured.");
                return Ok(());
            }
            println!("{:<20} {:<20} {:<20} STATUS", "NAME", "CRON", "JOB ID");
            println!("{}", "-".repeat(80));
            for t in triggers {
                let status = if t.enabled { "Enabled" } else { "Disabled" };
                println!("{:<20} {:<20} {:<20} {}", t.name, t.cron, t.job_id, status);
            }
        }
        TriggerAction::Create { name, cron, job_id } => {
            let success = client
                .create_trigger(
                    tarpc::context::current(),
                    name.clone(),
                    cron.clone(),
                    job_id.clone(),
                )
                .await?;
            if success {
                println!("Created trigger: {} ({} -> {})", name, cron, job_id);
            } else {
                eprintln!("Failed to create trigger: {}", name);
                std::process::exit(1);
            }
        }
        TriggerAction::Delete { name } => {
            let success = client
                .delete_trigger(tarpc::context::current(), name.clone())
                .await?;
            if success {
                println!("Deleted trigger: {}", name);
            } else {
                eprintln!("Failed to delete trigger: {}", name);
                std::process::exit(1);
            }
        }
        TriggerAction::Enable { name } => {
            let success = client
                .enable_trigger(tarpc::context::current(), name.clone())
                .await?;
            if success {
                println!("Enabled trigger: {}", name);
            } else {
                eprintln!("Failed to enable trigger: {}", name);
                std::process::exit(1);
            }
        }
        TriggerAction::Disable { name } => {
            let success = client
                .disable_trigger(tarpc::context::current(), name.clone())
                .await?;
            if success {
                println!("Disabled trigger: {}", name);
            } else {
                eprintln!("Failed to disable trigger: {}", name);
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────
// Submit handler
// ─────────────────────────────────────────────────────────────────

async fn handle_submit(
    cli_config: &CliConfig,
    server: Option<String>,
    token: Option<String>,
    file: Option<String>,
    wait: bool,
    follow: bool,
    yaml: Option<String>,
) -> anyhow::Result<()> {
    let yaml_content = if let Some(y) = yaml {
        y
    } else if let Some(path) = file {
        std::fs::read_to_string(&path)?
    } else {
        ["ruci.yml", ".ruci.yml", "ruci.yaml", ".ruci.yaml"]
            .iter()
            .find_map(|name| std::fs::read_to_string(name).ok())
            .unwrap_or_else(|| {
                eprintln!("Error: No ruci job file found");
                std::process::exit(1);
            })
    };

    let addr = resolve_server(cli_config, server);
    let tok = resolve_token(cli_config, token);
    let client = connect_and_auth(&addr, tok.as_deref()).await?;
    let resp = client
        .submit_job(tarpc::context::current(), yaml_content)
        .await?;

    if let Some(err_msg) = resp.error_message {
        eprintln!("Error: {}", err_msg);
        std::process::exit(1);
    }

    println!(
        "Submitted: job_id={} run_id={} build_num={}",
        resp.job_id, resp.run_id, resp.build_num
    );

    if wait || follow {
        loop {
            if let Some(run) = client
                .get_run(tarpc::context::current(), resp.run_id.clone())
                .await?
            {
                match run.status {
                    RunStatus::Success => {
                        println!("Build succeeded");
                        break;
                    }
                    RunStatus::Failed => {
                        println!("Build failed");
                        break;
                    }
                    RunStatus::Aborted => {
                        println!("Build aborted");
                        break;
                    }
                    _ => {
                        print!(".");
                        std::io::stdout().flush()?;
                    }
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }

    if follow {
        let log = client
            .get_run_log(tarpc::context::current(), resp.run_id)
            .await?;
        println!("\nLog:\n{}", log);
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────
// Validate handler
// ─────────────────────────────────────────────────────────────────

fn handle_validate(file: Option<String>) {
    let yaml_content = match file {
        Some(path) => match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) => {
                eprintln!("Error: Failed to read file '{}': {}", path, e);
                std::process::exit(1);
            }
        },
        None => {
            // Try standard filenames
            match ["ruci.yml", ".ruci.yml", "ruci.yaml", ".ruci.yaml"]
                .iter()
                .find_map(|name| std::fs::read_to_string(name).ok())
            {
                Some(content) => content,
                None => {
                    eprintln!("Error: No ruci job file found in current directory");
                    std::process::exit(1);
                }
            }
        }
    };

    // Parse and validate the job
    match ruci_core::executor::Job::parse(&yaml_content) {
        Ok(job) => {
            println!("Job '{}' is valid", job.name);
            println!("  Context: {}", job.context);
            println!("  Timeout: {}s", job.timeout);
            println!("  Steps: {}", job.steps.len());
            for (i, step) in job.steps.iter().enumerate() {
                println!(
                    "    {}. {} - {}",
                    i + 1,
                    step.name,
                    step.command.chars().take(50).collect::<String>()
                );
            }
            if !job.env.is_empty() {
                println!("  Environment variables: {} defined", job.env.len());
            }
        }
        Err(e) => {
            eprintln!("Job configuration is invalid:");
            eprintln!("  {}", e);
            std::process::exit(1);
        }
    }
}

// ─────────────────────────────────────────────────────────────────
// Status handler
// ─────────────────────────────────────────────────────────────────

async fn handle_status(
    cli_config: &CliConfig,
    server: Option<String>,
    token: Option<String>,
) -> anyhow::Result<()> {
    let addr = resolve_server(cli_config, server);
    let tok = resolve_token(cli_config, token);
    let client = connect_and_auth(&addr, tok.as_deref()).await?;
    let status = client.status(tarpc::context::current()).await?;
    println!("Ruci Daemon Status");
    println!("  Version: {}", status.version);
    println!("  Uptime: {} seconds", status.uptime_seconds);
    println!("  Jobs: {} total", status.jobs_total);
    println!("  Queued: {}", status.jobs_queued);
    println!("  Running: {}", status.jobs_running);
    Ok(())
}

// ─────────────────────────────────────────────────────────────────
// Token handler
// ─────────────────────────────────────────────────────────────────

async fn handle_token(
    cli_config: &CliConfig,
    server: Option<String>,
    token: Option<String>,
    action: TokenAction,
) -> anyhow::Result<()> {
    let addr = resolve_server(cli_config, server);
    let tok = resolve_token(cli_config, token);
    let client = connect_and_auth(&addr, tok.as_deref()).await?;

    match action {
        TokenAction::Generate {
            name,
            permissions,
            expires: _,
        } => {
            let resp = client
                .generate_token(tarpc::context::current(), name.clone(), permissions.clone())
                .await?;
            if resp.ok {
                println!("Token created: {}", resp.token);
                println!("  ID:          {}", resp.token_id);
                println!("  Name:        {}", name);
                println!("  Permissions: {}", permissions);
                println!();
                println!("Please save this token now. It will not be shown again.");
            } else {
                eprintln!(
                    "Error: {}",
                    resp.error_message
                        .unwrap_or_else(|| "Unknown error".to_string())
                );
                std::process::exit(1);
            }
        }
        TokenAction::List => {
            let tokens = client.list_tokens(tarpc::context::current()).await?;
            if tokens.is_empty() {
                println!("No API tokens configured.");
                return Ok(());
            }
            println!(
                "{:<6} {:<20} {:<28} {:<15} CREATED",
                "ID", "NAME", "TOKEN", "PERMISSIONS"
            );
            println!("{}", "-".repeat(90));
            for t in tokens {
                let token_display = if t.token_hash.len() > 12 {
                    format!(
                        "{}...{}",
                        &t.token_hash[..8],
                        &t.token_hash[t.token_hash.len() - 4..]
                    )
                } else {
                    t.token_hash.clone()
                };
                println!(
                    "{:<6} {:<20} {:<28} {:<15} {}",
                    t.id, t.name, token_display, t.permissions, t.created_at
                );
            }
        }
        TokenAction::Revoke { token_id } => {
            let success = client
                .revoke_token(tarpc::context::current(), token_id)
                .await?;
            if success {
                println!("Token ID {} revoked", token_id);
            } else {
                eprintln!("Failed to revoke token ID {}", token_id);
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────
// Config handler
// ─────────────────────────────────────────────────────────────────

fn handle_config(action: ConfigAction, cli_config: &CliConfig) {
    match action {
        ConfigAction::Init => {
            println!("Ruci CLI Configuration");
            println!();

            let default_server = cli_config
                .server
                .clone()
                .unwrap_or_else(|| "127.0.0.1:7741".to_string());
            println!("Server address [{}]: ", default_server);
            let mut input = String::new();
            std::io::stdin().read_line(&mut input).unwrap();
            let server = input.trim();
            let server = if server.is_empty() {
                default_server
            } else {
                server.to_string()
            };

            println!("API token: ");
            let mut token_input = String::new();
            std::io::stdin().read_line(&mut token_input).unwrap();
            let token = token_input.trim().to_string();

            let config = CliConfig {
                server: Some(server),
                token: if token.is_empty() { None } else { Some(token) },
            };

            match config.save() {
                Ok(()) => {
                    println!();
                    println!("Config saved to {}", CliConfig::config_path().display());
                }
                Err(e) => {
                    eprintln!("Error saving config: {}", e);
                    std::process::exit(1);
                }
            }
        }
        ConfigAction::Show => {
            let path = CliConfig::config_path();
            println!("Config file: {}", path.display());
            println!(
                "  Server: {}",
                cli_config
                    .server
                    .as_deref()
                    .unwrap_or("(not set, default: 127.0.0.1:7741)")
            );
            let token_display = cli_config
                .token
                .as_ref()
                .map(|t| {
                    if t.len() > 12 {
                        format!("{}...{}", &t[..8], &t[t.len() - 4..])
                    } else if t.is_empty() {
                        "(not set)".to_string()
                    } else {
                        t.clone()
                    }
                })
                .unwrap_or_else(|| "(not set)".to_string());
            println!("  Token:  {}", token_display);
        }
        ConfigAction::Set { key, value } => {
            let mut config = cli_config.clone();
            match key.as_str() {
                "server" => config.server = Some(value.clone()),
                "token" => config.token = Some(value.clone()),
                _ => {
                    eprintln!("Unknown config key: {} (valid: server, token)", key);
                    std::process::exit(1);
                }
            }
            match config.save() {
                Ok(()) => println!("Set {} = {}", key, value),
                Err(e) => {
                    eprintln!("Error saving config: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────
// Completions handler
// ─────────────────────────────────────────────────────────────────

fn handle_completions(cmd: CompletionsCmd) {
    let mut app = <Cli as clap::CommandFactory>::command();
    match cmd.shell {
        Shell::Bash => clap_complete::generate(
            clap_complete::Shell::Bash,
            &mut app,
            "ruci",
            &mut std::io::stdout(),
        ),
        Shell::Zsh => clap_complete::generate(
            clap_complete::Shell::Zsh,
            &mut app,
            "ruci",
            &mut std::io::stdout(),
        ),
        Shell::Fish => clap_complete::generate(
            clap_complete::Shell::Fish,
            &mut app,
            "ruci",
            &mut std::io::stdout(),
        ),
        Shell::PowerShell => clap_complete::generate(
            clap_complete::Shell::PowerShell,
            &mut app,
            "ruci",
            &mut std::io::stdout(),
        ),
        Shell::Elvish => clap_complete::generate(
            clap_complete::Shell::Elvish,
            &mut app,
            "ruci",
            &mut std::io::stdout(),
        ),
    }
}

// ─────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────

fn print_run_table(runs: &[ruci_protocol::RunInfo]) {
    println!(
        "{:<18} {:<18} {:<6} {:<10} STARTED",
        "RUN ID", "JOB", "BUILD", "STATUS"
    );
    println!("{}", "-".repeat(80));
    for run in runs {
        let started = run
            .started_at
            .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "-".to_string());
        println!(
            "{:<18} {:<18} #{:<5} {:<10} {}",
            run.id, run.job_name, run.build_num, run.status, started
        );
    }
}

async fn connect_raw(addr: &str) -> anyhow::Result<ruci_protocol::RuciRpcClient> {
    if let Some(path) = addr.strip_prefix("unix://") {
        let transport = tarpc::serde_transport::unix::connect(path, || {
            tarpc::tokio_serde::formats::Json::<_, _>::default()
        })
        .await?;
        let client =
            ruci_protocol::RuciRpcClient::new(tarpc::client::Config::default(), transport).spawn();
        Ok(client)
    } else {
        let transport = tarpc::serde_transport::tcp::connect(addr, || {
            tarpc::tokio_serde::formats::Json::<_, _>::default()
        })
        .await?;
        let client =
            ruci_protocol::RuciRpcClient::new(tarpc::client::Config::default(), transport).spawn();
        Ok(client)
    }
}

async fn connect_and_auth(
    addr: &str,
    token: Option<&str>,
) -> anyhow::Result<ruci_protocol::RuciRpcClient> {
    let client = connect_raw(addr).await?;

    if let Some(tok) = token {
        let resp = client
            .authenticate(tarpc::context::current(), tok.to_string())
            .await?;
        if !resp.ok {
            let msg = resp
                .error_message
                .unwrap_or_else(|| "Unknown error".to_string());
            eprintln!("Authentication failed: {}", msg);
            std::process::exit(1);
        }
    } else {
        // Try connecting without token (server may not require auth)
        // A quick status call will reveal if auth is needed
        let resp = client
            .authenticate(tarpc::context::current(), String::new())
            .await;
        match resp {
            Ok(r) if r.ok => {
                // Server doesn't require auth
            }
            Ok(r) => {
                let msg = r
                    .error_message
                    .unwrap_or_else(|| "Unknown error".to_string());
                eprintln!("Authentication required: {}", msg);
                eprintln!(
                    "Use --token flag, RUCI_TOKEN env var, or 'ruci config set token <value>'"
                );
                std::process::exit(1);
            }
            Err(e) => {
                // Connection error, not auth related - just proceed
                eprintln!("Warning: auth probe failed: {}", e);
            }
        }
    }

    Ok(client)
}
