//! VCS (Version Control System) module
//!
//! Provides unified VCS operations and types for Git-based platforms

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::error::{Error, Result};

/// VCS platform types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VcsType {
    Github,
    Gitlab,
    Gitea,
    Gogs,
    /// Custom Git server (self-hosted GitLab, Gitea, etc.)
    Custom,
}

impl std::fmt::Display for VcsType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VcsType::Github => write!(f, "github"),
            VcsType::Gitlab => write!(f, "gitlab"),
            VcsType::Gitea => write!(f, "gitea"),
            VcsType::Gogs => write!(f, "gogs"),
            VcsType::Custom => write!(f, "custom"),
        }
    }
}

/// VCS credentials for authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcsCredentials {
    /// Username for authentication
    pub username: Option<String>,
    /// Password or access token
    pub password: Option<String>,
    /// SSH private key path (optional)
    pub ssh_key: Option<String>,
}

impl VcsCredentials {
    /// Build git credential URL from credentials
    pub fn apply_to_url(&self, url: &str) -> String {
        if let (Some(user), Some(pass)) = (&self.username, &self.password) {
            // Replace protocol with embedded credentials
            if url.starts_with("https://") {
                return url.replacen("https://", &format!("https://{}:{}@", user, pass), 1);
            } else if url.starts_with("http://") {
                return url.replacen("http://", &format!("http://{}:{}@", user, pass), 1);
            }
        }
        url.to_string()
    }
}

/// Sanitize a URL for safe logging (redact credentials)
fn sanitize_url_for_log(url: &str) -> String {
    // Strip embedded credentials from URL: https://user:pass@host -> https://host
    for scheme in &["https://", "http://", "git://"] {
        if let Some(rest) = url.strip_prefix(scheme) {
            if let Some(at_pos) = rest.find('@') {
                return format!("{}<redacted>@{}", scheme, &rest[at_pos + 1..]);
            }
        }
    }
    // Strip token-based auth: https://token:x-oauth-basic@host
    if url.contains("x-oauth-basic") || url.contains("x-token-auth") {
        for scheme in &["https://", "http://"] {
            if let Some(rest) = url.strip_prefix(scheme) {
                if let Some(at_pos) = rest.find('@') {
                    return format!("{}<redacted>@{}", scheme, &rest[at_pos + 1..]);
                }
            }
        }
    }
    url.to_string()
}

/// VCS repository information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcsInfo {
    /// Repository URL (HTTPS or SSH)
    pub url: String,
    /// Repository name (owner/repo format)
    pub repository: String,
    /// Branch to checkout (default: main)
    #[serde(default = "default_branch")]
    pub branch: String,
    /// Specific commit SHA to checkout (optional)
    pub commit: Option<String>,
    /// Whether to fetch submodules
    #[serde(default)]
    pub submodules: bool,
    /// Credentials reference (credential_id from database)
    #[serde(default)]
    pub credential_id: Option<String>,
}

fn default_branch() -> String {
    "main".to_string()
}

/// Unified VCS event information parsed from webhook
#[derive(Debug, Clone)]
pub struct VcsEvent {
    /// VCS platform type
    pub vcs_type: VcsType,
    /// Repository full name (owner/repo)
    pub repository: String,
    /// Clone URL
    pub clone_url: String,
    /// Branch name (without refs/heads/)
    pub branch: Option<String>,
    /// Commit SHA
    pub commit_sha: Option<String>,
    /// Default branch of the repository
    pub default_branch: String,
    /// Event type
    pub event: VcsEventType,
    /// User who triggered the event
    pub sender: Option<String>,
}

/// VCS event types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VcsEventType {
    Push,
    TagPush,
    PullRequest,
    MergeRequest,
    Note,
    Issue,
    Release,
}

/// VCS operation trait - extension point for different VCS systems
#[async_trait]
pub trait VcsOperations: Send + Sync {
    /// Clone repository
    async fn clone(&self, url: &str, branch: &str, work_dir: &Path, submodules: bool)
        -> Result<()>;

    /// Fetch updates
    async fn fetch(&self, url: &str, branch: &str, work_dir: &Path) -> Result<()>;

    /// Checkout specific ref (branch, tag, or commit)
    async fn checkout(&self, work_dir: &Path, ref_: &str) -> Result<()>;

    /// Get current commit SHA
    async fn get_current_commit(&self, work_dir: &Path) -> Result<String>;
}

/// Git operations implementation
pub struct GitOperations {
    /// SSH key path for authentication
    ssh_key: Option<String>,
}

impl GitOperations {
    pub fn new(ssh_key: Option<String>) -> Self {
        Self { ssh_key }
    }

    /// Build git command with SSH settings
    fn build_git_cmd(&self) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new("git");
        if let Some(ref key) = self.ssh_key {
            cmd.env("GIT_SSH_KEY", key);
        }
        cmd
    }

    async fn run_git(&self, args: &[&str], work_dir: &Path) -> Result<()> {
        let output = self
            .build_git_cmd()
            .args(args)
            .current_dir(work_dir)
            .output()
            .await
            .map_err(|e| Error::Other(format!("git command failed: {}", e)))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(Error::Other(format!("git command failed: {}", stderr)))
        }
    }
}

