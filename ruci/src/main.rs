//! Ruci CLI - Command Line Interface
//!
//! Client for the Ruci CD system

use std::collections::HashMap;
use std::io::Write;

use clap::{Args, Parser, Subcommand, ValueEnum};

use ruci_protocol::RunStatus;

#[derive(Parser)]
#[command(name = "ruci")]
#[command(about = "Ruci Client", long_about = None)]
struct Cli {
    #[arg(short, long, help = "RPC server address")]
    server: Option<String>,

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

    match cli.command {
        Commands::Job { action } => handle_job(cli.server, action).await?,
        Commands::Run { action } => handle_run(cli.server, action).await?,
        Commands::Trigger { action } => handle_trigger(cli.server, action).await?,
        Commands::Submit {
            file,
            wait,
            follow,
            yaml,
        } => handle_submit(cli.server, file, wait, follow, yaml).await?,
        Commands::Status => handle_status(cli.server).await?,
        Commands::Validate { file } => handle_validate(file),
        Commands::Completions(cmd) => handle_completions(cmd),
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────
// Job handlers
// ─────────────────────────────────────────────────────────────────

async fn handle_job(server: Option<String>, action: JobAction) -> anyhow::Result<()> {
    let client = connect(server.as_deref()).await?;

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

async fn handle_run(server: Option<String>, action: RunAction) -> anyhow::Result<()> {
    let client = connect(server.as_deref()).await?;

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

async fn handle_trigger(server: Option<String>, action: TriggerAction) -> anyhow::Result<()> {
    let client = connect(server.as_deref()).await?;

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
    server: Option<String>,
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

    let client = connect(server.as_deref()).await?;
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

async fn handle_status(server: Option<String>) -> anyhow::Result<()> {
    let client = connect(server.as_deref()).await?;
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

async fn connect(addr: Option<&str>) -> anyhow::Result<ruci_protocol::RuciRpcClient> {
    let addr = addr.unwrap_or("127.0.0.1:7741");

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
