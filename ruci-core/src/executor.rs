//! Executor module
//!
//! Handles job execution with context limits

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{watch, Mutex};
use tokio::time::{timeout, Duration};

use crate::config::{Config, ContextConfig};
use crate::error::{Error, ExecutorError, Result};
use crate::vcs::VcsInfo;

/// Job step definition
#[derive(Debug, Clone)]
pub struct JobStep {
    pub name: String,
    pub command: String,
    pub artifacts: Vec<String>,
}

/// Job definition parsed from YAML
#[derive(Debug, Clone)]
pub struct Job {
    pub name: String,
    pub context: String,
    pub timeout: u64,
    pub env: HashMap<String, String>,
    pub steps: Vec<JobStep>,
    /// VCS configuration for automatic checkout
    pub vcs: Option<VcsInfo>,
    /// Whether to perform VCS checkout before running steps (default: true)
    pub checkout: bool,
}

impl Job {
    pub fn parse(yaml: &str) -> Result<Self> {
        #[derive(serde::Deserialize)]
        struct RawJob {
            name: String,
            #[serde(default = "default_context")]
            context: String,
            #[serde(default = "default_timeout")]
            timeout: u64,
            #[serde(default)]
            env: HashMap<String, String>,
            steps: Vec<RawStep>,
            #[serde(default)]
            vcs: Option<VcsInfo>,
            #[serde(default = "default_checkout")]
            checkout: bool,
        }

        #[derive(serde::Deserialize)]
        struct RawStep {
            name: String,
            command: String,
            #[serde(default)]
            artifacts: Vec<String>,
        }

        fn default_context() -> String {
            "default".to_string()
        }

        fn default_timeout() -> u64 {
            3600
        }

        fn default_checkout() -> bool {
            true
        }

        let raw: RawJob =
            yaml_serde::from_str(yaml).map_err(|e| ExecutorError::InvalidStep(e.to_string()))?;

        Ok(Self {
            name: raw.name,
            context: raw.context,
            timeout: raw.timeout,
            env: raw.env,
            steps: raw
                .steps
                .into_iter()
                .map(|s| JobStep {
                    name: s.name,
                    command: s.command,
                    artifacts: s.artifacts,
                })
                .collect(),
            vcs: raw.vcs,
            checkout: raw.checkout,
        })
    }
}

/// Job execution context
pub struct ExecutionContext {
    pub run_id: String,
    pub job_id: String,
    pub build_num: u64,
    pub work_dir: PathBuf,
    pub env: HashMap<String, String>,
    pub params: HashMap<String, String>,
}

/// Executor trait for job execution
///
/// Extension point for custom executors (e.g., Docker, remote SSH)
#[async_trait]
pub trait Executor: Send + Sync {
    /// Execute a job
    async fn execute(&self, ctx: &ExecutionContext, job: &Job) -> Result<ExecutionResult>;

    /// Abort a running job
    async fn abort(&self, run_id: &str) -> Result<()>;
}

/// Execution result
#[derive(Debug)]
pub struct ExecutionResult {
    pub exit_code: i32,
    pub logs: String,
    pub artifacts: Vec<Artifact>,
}

/// Artifact produced by a step
#[derive(Debug)]
pub struct Artifact {
    pub name: String,
    pub path: String,
    pub size: u64,
}

/// Bash executor implementation
pub struct BashExecutor {
    config: Arc<Config>,
    /// Track running child process PIDs by run_id
    processes: Arc<Mutex<HashMap<String, u32>>>,
}

impl BashExecutor {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            config,
            processes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn get_context(&self, name: &str) -> Result<&ContextConfig> {
        self.config
            .contexts
            .get(name)
            .ok_or_else(|| ExecutorError::ContextNotFound(name.to_string()).into())
    }

