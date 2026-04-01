# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2024-01-01

### Added

- **Web UI**: Modern interface with Tailwind CSS dark theme
  - Multi-user authentication (session-based with bcrypt)
  - Real-time log streaming via Server-Sent Events (SSE)
  - Job and Run management pages
  - Trigger management (enable/disable scheduled triggers)
  - Webhook management page

- **REST API**: Full programmatic access
  - Health check endpoint (`GET /health`)
  - Status endpoint (`GET /api/status`)
  - Jobs management (`GET /api/jobs`)
  - Runs management (`GET /api/runs`)
  - Trigger management (`POST /api/triggers/:name/enable|disable`)
  - Webhook endpoints (`POST /api/webhooks/:source`)
  - Log streaming (`GET /stream/logs/:run_id`)

- **CLI Client**: Command-line tool with RPC support
  - `ruci submit run` - Submit jobs from YAML files
  - `ruci job list` - List all jobs
  - `ruci job queue` - Queue a job for execution
  - `ruci job abort` - Abort running job
  - `ruci status` - Check daemon status
  - `ruci config validate` - Validate configuration

- **RPC Protocol**: Using tarpc framework with JSON codec
  - `queue_job`, `abort_job`, `submit_job`
  - `list_jobs`, `get_job`, `list_queued`, `list_running`
  - `get_run`, `get_run_log`
  - `upload_artifact`, `download_artifact`, `list_artifacts`

- **Job Execution**
  - Bash executor with context-based resource limits
  - Timeout support per job and context
  - Parallel job execution control via semaphores
  - VCS integration (automatic git clone/fetch + checkout)

- **Scheduled Triggers**: Cron-based job scheduling
  - Configurable via YAML configuration
  - Enable/disable via API and Web UI

- **Webhook Triggers**: GitHub, GitLab, Gogs support
  - HMAC-SHA256 signature verification (GitHub/Gogs)
  - Token-based verification (GitLab)
  - Push, Pull Request, Merge Request events
  - Branch and repository filtering with glob patterns
  - Automatic VCS parameter passing to jobs

- **VCS Integration**
  - Unified VCS abstraction layer
  - Support for Github, Gitlab, Gogs, Custom platforms
  - Automatic code checkout before job execution
  - Credential management via database

- **Storage**
  - Local filesystem storage
  - S3/Rustfs storage support with AWS SDK

- **Database**
  - SQLite with sqlx
  - PostgreSQL support
  - Repository pattern for data access

- **Operations**
  - Graceful shutdown with job completion tracking
  - Queue recovery on restart
  - Configuration hot reload via SIGHUP
  - Log rotation with tracing-appender
  - Job archival with tar
  - Prometheus metrics (`GET /metrics`)

### Changed

- Improved error handling with structured error codes
- Enhanced logging with tracing framework

### Fixed

- Context max_parallel configuration now enforced
- Queue job parameters persisted to database

### Documentation

- README.md with project overview and quick start
- API.md with REST API documentation
- DEPLOY.md with deployment guide (Docker, systemd)
- DESIGN.md with architecture documentation
- CONTRIBUTING.md with development guidelines
- Job configuration examples in contrib/examples/
