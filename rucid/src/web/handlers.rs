//! Web UI handlers
//!
//! Provides HTTP handlers for the Web UI pages.

use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
    Form,
};
use serde::Deserialize;

use crate::AppContext;
use ruci_core::db::{WebhookEvent, WebhookFilter, WebhookSource, WebhookTriggerInfo};

/// Log and unwrap a DB result, returning default on error
async fn db_or_default<T: Default>(result: Result<T, ruci_core::error::Error>, op: &str) -> T {
    match result {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("DB operation '{}' failed: {}", op, e);
            T::default()
        }
    }
}

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub context: Arc<AppContext>,
}

/// Session cookie name
pub const SESSION_COOKIE: &str = "ruci_session";

/// Helper to get session from cookies
fn get_session_from_cookies(
    auth: &ruci_core::auth::AuthService,
    cookies: &HeaderMap,
) -> Option<ruci_core::auth::Session> {
    if let Some(cookie_header) = cookies.get(header::COOKIE) {
        if let Ok(cookie_str) = cookie_header.to_str() {
            for cookie in cookie_str.split(';') {
                let cookie = cookie.trim();
                if let Some(session_id) = cookie.strip_prefix(&format!("{}=", SESSION_COOKIE)) {
                    return auth.get_session(session_id);
                }
            }
        }
    }
    None
}

/// Status badge HTML
fn status_badge(status: &str) -> String {
    let (bg_class, text_class) = match status {
        "SUCCESS" => ("bg-green-900", "text-green-300"),
        "FAILED" => ("bg-red-900", "text-red-300"),
        "RUNNING" => ("bg-yellow-900", "text-yellow-300"),
        "ABORTED" => ("bg-gray-700", "text-gray-300"),
        _ => ("bg-gray-700", "text-gray-300"),
    };
    format!(
        r#"<span class="px-2 py-1 rounded text-xs font-semibold {} {}">{}</span>"#,
        bg_class, text_class, status
    )
}

/// Generate the base HTML template with Tailwind CSS
fn base_html(title: &str, content: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en" class="dark">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{} - Ruci CI</title>
    <script src="https://cdn.tailwindcss.com"></script>
</head>
<body class="bg-gray-900 text-gray-100 min-h-screen">
    <nav class="bg-gray-800 border-b border-gray-700 px-6 py-4">
        <div class="container mx-auto flex justify-between items-center">
            <a href="/" class="text-xl font-bold text-blue-400">Ruci CI</a>
            <div class="flex items-center gap-4">
                <a href="/" class="hover:text-blue-400">Dashboard</a>
                <a href="/ui/jobs" class="hover:text-blue-400">Jobs</a>
                <a href="/ui/runs" class="hover:text-blue-400">Runs</a>
                <a href="/ui/queue" class="hover:text-blue-400">Queue</a>
                <a href="/ui/triggers" class="hover:text-blue-400">Triggers</a>
                <a href="/ui/webhooks" class="hover:text-blue-400">Webhooks</a>
                <form action="/ui/logout" method="post" class="inline">
                    <button type="submit" class="hover:text-red-400">Logout</button>
                </form>
            </div>
        </div>
    </nav>
    <main class="container mx-auto px-6 py-8">
        {}
    </main>
</body>
</html>"#,
        title, content
    )
}

