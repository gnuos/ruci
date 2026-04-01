# Ruci CI

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE-MIT)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE-APACHE)
[![Rust](https://img.shields.io/badge/Rust-1.70+-orange.svg)](https://www.rust-lang.org)

A lightweight CI system written in Rust, featuring a Web UI, REST API, and CLI client.

## Features

- **Web UI**: Modern interface with Tailwind CSS dark theme
  - Multi-user authentication (session-based)
  - Real-time log streaming via SSE
  - Job/Run management
  - Trigger and Webhook management

- **REST API**: Full programmatic access
  - Status and health endpoints
  - Job and run management
  - Webhook integration (GitHub, GitLab, Gogs)

- **CLI Client**: Command-line tool with RPC support
  - `ruci submit` - Submit jobs from YAML files
  - `ruci job` - Manage jobs (list, queue, abort)
  - `ruci run` - Local execution and monitoring
  - `ruci config` - Configuration management

- **Advanced Features**
  - Scheduled triggers (cron-based)
  - Webhook triggers (GitHub/GitLab/Gogs)
  - VCS integration (automatic git checkout)
  - Artifact storage (local or S3)
  - Job archival with rotation
  - Prometheus metrics

## Tech Stack

| Component | Technology |
|-----------|------------|
| Core | Rust |
| RPC | Tarpc |
| Web Framework | Axum |
| Database | SQLite / PostgreSQL / MySQL (via sqlx) |
| S3 Storage | aws-sdk-s3 |
| CLI | Clap |
| Authentication | bcrypt |
| Scheduling | tokio-cron-scheduler |

## Quick Start

### Prerequisites

- Rust 1.70+
- Cargo

### Build

```bash
make build
```

### Run Server

```bash
make dev
# Or with custom config:
./bin/rucid --config /path/to/ruci.yaml
```

### CLI Usage

```bash
# Submit a job
./bin/ruci submit run --file .ruci.yml

# List jobs
./bin/ruci job list

# Check status
./bin/ruci status

# Validate config
./bin/ruci config validate --config /path/to/ruci.yaml
```

## Project Structure

```
rucicd/
├── Cargo.toml              # Workspace configuration
├── Makefile                # Build scripts
├── README.md               # This file
├── AGENTS.md               # Development guidelines
├── TODO.md                 # Project tasks
│
├── ruci/                   # CLI client crate
│   └── src/main.rs
│
├── rucid/                  # Daemon server crate
│   └── src/
│       ├── main.rs         # Main entry point
│       └── web/
│           ├── handlers.rs  # HTTP handlers & Web UI
│           └── webhooks.rs  # Webhook processing
│
├── ruci-core/              # Core library crate
│   └── src/
│       ├── lib.rs          # AppContext definition
│       ├── config.rs       # Configuration management
│       ├── db.rs           # Database operations
│       ├── queue.rs        # Job queue (flume)
│       ├── executor.rs     # Bash executor
│       ├── storage.rs      # Storage abstraction
│       ├── rpc.rs          # RPC server
│       ├── trigger.rs      # Cron scheduler
│       ├── auth.rs         # Authentication
│       ├── metrics.rs      # Prometheus metrics
│       ├── archive.rs      # Job archival
│       └── vcs.rs          # VCS/Git operations
│
├── ruci-protocol/          # RPC protocol definitions
│   └── src/lib.rs          # Service trait & types
│
└── contrib/                # Deployment files
    ├── ruci.yaml.example   # Configuration example
    ├── rucid.service       # systemd service
    ├── docker/
    │   ├── Dockerfile
    │   └── entrypoint.sh
    ├── docker-compose.yml
    └── install.sh
```

## Configuration

Configuration file is loaded from (in order of priority):

1. `./ruci.yaml` (current directory)
2. `~/.config/ruci/ruci.yaml` (user directory)
3. `/etc/ruci/ruci.yaml` (system directory)

Or specify with `--config` flag:

```bash
./bin/rucid --config /path/to/config.yaml
```

See `contrib/ruci.yaml.example` for full configuration options.

## Job Definition

Jobs are defined in YAML files (`.ruci.yml` or `.ruci.yaml`):

```yaml
name: hello-world
context: default
timeout: 300

env:
  BUILD_VERSION: "1.0.0"

steps:
  - name: checkout
    command: git clone $REPO_URL /tmp/build

  - name: build
    command: make -C /tmp/build

  - name: test
    command: make test -C /tmp/build

  - name: archive
    command: tar czf /tmp/build.tar.gz /tmp/build
    artifacts:
      - /tmp/build.tar.gz
```

## Webhooks

Ruci supports webhooks from multiple VCS platforms:

- **GitHub**: HMAC-SHA256 signature verification
- **GitLab**: Token-based verification
- **Gogs**: Signature verification

Events supported:
- Push
- Pull Request / Merge Request

Webhook payloads are automatically parsed and VCS info (clone URL, branch, commit) is passed to jobs.

## License

This project is licensed under either of:
- [MIT License](LICENSE-MIT)
- [Apache License 2.0](LICENSE-APACHE)

at your option.
