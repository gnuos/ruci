# Ruci CI API Documentation

REST API endpoints for the Ruci CI daemon.

## Base URL

```
http://localhost:8080
```

## Authentication

The Web UI uses session-based authentication. API endpoints for webhook receivers do not require authentication (they use signature/token verification).

## Endpoints

### Health & Status

#### GET /health

Health check endpoint.

**Response:**
```json
{
  "status": "healthy",
  "version": "0.1.0",
  "checks": {
    "database": {
      "status": "connected"
    },
    "queue": {
      "status": "running",
      "pending_jobs": 0
    }
  }
}
```

#### GET /api/status

Get daemon status.

**Response:**
```json
{
  "status": "running",
  "version": "0.1.0"
}
```

#### GET /metrics

Prometheus metrics endpoint.

**Response:**
Plain text Prometheus format.

---

### Jobs

#### GET /api/jobs

List all jobs.

**Response:**
```json
{
  "jobs": [
    {
      "id": "abc123...",
      "name": "hello-world",
      "original_name": ".ruci.yml",
      "submitted_at": "2024-01-01T00:00:00Z"
    }
  ]
}
```

---

### Runs

#### GET /api/runs

List queued and running runs.

**Response:**
```json
{
  "queued": [
    {
      "id": "run-uuid",
      "job_id": "abc123...",
      "job_name": "hello-world",
      "build_num": 1,
      "status": "QUEUED",
      "started_at": null,
      "finished_at": null,
      "exit_code": null
    }
  ],
  "running": [
    {
      "id": "run-uuid",
      "job_id": "abc123...",
      "job_name": "hello-world",
      "build_num": 2,
      "status": "RUNNING",
      "started_at": "2024-01-01T00:00:00Z",
      "finished_at": null,
      "exit_code": null
    }
  ]
}
```

#### GET /ui/runs/:run_id

Get run detail page (HTML).

---

### Triggers

#### GET /api/triggers

List all scheduled triggers.

**Response:** HTML page with trigger management UI.

#### POST /api/triggers/:name/enable

Enable a trigger.

#### POST /api/triggers/:name/disable

Disable a trigger.

---

### Webhooks

#### POST /api/webhooks/:source

Receive webhook from VCS platform.

**Path Parameters:**
- `source`: One of `github`, `gitlab`, `gogs`

**Headers (GitHub):**
- `X-Hub-Signature-256`: HMAC-SHA256 signature
- `X-GitHub-Event`: Event type (push, pull_request)

**Headers (GitLab):**
- `X-Gitlab-Token`: Secret token
- `X-Gitlab-Event`: Event type

**Headers (Gogs):**
- `X-Gogs-Signature`: HMAC signature
- `X-Gogs-Event`: Event type

**Request Body:** VCS platform-specific payload (JSON)

**Response:**
```json
{
  "success": true,
  "message": "Processed 1 webhook(s)",
  "triggered_jobs": ["job-id-1"]
}
```

#### GET /ui/webhooks

Webhook management page (HTML).

#### POST /api/webhooks

Create a new webhook trigger.

**Request Body:**
```json
{
  "name": "my-webhook",
  "source": "github",
  "job_id": "abc123...",
  "secret": "my-secret",
  "filter": {
    "repository": "owner/repo",
    "branches": ["main", "develop"],
    "events": ["push", "pull_request"]
  },
  "enabled": true
}
```

#### POST /api/webhooks/:name/enable

Enable a webhook.

#### POST /api/webhooks/:name/disable

Disable a webhook.

#### POST /api/webhooks/:name/delete

Delete a webhook.

---

### Log Streaming

#### GET /stream/logs/:run_id

SSE stream for real-time job logs.

**Path Parameters:**
- `run_id`: The run ID to stream logs for

**Response:** Server-Sent Events stream

```
event: log
data: {"line": "Building project...", "timestamp": "2024-01-01T00:00:00Z"}

event: log
data: {"line": "Running tests...", "timestamp": "2024-01-01T00:00:01Z"}

event: exit
data: {"code": 0}
```

---

### Web UI Pages

| Endpoint | Description |
|----------|-------------|
| `GET /ui/login` | Login page |
| `POST /ui/login` | Login submission |
| `POST /ui/logout` | Logout |
| `GET /ui/jobs` | Jobs list page |
| `GET /ui/runs` | Runs list page |
| `GET /ui/runs/:run_id` | Run detail page |
| `GET /ui/queue` | Queue status page |
| `GET /ui/triggers` | Trigger management page |
| `GET /ui/webhooks` | Webhook management page |

---

## Webhook Payload Formats

### GitHub Push

```json
{
  "ref": "refs/heads/main",
  "after": "abc123...",
  "repository": {
    "name": "repo",
    "full_name": "owner/repo",
    "clone_url": "https://github.com/owner/repo.git"
  },
  "pusher": {
    "username": "user"
  }
}
```

### GitHub Pull Request

```json
{
  "action": "opened",
  "number": 123,
  "pull_request": {
    "title": "Feature PR",
    "head": {
      "ref": "feature-branch",
      "sha": "abc123..."
    },
    "base": {
      "ref": "main"
    }
  },
  "repository": {
    "full_name": "owner/repo"
  }
}
```

### GitLab Push

```json
{
  "object_kind": "push",
  "event_name": "push",
  "ref": "refs/heads/main",
  "checkout_sha": "abc123...",
  "user_name": "user",
  "project": {
    "name": "repo",
    "path_with_namespace": "owner/repo",
    "git_http_url": "https://gitlab.com/owner/repo.git"
  }
}
```

### GitLab Merge Request

```json
{
  "object_kind": "merge_request",
  "event_type": "merge_request",
  "user": {
    "username": "user"
  },
  "project": {
    "path_with_namespace": "owner/repo",
    "git_http_url": "https://gitlab.com/owner/repo.git"
  },
  "object_attributes": {
    "id": 123,
    "title": "MR Title",
    "source_branch": "feature",
    "target_branch": "main",
    "state": "opened",
    "last_commit": {
      "id": "abc123..."
    }
  }
}
```

### Gogs Push

Same format as GitHub Push.

---

## Error Responses

```json
{
  "success": false,
  "message": "Error description",
  "triggered_jobs": []
}
```

HTTP status codes:
- `200 OK` - Success
- `400 Bad Request` - Invalid request
- `401 Unauthorized` - Not authenticated
- `500 Internal Server Error` - Server error