#[async_trait]
impl VcsOperations for GitOperations {
    async fn clone(
        &self,
        url: &str,
        branch: &str,
        work_dir: &Path,
        submodules: bool,
    ) -> Result<()> {
        tracing::info!(url = %sanitize_url_for_log(url), branch = %branch, work_dir = %work_dir.display(), "Cloning repository");

        // Create parent directory if needed
        if let Some(parent) = work_dir.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::Other(format!("failed to create directory: {}", e)))?;
        }

        // Git clone with branch, shallow clone for efficiency
        let mut args = vec!["clone", "--branch", branch, "--depth", "1", url];
        if submodules {
            args.push("--recurse-submodules");
        }
        args.push(work_dir.to_str().unwrap_or("."));

        let status = self
            .build_git_cmd()
            .args(&args)
            .status()
            .await
            .map_err(|e| Error::Other(format!("git clone failed: {}", e)))?;

        if !status.success() {
            return Err(Error::Other(format!(
                "git clone failed with status: {}",
                status
            )));
        }

        tracing::info!(work_dir = %work_dir.display(), "Repository cloned successfully");
        Ok(())
    }

    async fn fetch(&self, url: &str, branch: &str, work_dir: &Path) -> Result<()> {
        tracing::info!(url = %sanitize_url_for_log(url), branch = %branch, work_dir = %work_dir.display(), "Fetching updates");

        // Add remote if not already present
        self.run_git(&["remote", "add", "origin", url], work_dir)
            .await
            .ok();
        // Fetch the branch
        self.run_git(
            &["fetch", "origin", &format!("refs/heads/{}", branch)],
            work_dir,
        )
        .await
    }

    async fn checkout(&self, work_dir: &Path, ref_: &str) -> Result<()> {
        tracing::info!(ref = %ref_, work_dir = %work_dir.display(), "Checking out ref");

        // First try as branch
        let result = self.run_git(&["checkout", ref_], work_dir).await;
        if result.is_ok() {
            return result;
        }

        // Fallback: try as commit SHA
        self.run_git(&["checkout", ref_], work_dir).await
    }

    async fn get_current_commit(&self, work_dir: &Path) -> Result<String> {
        let output = self
            .build_git_cmd()
            .args(["rev-parse", "HEAD"])
            .current_dir(work_dir)
            .output()
            .await
            .map_err(|e| Error::Other(format!("git rev-parse failed: {}", e)))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(Error::Other("failed to get current commit".to_string()))
        }
    }
}

/// Perform VCS checkout with automatic clone/fetch decision
pub async fn checkout(
    vcs_info: &VcsInfo,
    work_dir: &Path,
    params: &HashMap<String, String>,
    git_ops: &GitOperations,
) -> Result<()> {
    // Override with params from webhook if provided
    let url = params
        .get("vcs_url")
        .map(|s| s.as_str())
        .unwrap_or(&vcs_info.url);
    let branch = params
        .get("vcs_branch")
        .map(|s| s.as_str())
        .unwrap_or(&vcs_info.branch);
    let commit = params
        .get("vcs_commit")
        .map(|s| s.as_str())
        .or(vcs_info.commit.as_deref());

    // Apply credentials to URL
    let url_with_creds = if let Some(_cred_id) = &vcs_info.credential_id {
        // In real implementation, would fetch credentials from database using cred_id
        // For now, just use URL as-is
        url.to_string()
    } else {
        url.to_string()
    };

    tracing::info!(
        url = %sanitize_url_for_log(&url_with_creds),
        branch = %branch,
        commit = ?commit,
        work_dir = %work_dir.display(),
        "Performing VCS checkout"
    );

    // Check if directory exists and has .git
    let needs_clone = !work_dir.exists() || !work_dir.join(".git").exists();

    if needs_clone {
        git_ops
            .clone(&url_with_creds, branch, work_dir, vcs_info.submodules)
            .await?;
    } else {
        // Already cloned, just fetch and update
        git_ops.fetch(&url_with_creds, branch, work_dir).await?;
        git_ops.checkout(work_dir, branch).await?;
    }

    // Checkout specific commit if provided
    if let Some(commit) = commit {
        git_ops.checkout(work_dir, commit).await?;
    }

    // Get and log current commit
    match git_ops.get_current_commit(work_dir).await {
        Ok(sha) => tracing::info!(commit = %sha, "Checkout complete"),
        Err(e) => tracing::warn!("Failed to get current commit: {}", e),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vcs_type_display() {
        assert_eq!(VcsType::Github.to_string(), "github");
        assert_eq!(VcsType::Gitlab.to_string(), "gitlab");
        assert_eq!(VcsType::Gogs.to_string(), "gogs");
        assert_eq!(VcsType::Custom.to_string(), "custom");
    }

    #[test]
    fn test_vcs_credentials_apply_to_url() {
        let creds = VcsCredentials {
            username: Some("user".to_string()),
            password: Some("token123".to_string()),
            ssh_key: None,
        };

        let url = "https://github.com/owner/repo.git";
        let result = creds.apply_to_url(url);
        assert_eq!(result, "https://user:token123@github.com/owner/repo.git");
    }

    #[test]
    fn test_vcs_credentials_no_change_for_ssh() {
        let creds = VcsCredentials {
            username: Some("user".to_string()),
            password: Some("token123".to_string()),
            ssh_key: None,
        };

        let url = "git@github.com:owner/repo.git";
        let result = creds.apply_to_url(url);
        assert_eq!(result, url); // Should not modify SSH URLs
    }

    #[test]
    fn test_vcs_info_default_branch() {
        let yaml = r#"
url: https://github.com/owner/repo.git
repository: owner/repo
"#;
        let info: VcsInfo = yaml_serde::from_str(yaml).unwrap();
        assert_eq!(info.branch, "main");
        assert!(!info.submodules);
        assert!(info.commit.is_none());
    }
}