    async fn run_step(
        &self,
        ctx: &ExecutionContext,
        step: &JobStep,
        cancel_rx: &mut watch::Receiver<bool>,
    ) -> Result<(i32, String)> {
        let mut cmd = Command::new("bash");
        cmd.args(["-c", &step.command]);
        cmd.current_dir(&ctx.work_dir);

        // Set environment
        let mut env = ctx.env.clone();
        env.insert("RUCI_RUN_ID".to_string(), ctx.run_id.clone());
        env.insert("RUCI_JOB_ID".to_string(), ctx.job_id.clone());
        env.insert("RUCI_BUILD_NUM".to_string(), ctx.build_num.to_string());
        env.extend(ctx.params.clone());
        cmd.envs(&env);

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child: Child = cmd
            .spawn()
            .map_err(|e| ExecutorError::SpawnFailed(e.to_string()))?;

        // Register child process PID for abort capability
        if let Some(pid) = child.id() {
            let mut processes = self.processes.lock().await;
            processes.insert(ctx.run_id.clone(), pid);
        }

        let mut stdout = String::new();
        let mut stderr = String::new();

        // Read stdout
        if let Some(out) = child.stdout.take() {
            let mut reader = BufReader::new(out).lines();
            loop {
                tokio::select! {
                    line = reader.next_line() => {
                        match line {
                            Ok(Some(l)) => {
                                stdout.push_str(&l);
                                stdout.push('\n');
                            }
                            Ok(None) => break,
                            Err(_) => break,
                        }
                    }
                    _ = cancel_rx.changed() => {
                        if *cancel_rx.borrow() {
                            child.kill().await.ok();
                            return Err(Error::Executor(ExecutorError::Aborted));
                        }
                    }
                }
            }
        }

        // Read stderr
        if let Some(err) = child.stderr.take() {
            let mut reader = BufReader::new(err).lines();
            while let Ok(Some(l)) = reader.next_line().await {
                stderr.push_str(&l);
                stderr.push('\n');
            }
        }

        let status = child
            .wait()
            .await
            .map_err(|e| ExecutorError::SpawnFailed(e.to_string()))?;

        let exit_code = status.code().unwrap_or(-1);
        let full_output = format!(
            "{}\n{}\nSTDOUT:\n{}\nSTDERR:\n{}",
            step.name,
            "=".repeat(40),
            stdout,
            stderr
        );

        // Unregister child process on completion
        {
            let mut processes = self.processes.lock().await;
            processes.remove(&ctx.run_id);
        }

        Ok((exit_code, full_output))
    }
}

#[async_trait]
impl Executor for BashExecutor {
    async fn execute(&self, ctx: &ExecutionContext, job: &Job) -> Result<ExecutionResult> {
        tracing::info!(
            run_id=%ctx.run_id,
            job_id=%ctx.job_id,
            job_name=%job.name,
            context=%job.context,
            timeout=%job.timeout,
            steps=%job.steps.len(),
            "Starting job execution"
        );

        let context_config = self.get_context(&job.context)?;
        tracing::debug!(
            context=%job.context,
            work_dir=%ctx.work_dir.display(),
            "Using execution context"
        );

        // Create work directory
        let work_dir = ctx.work_dir.clone();
        if let Err(e) = std::fs::create_dir_all(&work_dir) {
            tracing::error!(
                work_dir=%work_dir.display(),
                error=%e,
                "Failed to create work directory"
            );
        }
        tracing::debug!(work_dir=%work_dir.display(), "Work directory ready");

        // Create cancel channel
        let (cancel_tx, mut cancel_rx) = watch::channel(false);

        let mut logs = String::new();
        let artifacts = Vec::new();

        for (step_idx, step) in job.steps.iter().enumerate() {
            tracing::info!(
                run_id=%ctx.run_id,
                step=%step_idx + 1,
                total_steps=%job.steps.len(),
                step_name=%step.name,
                "Executing step"
            );

            // Check timeout
            let step_timeout = job.timeout.max(context_config.timeout);
            tracing::debug!(timeout=%step_timeout, "Step timeout configured");

            let result = timeout(
                Duration::from_secs(step_timeout),
                self.run_step(ctx, step, &mut cancel_rx),
            )
            .await;

            match result {
                Ok(Ok((exit_code, output))) => {
                    logs.push_str(&output);
                    logs.push('\n');

                    if exit_code != 0 {
                        tracing::warn!(
                            run_id=%ctx.run_id,
                            step=%step.name,
                            exit_code=%exit_code,
                            "Step failed with non-zero exit code"
                        );
                        return Ok(ExecutionResult {
                            exit_code,
                            logs,
                            artifacts,
                        });
                    }
                    tracing::info!(
                        run_id=%ctx.run_id,
                        step=%step.name,
                        exit_code=%exit_code,
                        "Step completed successfully"
                    );
                }
                Ok(Err(e)) => {
                    tracing::error!(
                        run_id=%ctx.run_id,
                        step=%step.name,
                        error=%e,
                        "Step execution error"
                    );
                    return Err(e);
                }
                Err(_) => {
                    tracing::error!(
                        run_id=%ctx.run_id,
                        step=%step.name,
                        timeout=%step_timeout,
                        "Step timed out"
                    );
                    let _ = cancel_tx.send(true);
                    return Err(ExecutorError::Timeout {
                        seconds: job.timeout,
                    }
                    .into());
                }
            }
        }

        tracing::info!(
            run_id=%ctx.run_id,
            job_name=%job.name,
            exit_code=0,
            "Job completed successfully"
        );

        Ok(ExecutionResult {
            exit_code: 0,
            logs,
            artifacts,
        })
    }