/// Helper function to format bytes
fn bytes_to_human(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Login page handler
pub async fn login_page() -> Html<String> {
    let content = r#"
    <div class="max-w-md mx-auto mt-20">
        <div class="bg-gray-800 rounded-lg p-8 border border-gray-700">
            <h1 class="text-2xl font-bold mb-6 text-center">Sign In to Ruci CI</h1>
            <form action="/ui/login" method="post" class="space-y-4">
                <div>
                    <label class="block text-sm font-medium mb-2">Username</label>
                    <input type="text" name="username" required
                           class="w-full px-4 py-2 rounded bg-gray-700 border border-gray-600 focus:border-blue-500 focus:outline-none">
                </div>
                <div>
                    <label class="block text-sm font-medium mb-2">Password</label>
                    <input type="password" name="password" required
                           class="w-full px-4 py-2 rounded bg-gray-700 border border-gray-600 focus:border-blue-500 focus:outline-none">
                </div>
                <button type="submit"
                        class="w-full bg-blue-600 hover:bg-blue-700 py-2 rounded font-semibold transition">
                    Sign In
                </button>
            </form>
        </div>
    </div>
    "#;
    Html(base_html("Login", content))
}

/// Login form data
#[derive(Deserialize)]
pub struct LoginForm {
    username: String,
    password: String,
}

/// Login handler
pub async fn login_handler(State(state): State<AppState>, Form(form): Form<LoginForm>) -> Response {
    match state
        .context
        .auth
        .authenticate(&form.username, &form.password)
        .await
    {
        Ok(Some(session)) => {
            tracing::info!("User {} logged in", session.username);
            let cookie = format!(
                "{}={}; HttpOnly; Path=/; SameSite=Strict",
                SESSION_COOKIE, session.session_id
            );
            let response = axum::response::Redirect::to("/").into_response();
            let headers = HeaderMap::from_iter([(
                header::SET_COOKIE,
                HeaderValue::from_str(&cookie).unwrap(),
            )]);
            (headers, response).into_response()
        }
        Ok(None) => {
            tracing::warn!("Failed login attempt for user: {}", form.username);
            Html(base_html("Login", r#"
                <div class="max-w-md mx-auto mt-20">
                    <div class="bg-gray-800 rounded-lg p-8 border border-gray-700">
                        <h1 class="text-2xl font-bold mb-6 text-center">Sign In to Ruci CI</h1>
                        <div class="bg-red-900 border border-red-700 text-red-300 px-4 py-3 rounded mb-4">
                            Invalid username or password
                        </div>
                        <form action="/ui/login" method="post" class="space-y-4">
                            <div>
                                <label class="block text-sm font-medium mb-2">Username</label>
                                <input type="text" name="username" required
                                       class="w-full px-4 py-2 rounded bg-gray-700 border border-gray-600 focus:border-blue-500 focus:outline-none">
                            </div>
                            <div>
                                <label class="block text-sm font-medium mb-2">Password</label>
                                <input type="password" name="password" required
                                       class="w-full px-4 py-2 rounded bg-gray-700 border border-gray-600 focus:border-blue-500 focus:outline-none">
                            </div>
                            <button type="submit"
                                    class="w-full bg-blue-600 hover:bg-blue-700 py-2 rounded font-semibold transition">
                                Sign In
                            </button>
                        </form>
                    </div>
                </div>
            "#)).into_response()
        }
        Err(e) => {
            tracing::error!("Login error: {}", e);
            Html(base_html("Login Error", &format!(
                r#"<div class="text-center mt-20"><p class="text-red-400">Login error: {}</p><a href="/ui/login" class="text-blue-400 hover:underline mt-4 inline-block">Try again</a></div>"#,
                e
            ))).into_response()
        }
    }
}

/// Logout handler
pub async fn logout_handler() -> Response {
    let cookie = format!(
        "{}=; HttpOnly; Path=/; Max-Age=0; SameSite=Strict",
        SESSION_COOKIE
    );
    let response = axum::response::Redirect::to("/ui/login").into_response();
    let headers =
        HeaderMap::from_iter([(header::SET_COOKIE, HeaderValue::from_str(&cookie).unwrap())]);
    (headers, response).into_response()
}

/// Dashboard homepage handler
pub async fn dashboard_page(State(state): State<AppState>, cookies: HeaderMap) -> Response {
    if get_session_from_cookies(&state.context.auth, &cookies).is_none() {
        return axum::response::Redirect::to("/ui/login").into_response();
    }

    let jobs = db_or_default(state.context.db.list_jobs().await, "dashboard.list_jobs").await;
    let queued = db_or_default(
        state.context.db.list_runs_by_status("QUEUED").await,
        "dashboard.list_queued",
    )
    .await;
    let running = db_or_default(
        state.context.db.list_runs_by_status("RUNNING").await,
        "dashboard.list_running",
    )
    .await;

    // Recent runs (collect from DB by combining statuses)
    let mut recent_runs: Vec<_> = {
        let success = state
            .context
            .db
            .list_runs_by_status("SUCCESS")
            .await
            .unwrap_or_default();
        let failed = state
            .context
            .db
            .list_runs_by_status("FAILED")
            .await
            .unwrap_or_default();
        let aborted = state
            .context
            .db
            .list_runs_by_status("ABORTED")
            .await
            .unwrap_or_default();
        success
            .into_iter()
            .chain(failed)
            .chain(aborted)
            .collect()
    };
    recent_runs.sort_by(|a, b| b.build_num.cmp(&a.build_num));
    recent_runs.truncate(10);

    let recent_runs_html: String = if recent_runs.is_empty() {
        r#"<p class="text-gray-500 text-sm">No completed runs yet</p>"#.to_string()
    } else {
        recent_runs
            .iter()
            .map(|r| {
                format!(
                    r#"<div class="flex justify-between items-center py-2 border-b border-gray-700">
                        <div>
                            <a href="/ui/runs/{}" class="text-blue-400 hover:underline">{}</a>
                            <span class="text-gray-500 text-sm ml-2">#{}: {}</span>
                        </div>
                        {}
                    </div>"#,
                    r.id,
                    r.job_name,
                    r.build_num,
                    r.id,
                    status_badge(&r.status.to_string())
                )
            })
            .collect()
    };

    let running_html: String = if running.is_empty() {
        r#"<p class="text-gray-500 text-sm">No jobs running</p>"#.to_string()
    } else {
        running
            .iter()
            .map(|r| {
                format!(
                    r#"<div class="flex justify-between items-center py-2 border-b border-gray-700">
                        <div>
                            <a href="/ui/runs/{}" class="text-blue-400 hover:underline">{}</a>
                            <span class="text-gray-500 text-sm ml-2">#{}: {}</span>
                        </div>
                        {}
                    </div>"#,
                    r.id,
                    r.job_name,
                    r.build_num,
                    r.id,
                    status_badge(&r.status.to_string())
                )
            })
            .collect()
    };

    let content = format!(
        r#"<h1 class="text-2xl font-bold mb-6">Dashboard</h1>

        <div class="grid grid-cols-1 md:grid-cols-4 gap-4 mb-8">
            <a href="/ui/jobs" class="bg-gray-800 rounded-lg border border-gray-700 p-6 hover:border-blue-500 transition">
                <div class="text-3xl font-bold text-blue-400">{}</div>
                <div class="text-gray-400 text-sm mt-1">Total Jobs</div>
            </a>
            <a href="/ui/queue" class="bg-gray-800 rounded-lg border border-gray-700 p-6 hover:border-yellow-500 transition">
                <div class="text-3xl font-bold text-yellow-400">{}</div>
                <div class="text-gray-400 text-sm mt-1">Queued</div>
            </a>
            <div class="bg-gray-800 rounded-lg border border-gray-700 p-6">
                <div class="text-3xl font-bold text-green-400">{}</div>
                <div class="text-gray-400 text-sm mt-1">Running</div>
            </div>
            <div class="bg-gray-800 rounded-lg border border-gray-700 p-6">
                <div class="text-3xl font-bold text-gray-400">{}</div>
                <div class="text-gray-400 text-sm mt-1">Queued (Queue)</div>
            </div>
        </div>

        <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
            <div class="bg-gray-800 rounded-lg border border-gray-700 p-6">
                <h2 class="text-lg font-semibold mb-4">Running Now</h2>
                {}
            </div>
            <div class="bg-gray-800 rounded-lg border border-gray-700 p-6">
                <h2 class="text-lg font-semibold mb-4">Recent Runs</h2>
                {}
            </div>
        </div>"#,
        jobs.len(),
        queued.len(),
        running.len(),
        queued.len(),
        running_html,
        recent_runs_html
    );

    Html(base_html("Dashboard", &content)).into_response()
}

/// Jobs list page handler
pub async fn jobs_page(State(state): State<AppState>, cookies: HeaderMap) -> Response {
    if get_session_from_cookies(&state.context.auth, &cookies).is_none() {
        return axum::response::Redirect::to("/ui/login").into_response();
    }

    let jobs = state.context.db.list_jobs().await.unwrap_or_default();

    let rows: String = if jobs.is_empty() {
        r#"<tr><td colspan="4" class="text-center py-8 text-gray-500">No jobs found</td></tr>"#
            .to_string()
    } else {
        jobs.iter()
            .map(|j| {
                format!(
                    r#"<tr class="border-b border-gray-700 hover:bg-gray-800">
                        <td class="py-3 px-4">{}</td>
                        <td class="py-3 px-4">{}</td>
                        <td class="py-3 px-4">{}</td>
                        <td class="py-3 px-4"><a href="/ui/runs?job_id={}" class="text-blue-400 hover:underline">View Runs</a></td>
                    </tr>"#,
                    j.name,
                    j.id,
                    j.submitted_at.format("%Y-%m-%d %H:%M"),
                    j.id
                )
            })
            .collect()
    };

    let content = format!(
        r#"<div class="flex justify-between items-center mb-6">
            <h1 class="text-2xl font-bold">Jobs</h1>
        </div>
        <div class="bg-gray-800 rounded-lg border border-gray-700 overflow-hidden">
            <table class="w-full">
                <thead class="bg-gray-700">
                    <tr>
                        <th class="py-3 px-4 text-left">Name</th>
                        <th class="py-3 px-4 text-left">Job ID</th>
                        <th class="py-3 px-4 text-left">Created</th>
                        <th class="py-3 px-4 text-left">Actions</th>
                    </tr>
                </thead>
                <tbody>{}</tbody>
            </table>
        </div>"#,
        rows
    );

    Html(base_html("Jobs", &content)).into_response()
}

/// Runs list page handler
#[derive(Deserialize)]
pub struct RunsQuery {
    job_id: Option<String>,
}

pub async fn runs_page(
    State(state): State<AppState>,
    Query(query): Query<RunsQuery>,
    cookies: HeaderMap,
) -> Response {
    if get_session_from_cookies(&state.context.auth, &cookies).is_none() {
        return axum::response::Redirect::to("/ui/login").into_response();
    }

    let runs: Vec<_> = {
        let all_queued = state
            .context
            .db
            .list_runs_by_status("QUEUED")
            .await
            .unwrap_or_default();
        let all_running = state
            .context
            .db
            .list_runs_by_status("RUNNING")
            .await
            .unwrap_or_default();
        let all_success = state
            .context
            .db
            .list_runs_by_status("SUCCESS")
            .await
            .unwrap_or_default();
        let all_failed = state
            .context
            .db
            .list_runs_by_status("FAILED")
            .await
            .unwrap_or_default();
        let all_aborted = state
            .context
            .db
            .list_runs_by_status("ABORTED")
            .await
            .unwrap_or_default();

        let mut runs: Vec<_> = all_queued
            .into_iter()
            .chain(all_running)
            .chain(all_success)
            .chain(all_failed)
            .chain(all_aborted)
            .filter(|r| query.job_id.as_ref().map_or(true, |jid| &r.job_id == jid))
            .collect();
        runs.sort_by(|a, b| b.build_num.cmp(&a.build_num));
        runs.truncate(100);
        runs
    };

    let rows: String = if runs.is_empty() {
        r#"<tr><td colspan="7" class="text-center py-8 text-gray-500">No runs found</td></tr>"#
            .to_string()
    } else {
        runs.iter()
            .map(|r| {
                let started = r.started_at.map(|t| t.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_else(|| "-".to_string());
                let duration = if let (Some(start), Some(end)) = (r.started_at, r.finished_at) {
                    let dur = end.signed_duration_since(start);
                    if dur.num_seconds() < 60 {
                        format!("{}s", dur.num_seconds())
                    } else if dur.num_minutes() < 60 {
                        format!("{}m", dur.num_minutes())
                    } else {
                        format!("{}h {}m", dur.num_hours(), dur.num_minutes() % 60)
                    }
                } else if r.started_at.is_some() {
                    "running...".to_string()
                } else {
                    "-".to_string()
                };
                let status_str = r.status.to_string();
                let badge = status_badge(&status_str);

                format!(
                    r#"<tr class="border-b border-gray-700 hover:bg-gray-800">
                        <td class="py-3 px-4 font-mono text-sm">{}</td>
                        <td class="py-3 px-4">{}</td>
                        <td class="py-3 px-4">#{}</td>
                        <td class="py-3 px-4">{}</td>
                        <td class="py-3 px-4">{}</td>
                        <td class="py-3 px-4">{}</td>
                        <td class="py-3 px-4"><a href="/ui/runs/{}" class="text-blue-400 hover:underline">View</a></td>
                    </tr>"#,
                    r.id, r.job_name, r.build_num, badge, started, duration, r.id
                )
            })
            .collect()
    };

    let job_filter = if let Some(ref job_id) = query.job_id {
        format!(
            r#"<p class="text-gray-400 mb-4">Showing runs for job: <span class="text-white">{}</span></p>"#,
            job_id
        )
    } else {
        String::new()
    };

    let content = format!(
        r#"<div class="flex justify-between items-center mb-6">
            <h1 class="text-2xl font-bold">Runs</h1>
        </div>
        {}
        <div class="bg-gray-800 rounded-lg border border-gray-700 overflow-hidden">
            <table class="w-full">
                <thead class="bg-gray-700">
                    <tr>
                        <th class="py-3 px-4 text-left">Run ID</th>
                        <th class="py-3 px-4 text-left">Job</th>
                        <th class="py-3 px-4 text-left">Build</th>
                        <th class="py-3 px-4 text-left">Status</th>
                        <th class="py-3 px-4 text-left">Started</th>
                        <th class="py-3 px-4 text-left">Duration</th>
                        <th class="py-3 px-4 text-left">Actions</th>
                    </tr>
                </thead>
                <tbody>{}</tbody>
            </table>
        </div>"#,
        job_filter, rows
    );

    Html(base_html("Runs", &content)).into_response()
}

/// Run detail page handler
pub async fn run_detail_page(
    State(state): State<AppState>,
    axum::extract::Path(run_id): axum::extract::Path<String>,
    cookies: HeaderMap,
) -> Response {
    if get_session_from_cookies(&state.context.auth, &cookies).is_none() {
        return axum::response::Redirect::to("/ui/login").into_response();
    }

    let run = match state.context.db.get_run(&run_id).await {
        Ok(Some(r)) => r,
        _ => {
            return Html(base_html(
                "Run Not Found",
                r#"
                <div class="text-center py-20">
                    <h1 class="text-2xl font-bold mb-4">Run Not Found</h1>
                    <a href="/ui/runs" class="text-blue-400 hover:underline">Back to Runs</a>
                </div>
            "#,
            ))
            .into_response();
        }
    };

    let status_str = run.status.to_string();
    let badge = status_badge(&status_str);

    let params = state
        .context
        .db
        .get_run_params(&run_id)
        .await
        .unwrap_or_default();
    let params_html = if params.is_empty() {
        String::from("<span class='text-gray-500'>None</span>")
    } else {
        params
            .iter()
            .map(|(k, v)| {
                format!(
                    "<span class='inline-block bg-gray-700 px-2 py-1 rounded text-sm mr-2 mb-1'><span class='text-blue-400'>{}</span>: {}</span>",
                    k, v
                )
            })
            .collect()
    };

    let artifacts = state
        .context
        .db
        .list_artifacts(&run_id)
        .await
        .unwrap_or_default();
    let artifacts_html = if artifacts.is_empty() {
        String::from("<span class='text-gray-500'>No artifacts</span>")
    } else {
        artifacts
            .iter()
            .map(|a| {
                format!(
                    "<div class='bg-gray-700 px-3 py-2 rounded flex justify-between items-center'><span>{}</span><span class='text-gray-400 text-sm'>{}</span></div>",
                    a.name,
                    bytes_to_human(a.size)
                )
            })
            .collect::<Vec<_>>()
            .join("")
    };

    let duration = if let (Some(start), Some(end)) = (run.started_at, run.finished_at) {
        let dur = end.signed_duration_since(start);
        if dur.num_seconds() < 60 {
            format!("{} seconds", dur.num_seconds())
        } else if dur.num_minutes() < 60 {
            format!("{} minutes", dur.num_minutes())
        } else {
            format!(
                "{} hours {} minutes",
                dur.num_hours(),
                dur.num_minutes() % 60
            )
        }
    } else if run.started_at.is_some() {
        "Running...".to_string()
    } else {
        "-".to_string()
    };

    let started_str = run
        .started_at
        .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "-".to_string());
    let exit_code_str = run
        .exit_code
        .map(|c| c.to_string())
        .unwrap_or_else(|| "-".to_string());

    let content = format!(
        r#"<div class="mb-6">
            <a href="/ui/runs" class="text-blue-400 hover:underline">Back to Runs</a>
        </div>
        <div class="grid grid-cols-1 lg:grid-cols-3 gap-6">
            <div class="lg:col-span-2 space-y-6">
                <div class="bg-gray-800 rounded-lg border border-gray-700 p-6">
                    <div class="flex justify-between items-start mb-4">
                        <div>
                            <h1 class="text-2xl font-bold mb-1">Run {}</h1>
                            <p class="text-gray-400">Job: {} | Build #{}</p>
                        </div>
                        {}
                    </div>
                    <div class="grid grid-cols-2 gap-4 text-sm">
                        <div><span class="text-gray-400">Started:</span> <span class="ml-2">{}</span></div>
                        <div><span class="text-gray-400">Duration:</span> <span class="ml-2">{}</span></div>
                        <div><span class="text-gray-400">Exit Code:</span> <span class="ml-2">{}</span></div>
                    </div>
                </div>
                <div class="bg-gray-800 rounded-lg border border-gray-700 p-6">
                    <h2 class="text-lg font-semibold mb-4">Log Output</h2>
                    <div id="log-container" class="bg-gray-900 rounded p-4 h-96 overflow-y-auto font-mono text-sm">
                        <p class="text-gray-500">Loading logs...</p>
                    </div>
                </div>
            </div>
            <div class="space-y-6">
                <div class="bg-gray-800 rounded-lg border border-gray-700 p-6">
                    <h2 class="text-lg font-semibold mb-4">Parameters</h2>
                    <div class="flex flex-wrap">{}</div>
                </div>
                <div class="bg-gray-800 rounded-lg border border-gray-700 p-6">
                    <h2 class="text-lg font-semibold mb-4">Artifacts</h2>
                    <div class="space-y-2">{}</div>
                </div>
            </div>
        </div>
        <script>
            const runId = "{}";
            const logContainer = document.getElementById('log-container');
            const eventSource = new EventSource('/stream/logs/' + runId);
            eventSource.onmessage = function(event) {{
                const data = JSON.parse(event.data);
                if (data.type === 'log') {{
                    logContainer.textContent = data.content;
                    logContainer.scrollTop = logContainer.scrollHeight;
                }} else if (data.type === 'error') {{
                    logContainer.innerHTML = '<p class="text-red-400">' + data.message + '</p>';
                }} else if (data.type === 'end') {{
                    eventSource.close();
                }}
            }};
            eventSource.onerror = function() {{
                fetch('/api/runs/' + runId + '/log')
                    .then(r => r.text())
                    .then(text => {{
                        logContainer.textContent = text || 'No log available';
                    }})
                    .catch(() => {{
                        logContainer.innerHTML = '<p class="text-gray-500">Log not available</p>';
                    }});
                eventSource.close();
            }};
        </script>"#,
        run_id,
        run.job_name,
        run.build_num,
        badge,
        started_str,
        duration,
        exit_code_str,
        params_html,
        artifacts_html,
        run_id
    );

    Html(base_html(&format!("Run {}", run_id), &content)).into_response()
}

/// Queue status page handler
pub async fn queue_page(State(state): State<AppState>, cookies: HeaderMap) -> Response {
    if get_session_from_cookies(&state.context.auth, &cookies).is_none() {
        return axum::response::Redirect::to("/ui/login").into_response();
    }

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

    let queued_rows: String = if queued.is_empty() {
        r#"<p class="text-gray-500 py-4">No queued runs</p>"#.to_string()
    } else {
        queued
            .iter()
            .map(|r| {
                format!(
                    "<tr class='border-b border-gray-700'><td class='py-3 px-4'>{}</td><td class='py-3 px-4'>{}</td><td class='py-3 px-4'>#{}</td></tr>",
                    r.id, r.job_name, r.build_num
                )
            })
            .collect()
    };

    let running_rows: String = if running.is_empty() {
        r#"<p class="text-gray-500 py-4">No running jobs</p>"#.to_string()
    } else {
        running
            .iter()
            .map(|r| {
                let started = r.started_at.map(|t| t.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_else(|| "-".to_string());
                format!(
                    "<tr class='border-b border-gray-700'><td class='py-3 px-4'>{}</td><td class='py-3 px-4'>{}</td><td class='py-3 px-4'>#{}</td><td class='py-3 px-4'>{}</td><td class='py-3 px-4'><a href='/ui/runs/{}' class='text-blue-400 hover:underline'>View</a></td></tr>",
                    r.id, r.job_name, r.build_num, started, r.id
                )
            })
            .collect()
    };

    let content = format!(
        r#"<h1 class="text-2xl font-bold mb-6">Queue Status</h1>
        <div class="grid grid-cols-1 md:grid-cols-3 gap-6 mb-8">
            <div class="bg-gray-800 rounded-lg border border-gray-700 p-6">
                <div class="text-3xl font-bold text-blue-400">{}</div>
                <div class="text-gray-400">Queued</div>
            </div>
            <div class="bg-gray-800 rounded-lg border border-gray-700 p-6">
                <div class="text-3xl font-bold text-yellow-400">{}</div>
                <div class="text-gray-400">Running</div>
            </div>
            <div class="bg-gray-800 rounded-lg border border-gray-700 p-6">
                <div class="text-3xl font-bold text-green-400">{}</div>
                <div class="text-gray-400">Active Sessions</div>
            </div>
        </div>
        <div class="grid grid-cols-1 lg:grid-cols-2 gap-6">
            <div class="bg-gray-800 rounded-lg border border-gray-700">
                <div class="px-6 py-4 border-b border-gray-700"><h2 class="font-semibold">Queued Jobs</h2></div>
                <div class="p-6">{}</div>
            </div>
            <div class="bg-gray-800 rounded-lg border border-gray-700">
                <div class="px-6 py-4 border-b border-gray-700"><h2 class="font-semibold">Running Jobs</h2></div>
                <div class="p-6">{}</div>
            </div>
        </div>"#,
        queued.len(),
        running.len(),
        state.context.auth.active_sessions_count(),
        if queued.is_empty() {
            String::from("<p class='text-gray-500 py-4'>No queued runs</p>")
        } else {
            format!(
                "<table class='w-full'><tbody>{}</tbody></table>",
                queued_rows
            )
        },
        if running.is_empty() {
            String::from("<p class='text-gray-500 py-4'>No running jobs</p>")
        } else {
            format!("<table class='w-full'><thead><tr><th class='text-left text-sm text-gray-400 pb-2'>Run ID</th><th class='text-left text-sm text-gray-400 pb-2'>Job</th><th class='text-left text-sm text-gray-400 pb-2'>Build</th><th class='text-left text-sm text-gray-400 pb-2'>Started</th><th></th></tr></thead><tbody>{}</tbody></table>", running_rows)
        }
    );

    Html(base_html("Queue", &content)).into_response()
}

/// Sanitize a path component to prevent path traversal
fn sanitize_path_component(s: &str) -> bool {
    !s.is_empty()
        && !s.contains('/')
        && !s.contains('\\')
        && !s.contains("..")
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// SSE log streaming handler
pub async fn log_stream_handler(
    State(state): State<AppState>,
    axum::extract::Path(run_id): axum::extract::Path<String>,
) -> Response {
    if !sanitize_path_component(&run_id) {
        let body =
            Body::from("data: {\"type\":\"error\",\"message\":\"Invalid run ID\"}\n\n");
        return axum::http::Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .body(body)
            .unwrap();
    }

    let log_path = match state.context.db.get_run(&run_id).await {
        Ok(Some(run)) => format!(
            "{}/{}/{}/output.log",
            state.context.config.paths.run_dir, run.job_id, run_id
        ),
        Ok(None) => {
            let body =
                Body::from("data: {\"type\":\"error\",\"message\":\"Run not found\"}\n\n");
            return axum::http::Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .header(header::CACHE_CONTROL, "no-cache")
                .body(body)
                .unwrap();
        }
        Err(e) => {
            let body = Body::from(format!(
                "data: {{\"type\":\"error\",\"message\":\"Database error: {}\"}}\n\n",
                e
            ));
            return axum::http::Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .header(header::CACHE_CONTROL, "no-cache")
                .body(body)
                .unwrap();
        }
    };

    if !std::path::Path::new(&log_path).exists() {
        let body = Body::from("data: {\"type\":\"error\",\"message\":\"Log file not found\"}\n\n");
        return axum::http::Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .body(body)
            .unwrap();
    }

    match tokio::fs::read_to_string(&log_path).await {
        Ok(content) => {
            let sse_data = serde_json::json!({
                "type": "log",
                "content": content
            });
            let body = Body::from(format!("data: {}\n\n", sse_data));
            axum::http::Response::builder()
                .header(header::CONTENT_TYPE, "text/event-stream")
                .header(header::CACHE_CONTROL, "no-cache")
                .body(body)
                .unwrap()
        }
        Err(e) => {
            let sse_data = serde_json::json!({
                "type": "error",
                "message": format!("Failed to read log: {}", e)
            });
            let body = Body::from(format!("data: {}\n\n", sse_data));
            axum::http::Response::builder()
                .header(header::CONTENT_TYPE, "text/event-stream")
                .header(header::CACHE_CONTROL, "no-cache")
                .body(body)
                .unwrap()
        }
    }
}

/// Triggers list page handler
pub async fn triggers_page(State(state): State<AppState>, cookies: HeaderMap) -> Response {
    if get_session_from_cookies(&state.context.auth, &cookies).is_none() {
        return axum::response::Redirect::to("/ui/login").into_response();
    }

    let triggers = state.context.db.list_triggers().await.unwrap_or_default();
    let config_triggers = &state.context.config.triggers;

    let rows: String = if triggers.is_empty() && config_triggers.is_empty() {
        r#"<tr><td colspan="5" class="text-center py-8 text-gray-500">No triggers configured</td></tr>"#.to_string()
    } else {
        let mut all_triggers: Vec<(String, String, String, bool)> = Vec::new();

        // Add triggers from config
        for t in config_triggers {
            let enabled = triggers
                .iter()
                .find(|db_t| db_t.name == t.name)
                .map(|db_t| db_t.enabled)
                .unwrap_or(t.enabled);
            all_triggers.push((t.name.clone(), t.cron.clone(), t.job.clone(), enabled));
        }

        // Add triggers from DB that are not in config (might have been added dynamically)
        for t in &triggers {
            if !all_triggers.iter().any(|(name, _, _, _)| name == &t.name) {
                all_triggers.push((t.name.clone(), t.cron.clone(), t.job_id.clone(), t.enabled));
            }
        }

        all_triggers.iter()
            .map(|(name, cron, job, enabled)| {
                let status_badge = if *enabled {
                    r#"<span class="px-2 py-1 rounded text-xs font-semibold bg-green-900 text-green-300">Enabled</span>"#
                } else {
                    r#"<span class="px-2 py-1 rounded text-xs font-semibold bg-gray-700 text-gray-300">Disabled</span>"#
                };
                let toggle_url = if *enabled {
                    format!("/api/triggers/{}/disable", name)
                } else {
                    format!("/api/triggers/{}/enable", name)
                };
                let toggle_text = if *enabled { "Disable" } else { "Enable" };

                format!(
                    r#"<tr class="border-b border-gray-700 hover:bg-gray-800">
                        <td class="py-3 px-4">{}</td>
                        <td class="py-3 px-4 font-mono text-sm">{}</td>
                        <td class="py-3 px-4">{}</td>
                        <td class="py-3 px-4">{}</td>
                        <td class="py-3 px-4">
                            <form action="{}" method="post" class="inline">
                                <button type="submit" class="text-blue-400 hover:underline">{}</button>
                            </form>
                        </td>
                    </tr>"#,
                    name, cron, job, status_badge, toggle_url, toggle_text
                )
            })
            .collect()
    };

    let content = format!(
        r#"<div class="flex justify-between items-center mb-6">
            <h1 class="text-2xl font-bold">Scheduled Triggers</h1>
        </div>
        <div class="bg-gray-800 rounded-lg border border-gray-700 overflow-hidden">
            <table class="w-full">
                <thead class="bg-gray-700">
                    <tr>
                        <th class="py-3 px-4 text-left">Name</th>
                        <th class="py-3 px-4 text-left">Cron Expression</th>
                        <th class="py-3 px-4 text-left">Job ID</th>
                        <th class="py-3 px-4 text-left">Status</th>
                        <th class="py-3 px-4 text-left">Actions</th>
                    </tr>
                </thead>
                <tbody>{}</tbody>
            </table>
        </div>
        <div class="mt-4 text-sm text-gray-400">
            <p>Cron format: second minute hour day month weekday</p>
            <p>Examples: "0 0 * * * *" (every hour), "0 0 0 * * *" (daily at midnight), "0 */5 * * * *" (every 5 minutes)</p>
        </div>"#,
        rows
    );

    Html(base_html("Triggers", &content)).into_response()
}

/// Toggle trigger enabled/disabled API endpoint
pub async fn trigger_enable_handler(
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
    cookies: HeaderMap,
) -> Response {
    if get_session_from_cookies(&state.context.auth, &cookies).is_none() {
        return Html(base_html(
            "Unauthorized",
            r#"<p class="text-red-400">Unauthorized</p>"#,
        ))
        .into_response();
    }

    match state.context.db.set_trigger_enabled(&name, true).await {
        Ok(_) => {
            tracing::info!(trigger = %name, "Trigger enabled");
            axum::response::Redirect::to("/ui/triggers").into_response()
        }
        Err(e) => {
            tracing::error!(trigger = %name, error = %e, "Failed to enable trigger");
            Html(base_html("Error", &format!(r#"<p class="text-red-400">Failed to enable trigger: {}</p><a href="/ui/triggers" class="text-blue-400 hover:underline">Back</a>"#, e))).into_response()
        }
    }
}

pub async fn trigger_disable_handler(
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
    cookies: HeaderMap,
) -> Response {
    if get_session_from_cookies(&state.context.auth, &cookies).is_none() {
        return Html(base_html(
            "Unauthorized",
            r#"<p class="text-red-400">Unauthorized</p>"#,
        ))
        .into_response();
    }

    match state.context.db.set_trigger_enabled(&name, false).await {
        Ok(_) => {
            tracing::info!(trigger = %name, "Trigger disabled");
            axum::response::Redirect::to("/ui/triggers").into_response()
        }
        Err(e) => {
            tracing::error!(trigger = %name, error = %e, "Failed to disable trigger");
            Html(base_html("Error", &format!(r#"<p class="text-red-400">Failed to disable trigger: {}</p><a href="/ui/triggers" class="text-blue-400 hover:underline">Back</a>"#, e))).into_response()
        }
    }
}

/// API endpoint to list triggers (JSON)
pub async fn api_triggers_handler(State(state): State<AppState>) -> impl IntoResponse {
    let triggers = state.context.db.list_triggers().await.unwrap_or_default();
    let config_triggers = &state.context.config.triggers;

    #[derive(serde::Serialize)]
    struct TriggerResponse {
        name: String,
        cron: String,
        job_id: String,
        enabled: bool,
    }

    let mut all_triggers: Vec<TriggerResponse> = Vec::new();

    // Add triggers from config
    for t in config_triggers {
        let enabled = triggers
            .iter()
            .find(|db_t| db_t.name == t.name)
            .map(|db_t| db_t.enabled)
            .unwrap_or(t.enabled);
        all_triggers.push(TriggerResponse {
            name: t.name.clone(),
            cron: t.cron.clone(),
            job_id: t.job.clone(),
            enabled,
        });
    }

    // Add triggers from DB not in config
    for t in &triggers {
        if !all_triggers.iter().any(|tr| tr.name == t.name) {
            all_triggers.push(TriggerResponse {
                name: t.name.clone(),
                cron: t.cron.clone(),
                job_id: t.job_id.clone(),
                enabled: t.enabled,
            });
        }
    }

    axum::Json(serde_json::json!({ "triggers": all_triggers }))
}

/// Webhooks list page handler
pub async fn webhooks_page(State(state): State<AppState>, cookies: HeaderMap) -> Response {
    if get_session_from_cookies(&state.context.auth, &cookies).is_none() {
        return axum::response::Redirect::to("/ui/login").into_response();
    }

    let webhooks = state
        .context
        .db
        .list_webhook_triggers()
        .await
        .unwrap_or_default();
    let jobs = state.context.db.list_jobs().await.unwrap_or_default();

    let rows: String = if webhooks.is_empty() {
        r#"<tr><td colspan="6" class="text-center py-8 text-gray-500">No webhook triggers configured</td></tr>"#.to_string()
    } else {
        webhooks.iter()
            .map(|w| {
                let status_badge = if w.enabled {
                    r#"<span class="px-2 py-1 rounded text-xs font-semibold bg-green-900 text-green-300">Enabled</span>"#
                } else {
                    r#"<span class="px-2 py-1 rounded text-xs font-semibold bg-gray-700 text-gray-300">Disabled</span>"#
                };
                let toggle_url = if w.enabled {
                    format!("/api/webhooks/{}/disable", urlencoding::encode(&w.name))
                } else {
                    format!("/api/webhooks/{}/enable", urlencoding::encode(&w.name))
                };
                let toggle_text = if w.enabled { "Disable" } else { "Enable" };
                let source_badge = match w.source {
                    WebhookSource::Github => r#"<span class="px-2 py-1 rounded text-xs font-semibold bg-gray-700 text-white">GitHub</span>"#,
                    WebhookSource::Gitlab => r#"<span class="px-2 py-1 rounded text-xs font-semibold bg-orange-700 text-white">GitLab</span>"#,
                    WebhookSource::Gogs => r#"<span class="px-2 py-1 rounded text-xs font-semibold bg-green-700 text-white">Gogs</span>"#,
                };
                let events_str = w.filter.events.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(", ");

                format!(
                    r#"<tr class="border-b border-gray-700 hover:bg-gray-800">
                        <td class="py-3 px-4">{}</td>
                        <td class="py-3 px-4">{}</td>
                        <td class="py-3 px-4">{}</td>
                        <td class="py-3 px-4">{}</td>
                        <td class="py-3 px-4 font-mono text-sm">{}</td>
                        <td class="py-3 px-4">
                            <form action="{}" method="post" class="inline mr-2">
                                <button type="submit" class="text-blue-400 hover:underline">{}</button>
                            </form>
                            <form action="/api/webhooks/{}/delete" method="post" class="inline">
                                <button type="submit" class="text-red-400 hover:underline">Delete</button>
                            </form>
                        </td>
                    </tr>"#,
                    w.name, source_badge, w.job_id, events_str, status_badge, toggle_url, toggle_text, urlencoding::encode(&w.name)
                )
            })
            .collect()
    };

    let job_options: String = jobs
        .iter()
        .map(|j| format!(r#"<option value="{}">{}</option>"#, j.id, j.name))
        .collect();

    let content = format!(
        r#"<div class="flex justify-between items-center mb-6">
            <h1 class="text-2xl font-bold">Webhook Triggers</h1>
            <button onclick="showAddForm()" class="bg-blue-600 hover:bg-blue-700 px-4 py-2 rounded font-semibold">Add Webhook</button>
        </div>

        <div id="add-form" class="hidden mb-6 bg-gray-800 rounded-lg border border-gray-700 p-6">
            <h2 class="text-lg font-semibold mb-4">Add Webhook Trigger</h2>
            <form action="/api/webhooks" method="post" class="space-y-4">
                <div class="grid grid-cols-2 gap-4">
                    <div>
                        <label class="block text-sm font-medium mb-2">Name</label>
                        <input type="text" name="name" required class="w-full px-4 py-2 rounded bg-gray-700 border border-gray-600 focus:border-blue-500 focus:outline-none">
                    </div>
                    <div>
                        <label class="block text-sm font-medium mb-2">Job</label>
                        <select name="job_id" required class="w-full px-4 py-2 rounded bg-gray-700 border border-gray-600 focus:border-blue-500 focus:outline-none">
                            <option value="">Select a job...</option>
                            {}
                        </select>
                    </div>
                    <div>
                        <label class="block text-sm font-medium mb-2">Source</label>
                        <select name="source" required class="w-full px-4 py-2 rounded bg-gray-700 border border-gray-600 focus:border-blue-500 focus:outline-none">
                            <option value="github">GitHub</option>
                            <option value="gitlab">GitLab</option>
                            <option value="gogs">Gogs</option>
                        </select>
                    </div>
                    <div>
                        <label class="block text-sm font-medium mb-2">Secret (for signature verification)</label>
                        <input type="text" name="secret" placeholder="Leave empty to skip verification" class="w-full px-4 py-2 rounded bg-gray-700 border border-gray-600 focus:border-blue-500 focus:outline-none">
                    </div>
                    <div>
                        <label class="block text-sm font-medium mb-2">Repository Pattern</label>
                        <input type="text" name="repository" placeholder="e.g., owner/repo or *" class="w-full px-4 py-2 rounded bg-gray-700 border border-gray-600 focus:border-blue-500 focus:outline-none">
                    </div>
                    <div>
                        <label class="block text-sm font-medium mb-2">Branch Patterns (comma-separated)</label>
                        <input type="text" name="branches" placeholder="e.g., main, feature/*" class="w-full px-4 py-2 rounded bg-gray-700 border border-gray-600 focus:border-blue-500 focus:outline-none">
                    </div>
                </div>
                <div>
                    <label class="block text-sm font-medium mb-2">Events</label>
                    <div class="flex gap-4">
                        <label class="inline-flex items-center">
                            <input type="checkbox" name="events" value="push" checked class="mr-2">
                            Push
                        </label>
                        <label class="inline-flex items-center">
                            <input type="checkbox" name="events" value="tag_push" class="mr-2">
                            Tag Push
                        </label>
                        <label class="inline-flex items-center">
                            <input type="checkbox" name="events" value="pull_request" class="mr-2">
                            Pull Request
                        </label>
                        <label class="inline-flex items-center">
                            <input type="checkbox" name="events" value="merge_request" class="mr-2">
                            Merge Request
                        </label>
                    </div>
                </div>
                <div class="flex gap-4">
                    <button type="submit" class="bg-blue-600 hover:bg-blue-700 px-6 py-2 rounded font-semibold">Create</button>
                    <button type="button" onclick="hideAddForm()" class="bg-gray-600 hover:bg-gray-700 px-6 py-2 rounded font-semibold">Cancel</button>
                </div>
            </form>
        </div>

        <div class="bg-gray-800 rounded-lg border border-gray-700 overflow-hidden">
            <table class="w-full">
                <thead class="bg-gray-700">
                    <tr>
                        <th class="py-3 px-4 text-left">Name</th>
                        <th class="py-3 px-4 text-left">Source</th>
                        <th class="py-3 px-4 text-left">Job</th>
                        <th class="py-3 px-4 text-left">Events</th>
                        <th class="py-3 px-4 text-left">Status</th>
                        <th class="py-3 px-4 text-left">Actions</th>
                    </tr>
                </thead>
                <tbody>{}</tbody>
            </table>
        </div>
        <div class="mt-4 text-sm text-gray-400">
            <p>Webhook URL: <code class="bg-gray-800 px-2 py-1 rounded">/api/webhooks/:source</code> where :source is github, gitlab, or gogs</p>
        </div>
        <script>
            function showAddForm() {{
                document.getElementById('add-form').classList.remove('hidden');
            }}
            function hideAddForm() {{
                document.getElementById('add-form').classList.add('hidden');
            }}
        </script>"#,
        job_options, rows
    );

    Html(base_html("Webhooks", &content)).into_response()
}

/// Form data for creating a webhook
#[derive(Deserialize)]
pub struct WebhookForm {
    name: String,
    job_id: String,
    source: String,
    secret: String,
    repository: Option<String>,
    branches: Option<String>,
    events: Vec<String>,
}

/// Create a new webhook trigger
pub async fn webhook_create_handler(
    State(state): State<AppState>,
    cookies: HeaderMap,
    Form(form): Form<WebhookForm>,
) -> Response {
    if get_session_from_cookies(&state.context.auth, &cookies).is_none() {
        return Html(base_html(
            "Unauthorized",
            r#"<p class="text-red-400">Unauthorized</p>"#,
        ))
        .into_response();
    }

    let source = match form.source.as_str() {
        "github" => WebhookSource::Github,
        "gitlab" => WebhookSource::Gitlab,
        "gogs" => WebhookSource::Gogs,
        _ => {
            return Html(base_html("Error", &format!(r#"<p class="text-red-400">Invalid source: {}</p><a href="/ui/webhooks" class="text-blue-400 hover:underline">Back</a>"#, form.source))).into_response();
        }
    };

    let events: Vec<WebhookEvent> = form
        .events
        .iter()
        .filter_map(|e| match e.as_str() {
            "push" => Some(WebhookEvent::Push),
            "tag_push" => Some(WebhookEvent::TagPush),
            "pull_request" => Some(WebhookEvent::PullRequest),
            "merge_request" => Some(WebhookEvent::MergeRequest),
            _ => None,
        })
        .collect();

    let filter = WebhookFilter {
        repository: form.repository.filter(|s| !s.is_empty()),
        branches: form
            .branches
            .filter(|s| !s.is_empty())
            .map(|s| s.split(',').map(|b| b.trim().to_string()).collect())
            .unwrap_or_default(),
        events,
    };

    let webhook = WebhookTriggerInfo {
        name: form.name.clone(),
        job_id: form.job_id.clone(),
        enabled: true,
        secret: form.secret.clone(),
        source,
        filter,
        credential_id: None,
    };

    match state.context.db.upsert_webhook_trigger(&webhook).await {
        Ok(_) => {
            tracing::info!(webhook = %form.name, "Webhook created");
            axum::response::Redirect::to("/ui/webhooks").into_response()
        }
        Err(e) => {
            tracing::error!(webhook = %form.name, error = %e, "Failed to create webhook");
            Html(base_html("Error", &format!(r#"<p class="text-red-400">Failed to create webhook: {}</p><a href="/ui/webhooks" class="text-blue-400 hover:underline">Back</a>"#, e))).into_response()
        }
    }
}

/// Enable a webhook trigger
pub async fn webhook_enable_handler(
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
    cookies: HeaderMap,
) -> Response {
    if get_session_from_cookies(&state.context.auth, &cookies).is_none() {
        return Html(base_html(
            "Unauthorized",
            r#"<p class="text-red-400">Unauthorized</p>"#,
        ))
        .into_response();
    }

    let name = urlencoding::decode(&name).unwrap_or_default().to_string();

    match state
        .context
        .db
        .set_webhook_trigger_enabled(&name, true)
        .await
    {
        Ok(_) => {
            tracing::info!(webhook = %name, "Webhook enabled");
            axum::response::Redirect::to("/ui/webhooks").into_response()
        }
        Err(e) => {
            tracing::error!(webhook = %name, error = %e, "Failed to enable webhook");
            Html(base_html("Error", &format!(r#"<p class="text-red-400">Failed to enable webhook: {}</p><a href="/ui/webhooks" class="text-blue-400 hover:underline">Back</a>"#, e))).into_response()
        }
    }
}

/// Disable a webhook trigger
pub async fn webhook_disable_handler(
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
    cookies: HeaderMap,
) -> Response {
    if get_session_from_cookies(&state.context.auth, &cookies).is_none() {
        return Html(base_html(
            "Unauthorized",
            r#"<p class="text-red-400">Unauthorized</p>"#,
        ))
        .into_response();
    }

    let name = urlencoding::decode(&name).unwrap_or_default().to_string();

    match state
        .context
        .db
        .set_webhook_trigger_enabled(&name, false)
        .await
    {
        Ok(_) => {
            tracing::info!(webhook = %name, "Webhook disabled");
            axum::response::Redirect::to("/ui/webhooks").into_response()
        }
        Err(e) => {
            tracing::error!(webhook = %name, error = %e, "Failed to disable webhook");
            Html(base_html("Error", &format!(r#"<p class="text-red-400">Failed to disable webhook: {}</p><a href="/ui/webhooks" class="text-blue-400 hover:underline">Back</a>"#, e))).into_response()
        }
    }
}

/// Delete a webhook trigger
pub async fn webhook_delete_handler(
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
    cookies: HeaderMap,
) -> Response {
    if get_session_from_cookies(&state.context.auth, &cookies).is_none() {
        return Html(base_html(
            "Unauthorized",
            r#"<p class="text-red-400">Unauthorized</p>"#,
        ))
        .into_response();
    }

    let name = urlencoding::decode(&name).unwrap_or_default().to_string();

    match state.context.db.delete_webhook_trigger(&name).await {
        Ok(_) => {
            tracing::info!(webhook = %name, "Webhook deleted");
            axum::response::Redirect::to("/ui/webhooks").into_response()
        }
        Err(e) => {
            tracing::error!(webhook = %name, error = %e, "Failed to delete webhook");
            Html(base_html("Error", &format!(r#"<p class="text-red-400">Failed to delete webhook: {}</p><a href="/ui/webhooks" class="text-blue-400 hover:underline">Back</a>"#, e))).into_response()
        }
    }
}
