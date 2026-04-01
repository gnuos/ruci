//! Ruci CLI - Command Line Interface
//!
//! Client for the Ruci CI system

use std::collections::HashMap;
use std::io::Write;

use clap::{Args, Parser, Subcommand, ValueEnum};

use ruci_protocol::RunStatus;

#[derive(Parser)]
#[command(name = "ruci")]
#[command(about = "Ruci CI Client", long_about = None)]
struct Cli {
    #[arg(short, long, help = "RPC server address")]
    server: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Daemon management
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },

    /// Job management
    Job {
        #[command(subcommand)]
        action: JobAction,
    },

    /// Submit a job (Travis CI style)
    Submit {
        #[command(subcommand)]
        action: SubmitAction,
    },

    /// Local run
    Run {
        #[command(subcommand)]
        action: RunAction,
    },

    /// Config management
    Config {
        #[command(subcommand)]
        action: ConfigAction,
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
enum DaemonAction {
    Start,
    Stop,
    Restart,
}

#[derive(Subcommand)]
enum JobAction {
    /// List all jobs
    List,

    /// Queue a job
    Queue {
        #[arg(help = "Job ID")]
        job_id: String,

        #[arg(long, help = "Parameters as KEY=VALUE")]
        params: Vec<String>,
    },

    /// Abort a running job
    Abort {
        #[arg(help = "Run ID")]
        run_id: String,
    },

    /// Get job info
    Get {
        #[arg(help = "Job ID")]
        job_id: String,
    },
}

#[derive(Subcommand)]
enum SubmitAction {
    /// Submit from current directory
    Run {
        #[arg(short, long, help = "Job file path")]
        file: Option<String>,

        #[arg(short, long, help = "Wait for completion")]
        wait: bool,

        #[arg(short, long, help = "Follow log output")]
        follow: bool,
    },

    /// Submit from CI environment
    Ci {
        #[arg(help = "YAML content")]
        yaml: Option<String>,
    },
}

#[derive(Subcommand)]
enum RunAction {
    /// Start a local run
    Start {
        #[arg(help = "Job file path")]
        file: String,
    },

    /// Watch a run
    Watch {
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
enum ConfigAction {
    /// Show current config
    Show,

    /// Validate config file
    Validate {
        #[arg(short, long, help = "Config file path")]
        file: Option<String>,
    },
}

#[derive(Args)]
struct CompletionsCmd {
    #[arg(value_enum, default_value = "bash")]
    shell: Shell,
}

#[derive(ValueEnum, Clone)]
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
        Commands::Daemon { action } => handle_daemon(action),
        Commands::Job { action } => handle_job(cli.server, action).await?,
        Commands::Submit { action } => handle_submit(cli.server, action).await?,
        Commands::Run { action } => handle_run(cli.server, action).await?,
        Commands::Config { action } => handle_config(action),
        Commands::Status => handle_status(cli.server).await?,
        Commands::Validate { file } => handle_validate(file),
        Commands::Completions(cmd) => handle_completions(cmd),
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────
// Daemon handlers
// ─────────────────────────────────────────────────────────────────

fn handle_daemon(action: DaemonAction) {
    match action {
        DaemonAction::Start => println!("Starting rucid daemon..."),
        DaemonAction::Stop => println!("Stopping rucid daemon..."),
        DaemonAction::Restart => println!("Restarting rucid daemon..."),
    }
}

// ─────────────────────────────────────────────────────────────────
// Job handlers
// ─────────────────────────────────────────────────────────────────

async fn handle_job(server: Option<String>, action: JobAction) -> anyhow::Result<()> {
    let client = connect(server.as_deref()).await?;

    match action {
        JobAction::List => {
            let jobs = client.list_jobs(tarpc::context::current()).await?;
            println!("Jobs:");
            for job in jobs {
                println!("  {} ({}) - {}", job.id, job.original_name, job.name);
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
            println!(
                "Queued: run_id={} build_num={} status={}",
                resp.run_id, resp.build_num, resp.status
            );
        }
        JobAction::Abort { run_id } => {
            client.abort_job(tarpc::context::current(), run_id).await?;
            println!("Aborted");
        }
        JobAction::Get { job_id } => {
            if let Some(job) = client.get_job(tarpc::context::current(), job_id).await? {
                println!("Job: {} ({})", job.name, job.id);
                println!("  Original: {}", job.original_name);
                println!("  Submitted: {}", job.submitted_at);
            } else {
                println!("Job not found");
            }
        }
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────
// Submit handlers
// ─────────────────────────────────────────────────────────────────

async fn handle_submit(server: Option<String>, action: SubmitAction) -> anyhow::Result<()> {
    match action {
        SubmitAction::Run { file, wait, follow } => {
            let yaml_content = if let Some(path) = file {
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
            println!(
                "Submitted: job_id={} run_id={} build_num={}",
                resp.job_id, resp.run_id, resp.build_num
            );

            if wait {
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
        }
        SubmitAction::Ci { yaml } => {
            let yaml_content = yaml.unwrap_or_else(|| {
                eprintln!("Error: No YAML content provided");
                std::process::exit(1);
            });

            let client = connect(server.as_deref()).await?;
            let resp = client
                .submit_job(tarpc::context::current(), yaml_content)
                .await?;
            println!(
                "Submitted: job_id={} run_id={} build_num={}",
                resp.job_id, resp.run_id, resp.build_num
            );
        }
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────
// Run handlers
// ─────────────────────────────────────────────────────────────────

async fn handle_run(server: Option<String>, action: RunAction) -> anyhow::Result<()> {
    match action {
        RunAction::Start { file } => {
            let yaml_content = std::fs::read_to_string(&file)?;
            let client = connect(server.as_deref()).await?;
            let resp = client
                .submit_job(tarpc::context::current(), yaml_content)
                .await?;
            println!("Started: run_id={}", resp.run_id);
        }
        RunAction::Watch { run_id } => {
            let client = connect(server.as_deref()).await?;
            loop {
                if let Some(run) = client
                    .get_run(tarpc::context::current(), run_id.clone())
                    .await?
                {
                    println!("Status: {:?}", run.status);
                    if matches!(
                        run.status,
                        RunStatus::Success | RunStatus::Failed | RunStatus::Aborted
                    ) {
                        break;
                    }
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        }
        RunAction::Artifact { action } => match action {
            ArtifactAction::Upload { run_id, path } => {
                let client = connect(server.as_deref()).await?;
                let artifact = client
                    .upload_artifact(tarpc::context::current(), run_id, path)
                    .await?;
                println!("Uploaded: {}", artifact.name);
            }
            ArtifactAction::List { run_id } => {
                let client = connect(server.as_deref()).await?;
                let artifacts = client
                    .list_artifacts(tarpc::context::current(), run_id)
                    .await?;
                for a in artifacts {
                    println!("  {} ({} bytes)", a.name, a.size);
                }
            }
            ArtifactAction::Download { artifact_id } => {
                let client = connect(server.as_deref()).await?;
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
// Config handlers
// ─────────────────────────────────────────────────────────────────

fn handle_config(action: ConfigAction) {
    match action {
        ConfigAction::Show => {
            println!("Config (showing defaults):");
            println!("  server.host: 127.0.0.0");
            println!("  server.port: 7741");
            println!("  server.web_port: 8080");
        }
        ConfigAction::Validate { .. } => {
            println!("Config validation not available in CLI-only build");
        }
    }
}

// ─────────────────────────────────────────────────────────────────
// Validate handlers
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
            println!("✓ Job '{}' is valid", job.name);
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
            eprintln!("✗ Job configuration is invalid:");
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
// Connection helper
// ─────────────────────────────────────────────────────────────────

async fn connect(addr: Option<&str>) -> anyhow::Result<ruci_protocol::RuciRpcClient> {
    let addr = addr.unwrap_or("127.0.0.1:7741");

    let transport = tarpc::serde_transport::tcp::connect(addr, || {
        tarpc::tokio_serde::formats::Json::<_, _>::default()
    })
    .await?;
    let client =
        ruci_protocol::RuciRpcClient::new(tarpc::client::Config::default(), transport).spawn();

    Ok(client)
}