    async fn abort(&self, run_id: &str) -> Result<()> {
        tracing::warn!(run_id=%run_id, "Abort requested for run");

        // Find and kill the child process by PID
        let pid = {
            let mut processes = self.processes.lock().await;
            processes.remove(run_id)
        };

        if let Some(pid) = pid {
            tracing::info!(run_id=%run_id, pid=%pid, "Killing process using SIGTERM");
            // Use tokio's Command to kill the process
            let output = tokio::process::Command::new("kill")
                .arg("-TERM")
                .arg(pid.to_string())
                .output()
                .await;

            match output {
                Ok(output) => {
                    if output.status.success() {
                        tracing::info!(run_id=%run_id, pid=%pid, "SIGTERM sent successfully");
                        // Give it a moment then send SIGKILL if still alive
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        let _ = tokio::process::Command::new("kill")
                            .arg("-KILL")
                            .arg(pid.to_string())
                            .output()
                            .await;
                    } else {
                        tracing::error!(run_id=%run_id, pid=%pid, "Failed to send SIGTERM");
                    }
                }
                Err(e) => {
                    tracing::error!(run_id=%run_id, error=%e, "Failed to execute kill command");
                }
            }
        } else {
            tracing::debug!(run_id=%run_id, "No running process found for this run_id");
        }

        Ok(())
    }
}

impl Config {
    /// Calculate content hash for a job file
    pub fn hash_job_content(content: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let result = hasher.finalize();
        base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, result)
    }

    /// Generate short hash (16 chars) for file naming
    pub fn short_hash(content: &str) -> String {
        let full = Self::hash_job_content(content);
        full.chars().take(16).collect()
    }
}

// ═══════════════════════════════════════════════════════════════
// Plugin Extension Point
// ═══════════════════════════════════════════════════════════════

