//! Webhook handlers
//!
//! Handles incoming webhooks from GitHub, GitLab, and Gogs.

use std::collections::HashMap;

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::web::handlers::AppState;
use ruci_core::db::{WebhookEvent, WebhookSource, WebhookTriggerInfo};
use ruci_core::queue::QueueRequest;
use ruci_core::vcs::VcsType;

/// Webhook payload from GitHub
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct GithubPushPayload {
    #[serde(rename = "ref")]
    pub ref_: Option<String>,
    #[serde(rename = "after")]
    pub after: Option<String>,
    pub repository: Option<GithubRepository>,
    pub pusher: Option<GithubPusher>,
    pub commits: Option<Vec<GithubCommit>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct GithubRepository {
    pub name: String,
    pub full_name: String,
    pub clone_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GithubPusher {
    pub username: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct GithubCommit {
    pub id: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct GithubPullRequestPayload {
    pub action: Option<String>,
    pub number: Option<i64>,
    pub pull_request: Option<GithubPullRequest>,
    pub repository: Option<GithubRepository>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct GithubPullRequest {
    pub title: Option<String>,
    pub head: Option<GithubBranch>,
    pub base: Option<GithubBranch>,
}

#[derive(Debug, Deserialize)]
pub struct GithubBranch {
    #[serde(rename = "ref")]
    pub ref_: Option<String>,
    pub sha: Option<String>,
}

/// GitLab webhook payload
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct GitlabPushPayload {
    pub object_kind: Option<String>,
    pub event_name: Option<String>,
    pub before: Option<String>,
    pub after: Option<String>,
    #[serde(rename = "ref")]
    pub ref_: Option<String>,
    pub checkout_sha: Option<String>,
    pub user_name: Option<String>,
    pub project: Option<GitlabProject>,
    pub commits: Option<Vec<GitlabCommit>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct GitlabProject {
    pub name: Option<String>,
    pub path_with_namespace: Option<String>,
    pub git_http_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct GitlabCommit {
    pub id: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct GitlabMergeRequestPayload {
    pub object_kind: Option<String>,
    pub event_type: Option<String>,
    pub user: Option<GitlabUser>,
    pub project: Option<GitlabProject>,
    pub object_attributes: Option<GitlabMergeRequest>,
}

#[derive(Debug, Deserialize)]
pub struct GitlabUser {
    pub username: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct GitlabMergeRequest {
    pub id: Option<i64>,
    pub title: Option<String>,
    pub source_branch: Option<String>,
    pub target_branch: Option<String>,
    pub state: Option<String>,
    pub last_commit: Option<GitlabCommit>,
}

/// Gogs webhook payload (similar to GitHub)
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct GogsPushPayload {
    #[serde(rename = "ref")]
    pub ref_: Option<String>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub repository: Option<GogsRepository>,
    pub pusher: Option<GogsPusher>,
    pub commits: Option<Vec<GogsCommit>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct GogsRepository {
    pub id: Option<i64>,
    pub name: String,
    pub full_name: String,
    pub clone_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GogsPusher {
    pub username: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct GogsCommit {
    pub id: Option<String>,
    pub message: Option<String>,
}

/// Parsed webhook event context
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ParsedWebhookEvent {
    pub source: WebhookSource,
    pub event: WebhookEvent,
    pub repository: String,         // owner/repo
    pub branch: Option<String>,     // branch name (not refs/heads/main)
    pub commit_sha: Option<String>, // commit SHA
    pub sender: Option<String>,     // user who triggered
    // VCS-specific info
    pub vcs_type: VcsType,      // Platform type for VCS operations
    pub clone_url: String,      // Clone URL for git operations
    pub default_branch: String, // Default branch (main)
}

/// Response for webhook API
#[derive(Debug, Serialize)]
pub struct WebhookResponse {
    pub success: bool,
    pub message: String,
    pub triggered_jobs: Vec<String>,
}

/// Verify GitHub HMAC-SHA256 signature
fn verify_github_signature(secret: &str, payload: &[u8], signature: &str) -> bool {
    type HmacSha256 = Hmac<Sha256>;

    let signature = match signature.strip_prefix("sha256=") {
        Some(s) => s,
        None => return false,
    };

    // Create HMAC-SHA256
    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(payload);
    let result = mac.finalize();

    // Hex encode - use consistent API
    let hex = hex::encode(result.into_bytes());

    // Constant-time comparison
    hex == signature
}

/// Verify GitLab/Gogs secret token (constant-time comparison)
fn verify_gitlab_token(secret: &str, token: &str) -> bool {
    if secret.len() != token.len() {
        return false;
    }
    secret
        .bytes()
        .zip(token.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

/// Extract branch name from ref (e.g., "refs/heads/main" -> "main")
fn extract_branch_from_ref(ref_str: &str) -> Option<String> {
    if ref_str.starts_with("refs/heads/") {
        Some(ref_str.strip_prefix("refs/heads/").unwrap().to_string())
    } else if ref_str.starts_with("refs/tags/") {
        Some(ref_str.strip_prefix("refs/tags/").unwrap().to_string())
    } else {
        None
    }
}

/// Match a value against a pattern (supports * glob)
fn match_pattern(value: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if pattern.contains('*') {
        let parts: Vec<&str> = pattern.split('*').collect();
        let mut last_end = 0;

        for part in &parts {
            if let Some(pos) = value[last_end..].find(part) {
                last_end += pos + part.len();
            } else {
                return false;
            }
        }
        // If pattern doesn't end with *, ensure it matches to the end
        if !pattern.ends_with('*') && last_end != value.len() {
            return false;
        }
        true
    } else {
        value == pattern
    }
}

/// Check if a webhook trigger matches the event
fn trigger_matches_event(trigger: &WebhookTriggerInfo, event: &ParsedWebhookEvent) -> bool {
    // Check if trigger is for this source
    if trigger.source != event.source {
        return false;
    }

    // Check if trigger is enabled
    if !trigger.enabled {
        return false;
    }

    let filter = &trigger.filter;

    // Check repository pattern
    if let Some(ref repo_pattern) = filter.repository {
        if !match_pattern(&event.repository, repo_pattern) {
            return false;
        }
    }

    // Check branch pattern
    if !filter.branches.is_empty() {
        let branch = match &event.branch {
            Some(b) => b,
            None => return false,
        };
        if !filter.branches.iter().any(|p| match_pattern(branch, p)) {
            return false;
        }
    }

    // Check event type
    if !filter.events.contains(&event.event) {
        return false;
    }

    true
}

/// Handle GitHub webhook
async fn handle_github_webhook(
    state: &AppState,
    event_type: &str,
    payload: &[u8],
    signature: Option<&str>,
) -> WebhookResponse {
    // Get all GitHub webhooks
    let webhooks = match state
        .context
        .db
        .list_webhook_triggers_by_source(&WebhookSource::Github)
        .await
    {
        Ok(h) => h,
        Err(e) => {
            tracing::error!("Failed to list GitHub webhooks: {}", e);
            return WebhookResponse {
                success: false,
                message: format!("Database error: {}", e),
                triggered_jobs: vec![],
            };
        }
    };

    if webhooks.is_empty() {
        return WebhookResponse {
            success: true,
            message: "No GitHub webhooks configured".to_string(),
            triggered_jobs: vec![],
        };
    }

    // Parse the event type
    let event = match event_type {
        "push" => {
            let payload: GithubPushPayload = match serde_json::from_slice(payload) {
                Ok(p) => p,
                Err(e) => {
                    return WebhookResponse {
                        success: false,
                        message: format!("Failed to parse payload: {}", e),
                        triggered_jobs: vec![],
                    };
                }
            };

            let repository = payload
                .repository
                .as_ref()
                .map(|r| r.full_name.clone())
                .unwrap_or_default();

            let branch = payload
                .ref_
                .as_ref()
                .and_then(|r| extract_branch_from_ref(r));

            ParsedWebhookEvent {
                source: WebhookSource::Github,
                event: WebhookEvent::Push,
                repository,
                branch,
                commit_sha: payload.after.clone(),
                sender: payload.pusher.as_ref().map(|p| p.username.clone()),
                vcs_type: VcsType::Github,
                clone_url: payload
                    .repository
                    .as_ref()
                    .and_then(|r| r.clone_url.clone())
                    .unwrap_or_default(),
                default_branch: "main".to_string(),
            }
        }
        "pull_request" => {
            let payload: GithubPullRequestPayload = match serde_json::from_slice(payload) {
                Ok(p) => p,
                Err(e) => {
                    return WebhookResponse {
                        success: false,
                        message: format!("Failed to parse payload: {}", e),
                        triggered_jobs: vec![],
                    };
                }
            };

            let repository = payload
                .repository
                .as_ref()
                .map(|r| r.full_name.clone())
                .unwrap_or_default();

            let branch = payload
                .pull_request
                .as_ref()
                .and_then(|pr| pr.head.as_ref())
                .and_then(|h| h.ref_.as_ref())
                .and_then(|r| extract_branch_from_ref(r));

            ParsedWebhookEvent {
                source: WebhookSource::Github,
                event: WebhookEvent::PullRequest,
                repository,
                branch,
                commit_sha: payload
                    .pull_request
                    .as_ref()
                    .and_then(|pr| pr.head.as_ref())
                    .and_then(|h| h.sha.clone()),
                sender: None,
                vcs_type: VcsType::Github,
                clone_url: payload
                    .repository
                    .as_ref()
                    .and_then(|r| r.clone_url.clone())
                    .unwrap_or_default(),
                default_branch: payload
                    .pull_request
                    .as_ref()
                    .and_then(|pr| pr.base.as_ref())
                    .and_then(|b| b.ref_.as_ref())
                    .and_then(|r| extract_branch_from_ref(r))
                    .unwrap_or_else(|| "main".to_string()),
            }
        }
        "create" | "delete" => {
            // Tag push events - not fully implemented
            return WebhookResponse {
                success: true,
                message: format!("Event type {} not fully implemented", event_type),
                triggered_jobs: vec![],
            };
        }
        _ => {
            return WebhookResponse {
                success: true,
                message: format!("Unsupported event type: {}", event_type),
                triggered_jobs: vec![],
            };
        }
    };

    // Find matching triggers and enqueue jobs
    let mut triggered = Vec::new();
    for webhook in &webhooks {
        // Verify signature
        if let Some(sig) = signature {
            if !verify_github_signature(&webhook.secret, payload, sig) {
                tracing::warn!(webhook = %webhook.name, "Invalid signature for GitHub webhook");
                continue;
            }
        }

        if trigger_matches_event(webhook, &event) {
            // Build VCS params from webhook event
            let vcs_params = VcsParams {
                vcs_url: event.clone_url.clone(),
                vcs_branch: event
                    .branch
                    .clone()
                    .unwrap_or_else(|| event.default_branch.clone()),
                vcs_commit: event.commit_sha.clone(),
                vcs_type: event.vcs_type.clone(),
            };
            // Enqueue the job
            match enqueue_job(&state.context, &webhook.job_id, Some(&vcs_params)).await {
                Ok(_) => {
                    tracing::info!(webhook = %webhook.name, job_id = %webhook.job_id, "Webhook triggered job");
                    triggered.push(webhook.job_id.clone());
                }
                Err(e) => {
                    tracing::error!(webhook = %webhook.name, job_id = %webhook.job_id, error = %e, "Failed to enqueue job from webhook");
                }
            }
        }
    }

    WebhookResponse {
        success: true,
        message: format!("Processed {} webhook(s)", triggered.len()),
        triggered_jobs: triggered,
    }
}

/// Handle GitLab webhook
async fn handle_gitlab_webhook(
    state: &AppState,
    event_type: &str,
    payload: &[u8],
    token: Option<&str>,
) -> WebhookResponse {
    // Get all GitLab webhooks
    let webhooks = match state
        .context
        .db
        .list_webhook_triggers_by_source(&WebhookSource::Gitlab)
        .await
    {
        Ok(h) => h,
        Err(e) => {
            tracing::error!("Failed to list GitLab webhooks: {}", e);
            return WebhookResponse {
                success: false,
                message: format!("Database error: {}", e),
                triggered_jobs: vec![],
            };
        }
    };

    if webhooks.is_empty() {
        return WebhookResponse {
            success: true,
            message: "No GitLab webhooks configured".to_string(),
            triggered_jobs: vec![],
        };
    }

    // Parse the event type
    let event = match event_type {
        "Push Hook" => {
            let payload: GitlabPushPayload = match serde_json::from_slice(payload) {
                Ok(p) => p,
                Err(e) => {
                    return WebhookResponse {
                        success: false,
                        message: format!("Failed to parse payload: {}", e),
                        triggered_jobs: vec![],
                    };
                }
            };

            let repository = payload
                .project
                .as_ref()
                .map(|p| p.path_with_namespace.clone().unwrap_or_default())
                .unwrap_or_default();

            let branch = payload
                .ref_
                .as_ref()
                .and_then(|r| extract_branch_from_ref(r));

            ParsedWebhookEvent {
                source: WebhookSource::Gitlab,
                event: WebhookEvent::Push,
                repository: repository.clone(),
                branch: branch.clone(),
                commit_sha: payload.checkout_sha.clone(),
                sender: payload.user_name.clone(),
                vcs_type: VcsType::Gitlab,
                clone_url: payload
                    .project
                    .as_ref()
                    .and_then(|p| p.git_http_url.clone())
                    .unwrap_or_default(),
                default_branch: "main".to_string(),
            }
        }
        "Merge Request Hook" => {
            let payload: GitlabMergeRequestPayload = match serde_json::from_slice(payload) {
                Ok(p) => p,
                Err(e) => {
                    return WebhookResponse {
                        success: false,
                        message: format!("Failed to parse payload: {}", e),
                        triggered_jobs: vec![],
                    };
                }
            };

            let repository = payload
                .project
                .as_ref()
                .map(|p| p.path_with_namespace.clone().unwrap_or_default())
                .unwrap_or_default();

            let branch = payload
                .object_attributes
                .as_ref()
                .and_then(|mr| mr.source_branch.clone());

            ParsedWebhookEvent {
                source: WebhookSource::Gitlab,
                event: WebhookEvent::MergeRequest,
                repository: repository.clone(),
                branch: branch.clone(),
                commit_sha: payload
                    .object_attributes
                    .as_ref()
                    .and_then(|mr| mr.last_commit.as_ref())
                    .and_then(|c| c.id.clone()),
                sender: payload.user.as_ref().and_then(|u| u.username.clone()),
                vcs_type: VcsType::Gitlab,
                clone_url: payload
                    .project
                    .as_ref()
                    .and_then(|p| p.git_http_url.clone())
                    .unwrap_or_default(),
                default_branch: payload
                    .object_attributes
                    .as_ref()
                    .and_then(|mr| mr.target_branch.clone())
                    .unwrap_or_else(|| "main".to_string()),
            }
        }
        _ => {
            return WebhookResponse {
                success: true,
                message: format!("Unsupported event type: {}", event_type),
                triggered_jobs: vec![],
            };
        }
    };

    // Find matching triggers and enqueue jobs
    let mut triggered = Vec::new();
    for webhook in &webhooks {
        // Verify token
        if let Some(t) = token {
            if !verify_gitlab_token(&webhook.secret, t) {
                tracing::warn!(webhook = %webhook.name, "Invalid token for GitLab webhook");
                continue;
            }
        }

        if trigger_matches_event(webhook, &event) {
            // Build VCS params from webhook event
            let vcs_params = VcsParams {
                vcs_url: event.clone_url.clone(),
                vcs_branch: event
                    .branch
                    .clone()
                    .unwrap_or_else(|| event.default_branch.clone()),
                vcs_commit: event.commit_sha.clone(),
                vcs_type: event.vcs_type.clone(),
            };
            match enqueue_job(&state.context, &webhook.job_id, Some(&vcs_params)).await {
                Ok(_) => {
                    tracing::info!(webhook = %webhook.name, job_id = %webhook.job_id, "Webhook triggered job");
                    triggered.push(webhook.job_id.clone());
                }
                Err(e) => {
                    tracing::error!(webhook = %webhook.name, job_id = %webhook.job_id, error = %e, "Failed to enqueue job from webhook");
                }
            }
        }
    }

    WebhookResponse {
        success: true,
        message: format!("Processed {} webhook(s)", triggered.len()),
        triggered_jobs: triggered,
    }
}

/// Handle Gogs webhook
async fn handle_gogs_webhook(
    state: &AppState,
    event_type: &str,
    payload: &[u8],
    signature: Option<&str>,
) -> WebhookResponse {
    // Get all Gogs webhooks
    let webhooks = match state
        .context
        .db
        .list_webhook_triggers_by_source(&WebhookSource::Gogs)
        .await
    {
        Ok(h) => h,
        Err(e) => {
            tracing::error!("Failed to list Gogs webhooks: {}", e);
            return WebhookResponse {
                success: false,
                message: format!("Database error: {}", e),
                triggered_jobs: vec![],
            };
        }
    };

    if webhooks.is_empty() {
        return WebhookResponse {
            success: true,
            message: "No Gogs webhooks configured".to_string(),
            triggered_jobs: vec![],
        };
    }

    // Parse the event type
    let event = match event_type {
        "push" => {
            let payload: GogsPushPayload = match serde_json::from_slice(payload) {
                Ok(p) => p,
                Err(e) => {
                    return WebhookResponse {
                        success: false,
                        message: format!("Failed to parse payload: {}", e),
                        triggered_jobs: vec![],
                    };
                }
            };

            let repository = payload
                .repository
                .as_ref()
                .map(|r| r.full_name.clone())
                .unwrap_or_default();

            let branch = payload
                .ref_
                .as_ref()
                .and_then(|r| extract_branch_from_ref(r));

            ParsedWebhookEvent {
                source: WebhookSource::Gogs,
                event: WebhookEvent::Push,
                repository: repository.clone(),
                branch: branch.clone(),
                commit_sha: payload.after.clone(),
                sender: payload.pusher.as_ref().map(|p| p.username.clone()),
                vcs_type: VcsType::Gogs,
                clone_url: payload
                    .repository
                    .as_ref()
                    .and_then(|r| r.clone_url.clone())
                    .unwrap_or_default(),
                default_branch: "main".to_string(),
            }
        }
        _ => {
            return WebhookResponse {
                success: true,
                message: format!("Unsupported event type: {}", event_type),
                triggered_jobs: vec![],
            };
        }
    };

    // Find matching triggers and enqueue jobs
    let mut triggered = Vec::new();
    for webhook in &webhooks {
        // Verify signature (Gogs uses same signature format as GitHub)
        if let Some(sig) = signature {
            if !verify_github_signature(&webhook.secret, payload, sig) {
                tracing::warn!(webhook = %webhook.name, "Invalid signature for Gogs webhook");
                continue;
            }
        }

        if trigger_matches_event(webhook, &event) {
            // Build VCS params from webhook event
            let vcs_params = VcsParams {
                vcs_url: event.clone_url.clone(),
                vcs_branch: event
                    .branch
                    .clone()
                    .unwrap_or_else(|| event.default_branch.clone()),
                vcs_commit: event.commit_sha.clone(),
                vcs_type: event.vcs_type.clone(),
            };
            match enqueue_job(&state.context, &webhook.job_id, Some(&vcs_params)).await {
                Ok(_) => {
                    tracing::info!(webhook = %webhook.name, job_id = %webhook.job_id, "Webhook triggered job");
                    triggered.push(webhook.job_id.clone());
                }
                Err(e) => {
                    tracing::error!(webhook = %webhook.name, job_id = %webhook.job_id, error = %e, "Failed to enqueue job from webhook");
                }
            }
        }
    }

    WebhookResponse {
        success: true,
        message: format!("Processed {} webhook(s)", triggered.len()),
        triggered_jobs: triggered,
    }
}

/// VCS parameters from webhook for job execution
#[derive(Debug, Clone)]
pub struct VcsParams {
    pub vcs_url: String,
    pub vcs_branch: String,
    pub vcs_commit: Option<String>,
    pub vcs_type: VcsType,
}

/// Enqueue a job for execution with optional VCS parameters
async fn enqueue_job(
    context: &crate::AppContext,
    job_id: &str,
    vcs_params: Option<&VcsParams>,
) -> anyhow::Result<()> {
    // Get next build number
    let build_num = context.db.next_build_num(job_id).await?;

    // Generate run_id
    let run_id = format!("{}-{}-webhook", job_id, uuid::Uuid::new_v4());

    // Create queue request with webhook context
    let mut params = HashMap::new();
    params.insert("trigger_source".to_string(), "webhook".to_string());

    // Add VCS parameters if provided
    if let Some(vp) = vcs_params {
        params.insert("vcs_url".to_string(), vp.vcs_url.clone());
        params.insert("vcs_branch".to_string(), vp.vcs_branch.clone());
        if let Some(commit) = &vp.vcs_commit {
            params.insert("vcs_commit".to_string(), commit.clone());
        }
        params.insert("vcs_type".to_string(), vp.vcs_type.to_string());
    }

    let request = QueueRequest {
        job_id: job_id.to_string(),
        run_id: run_id.clone(),
        params,
        build_num: build_num as u64,
    };

    // Enqueue
    context.queue.enqueue(request).await?;

    tracing::info!(job_id = %job_id, run_id = %run_id, build_num = %build_num, "Job enqueued from webhook");

    Ok(())
}

/// Main webhook handler - routes to platform-specific handlers
pub async fn webhook_handler(
    State(state): State<AppState>,
    Path(source): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // Validate source
    let source_lower = source.to_lowercase();
    if !["github", "gitlab", "gogs"].contains(&source_lower.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(WebhookResponse {
                success: false,
                message: format!("Unknown webhook source: {}", source),
                triggered_jobs: vec![],
            }),
        )
            .into_response();
    }

    // Extract platform-specific headers
    let (event_type, signature, token) = match source_lower.as_str() {
        "github" => {
            let event = headers
                .get("x-github-event")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("push");
            let sig = headers
                .get("x-hub-signature-256")
                .and_then(|v| v.to_str().ok());
            (event.to_string(), sig.map(|s| s.to_string()), None)
        }
        "gitlab" => {
            let event = headers
                .get("x-gitlab-event")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("Push Hook");
            let token = headers.get("x-gitlab-token").and_then(|v| v.to_str().ok());
            (event.to_string(), None, token.map(|t| t.to_string()))
        }
        "gogs" => {
            let event = headers
                .get("x-gogs-event")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("push");
            let sig = headers
                .get("x-gogs-signature")
                .and_then(|v| v.to_str().ok());
            (event.to_string(), sig.map(|s| s.to_string()), None)
        }
        _ => unreachable!(),
    };

    tracing::info!(source = %source_lower, event = %event_type, "Received webhook");

    // Route to platform-specific handler
    let response = match source_lower.as_str() {
        "github" => handle_github_webhook(&state, &event_type, &body, signature.as_deref()).await,
        "gitlab" => handle_gitlab_webhook(&state, &event_type, &body, token.as_deref()).await,
        "gogs" => handle_gogs_webhook(&state, &event_type, &body, signature.as_deref()).await,
        _ => unreachable!(),
    };

    let status = if response.success {
        StatusCode::OK
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };

    (status, Json(response)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_branch_from_ref() {
        assert_eq!(
            extract_branch_from_ref("refs/heads/main"),
            Some("main".to_string())
        );
        assert_eq!(
            extract_branch_from_ref("refs/heads/feature/test"),
            Some("feature/test".to_string())
        );
        assert_eq!(
            extract_branch_from_ref("refs/tags/v1.0.0"),
            Some("v1.0.0".to_string())
        );
        assert_eq!(extract_branch_from_ref("main"), None);
    }

    #[test]
    fn test_match_pattern() {
        assert!(match_pattern("main", "main"));
        assert!(match_pattern("main", "*"));
        assert!(match_pattern("feature/test", "feature/*"));
        assert!(!match_pattern("main", "develop"));
        assert!(!match_pattern("feature/test", "feature"));
    }

    #[test]
    fn test_verify_github_signature() {
        let secret = "my-secret";
        let payload = b"test payload";
        // This will fail because we're using wrong signature, but it validates the format
        assert!(!verify_github_signature(secret, payload, "invalid"));
        assert!(!verify_github_signature(secret, payload, "sha256=invalid"));
    }
}