// Extension point for custom executors
//
// To add a new executor (e.g., Docker, Kubernetes):
// 1. Implement the `Executor` trait
// 2. Add a new variant to Config or use a registry

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_parse_basic() {
        let yaml = r#"
name: test-job
context: default
timeout: 3600
steps:
  - name: build
    command: echo hello
  - name: test
    command: echo world
"#;
        let job = Job::parse(yaml).expect("Failed to parse job");
        assert_eq!(job.name, "test-job");
        assert_eq!(job.context, "default");
        assert_eq!(job.timeout, 3600);
        assert_eq!(job.steps.len(), 2);
        assert_eq!(job.steps[0].name, "build");
        assert_eq!(job.steps[1].command, "echo world");
    }

    #[test]
    fn test_job_parse_with_env() {
        let yaml = r#"
name: env-job
env:
  FOO: bar
  BAZ: qux
steps:
  - name: test
    command: echo $FOO
"#;
        let job = Job::parse(yaml).expect("Failed to parse job");
        assert_eq!(job.env.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(job.env.get("BAZ"), Some(&"qux".to_string()));
    }

    #[test]
    fn test_job_parse_default_values() {
        let yaml = r#"
name: minimal-job
steps:
  - name: step1
    command: echo hello
"#;
        let job = Job::parse(yaml).expect("Failed to parse job");
        assert_eq!(job.context, "default");
        assert_eq!(job.timeout, 3600);
        assert!(job.env.is_empty());
    }

    #[test]
    fn test_job_parse_with_artifacts() {
        let yaml = r#"
name: artifact-job
steps:
  - name: build
    command: make
    artifacts:
      - dist/*
      - target/binary
"#;
        let job = Job::parse(yaml).expect("Failed to parse job");
        assert_eq!(job.steps[0].artifacts.len(), 2);
        assert_eq!(job.steps[0].artifacts[0], "dist/*");
    }

    #[test]
    fn test_hash_job_content() {
        let content = "test content";
        let hash1 = Config::hash_job_content(content);
        let hash2 = Config::hash_job_content(content);

        // Same content should produce same hash
        assert_eq!(hash1, hash2);

        // Hash should be base64 URL-safe encoded
        assert!(!hash1.contains('+'));
        assert!(!hash1.contains('/'));
    }

    #[test]
    fn test_short_hash() {
        let content = "test content for short hash";
        let short = Config::short_hash(content);

        // Short hash should be 16 characters
        assert_eq!(short.len(), 16);

        // Should be consistent
        assert_eq!(short, Config::short_hash(content));
    }

    #[test]
    fn test_execution_result() {
        let result = ExecutionResult {
            exit_code: 0,
            logs: "Build completed".to_string(),
            artifacts: vec![],
        };
        assert_eq!(result.exit_code, 0);
        assert!(result.logs.contains("Build"));
    }

    #[test]
    fn test_artifact() {
        let artifact = Artifact {
            name: "binary".to_string(),
            path: "/path/to/binary".to_string(),
            size: 1024,
        };
        assert_eq!(artifact.name, "binary");
        assert_eq!(artifact.size, 1024);
    }

    #[test]
    fn test_job_parse_invalid_yaml() {
        let yaml = "invalid: yaml: content: [";
        let result = Job::parse(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_job_parse_missing_name() {
        let yaml = r#"
context: default
steps:
  - name: step1
    command: echo hello
"#;
        let result = Job::parse(yaml);
        // name is required, should fail
        assert!(result.is_err());
    }

    #[test]
    fn test_job_parse_missing_steps() {
        let yaml = r#"
name: no-steps-job
"#;
        let result = Job::parse(yaml);
        // steps is required, should fail
        assert!(result.is_err());
    }

    #[test]
    fn test_job_parse_empty_steps() {
        let yaml = r#"
name: empty-steps-job
steps: []
"#;
        let result = Job::parse(yaml);
        // Empty steps array - this might be valid or invalid depending on design
        // Currently passes with 0 steps
        let job = result.expect("Should parse even with empty steps");
        assert_eq!(job.steps.len(), 0);
    }

    #[test]
    fn test_job_parse_step_missing_name() {
        let yaml = r#"
name: step-no-name
steps:
  - command: echo hello
"#;
        let result = Job::parse(yaml);
        // step name is required
        assert!(result.is_err());
    }

    #[test]
    fn test_job_parse_step_missing_command() {
        let yaml = r#"
name: step-no-command
steps:
  - name: step1
"#;
        let result = Job::parse(yaml);
        // command is required
        assert!(result.is_err());
    }

    #[test]
    fn test_job_parse_with_special_chars_in_env() {
        let yaml = r#"
name: special-chars-job
env:
  PATH: "/usr/bin:$PATH"
  SPECIAL: "value with 'quotes' and \"double quotes\""
steps:
  - name: test
    command: echo $SPECIAL
"#;
        let job = Job::parse(yaml).expect("Should parse with special chars");
        assert_eq!(job.env.get("PATH"), Some(&"/usr/bin:$PATH".to_string()));
        assert_eq!(
            job.env.get("SPECIAL"),
            Some(&"value with 'quotes' and \"double quotes\"".to_string())
        );
    }

    #[test]
    fn test_job_parse_with_numeric_env_values() {
        let yaml = r#"
name: numeric-env-job
env:
  PORT: "8080"
  TIMEOUT: "3600"
steps:
  - name: test
    command: echo $PORT
"#;
        let job = Job::parse(yaml).expect("Should parse with numeric strings");
        assert_eq!(job.env.get("PORT"), Some(&"8080".to_string()));
    }

    #[test]
    fn test_job_step() {
        let step = JobStep {
            name: "build".to_string(),
            command: "cargo build".to_string(),
            artifacts: vec!["target/debug/*".to_string()],
        };
        assert_eq!(step.name, "build");
        assert_eq!(step.command, "cargo build");
        assert_eq!(step.artifacts.len(), 1);
    }

    #[test]
    fn test_execution_context() {
        let ctx = ExecutionContext {
            run_id: "run-123".to_string(),
            job_id: "job-456".to_string(),
            build_num: 42,
            work_dir: std::path::PathBuf::from("/tmp/work"),
            env: std::collections::HashMap::new(),
            params: std::collections::HashMap::new(),
        };
        assert_eq!(ctx.run_id, "run-123");
        assert_eq!(ctx.job_id, "job-456");
        assert_eq!(ctx.build_num, 42);
    }

    #[test]
    fn test_execution_result_with_artifacts() {
        let artifact = Artifact {
            name: "binary".to_string(),
            path: "/tmp/binary".to_string(),
            size: 2048,
        };
        let result = ExecutionResult {
            exit_code: 0,
            logs: "Build successful".to_string(),
            artifacts: vec![artifact],
        };
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.artifacts.len(), 1);
        assert_eq!(result.artifacts[0].name, "binary");
    }

    #[test]
    fn test_execution_result_failure() {
        let result = ExecutionResult {
            exit_code: 1,
            logs: "Build failed: compilation error".to_string(),
            artifacts: vec![],
        };
        assert_eq!(result.exit_code, 1);
        assert!(result.logs.contains("failed"));
    }

    #[test]
    fn test_hash_different_content() {
        let hash1 = Config::hash_job_content("content 1");
        let hash2 = Config::hash_job_content("content 2");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_hash_empty_content() {
        let hash = Config::hash_job_content("");
        assert!(!hash.is_empty());
        // Should still be consistent
        assert_eq!(hash, Config::hash_job_content(""));
    }

    #[test]
    fn test_short_hash_different_content() {
        let short1 = Config::short_hash("content 1");
        let short2 = Config::short_hash("content 2");
        assert_ne!(short1, short2);
    }

    #[test]
    fn test_short_hash_empty() {
        let short = Config::short_hash("");
        assert_eq!(short.len(), 16);
    }

    #[test]
    fn test_short_hash_consistency() {
        let content = "consistent content";
        let short1 = Config::short_hash(content);
        let short2 = Config::short_hash(content);
        assert_eq!(short1, short2);
    }

    #[test]
    fn test_bash_executor_new() {
        let config = Config::default();
        let executor = BashExecutor::new(std::sync::Arc::new(config));
        // Just verify it can be created
    }

    #[test]
    fn test_bash_executor_get_context_default() {
        let config = Config::default();
        let executor = BashExecutor::new(std::sync::Arc::new(config));
        let context = executor.get_context("default");
        assert!(context.is_ok());
        let ctx = context.unwrap();
        assert_eq!(ctx.max_parallel, 4);
        assert_eq!(ctx.timeout, 3600);
    }

    #[test]
    fn test_bash_executor_get_context_not_found() {
        let config = Config::default();
        let executor = BashExecutor::new(std::sync::Arc::new(config));
        let context = executor.get_context("nonexistent");
        assert!(context.is_err());
    }
}
