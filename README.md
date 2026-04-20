# Ruci CD

这是一个使用 Rust 编写的轻量级CI/CD系统，提供 Web UI、REST API 和 CLI 客户端。为的是做一个 [Laminar](https://github.com/ohwgiles/laminar) 的替代品。

**本项目通过 MiniMax-M2.7 和 MiMo-V2-Pro 模型设计并实现了全部的功能**

## 目录

- [核心特性](#核心特性)
- [技术栈](#技术栈)
- [快速开始](#快速开始)
- [部署方式](#部署方式)
- [配置说明](#配置说明)
- [作业定义](#作业定义)
- [Webhook 集成](#webhook-集成)
- [架构概览](#架构概览)
- [项目结构](#项目结构)
- [扩展指南](#扩展指南)
- [测试](#测试)
- [相关文档](#相关文档)
- [许可证](#许可证)

---

## 核心特性

### Web UI

- 多用户认证（bcrypt 密码哈希 + Session）
- 实时日志流（Server-Sent Events）
- 作业与运行管理
- 定时触发器管理（启用/禁用）
- Webhook 触发器管理
- 队列状态监控
- Tailwind CSS 深色主题

### REST API

- 健康检查与状态端点
- 作业与运行管理接口
- Webhook 接收端点（GitHub、GitLab、Gogs）
- 触发器启用/禁用接口
- SSE 日志流接口

### CLI 客户端

- `ruci submit run` — 从 YAML 文件提交作业
- `ruci job list/queue/abort` — 作业管理
- `ruci run` — 本地执行与监控
- `ruci config validate` — 配置验证
- `ruci status` — Daemon 状态查询

### 高级功能

- **多数据库支持** — SQLite、PostgreSQL、MySQL
- **定时触发器** — Cron 表达式调度
- **Webhook 触发器** — GitHub/GitLab/Gogs，支持 HMAC 签名验证
- **VCS 集成** — 自动 Git 克隆/检出，凭据管理
- **制品存储** — 本地文件系统或 S3/MinIO
- **作业归档** — tar 归档 + 定期清理
- **优雅关闭** — 等待运行中作业完成，超时强制终止
- **队列恢复** — 重启后自动恢复队列中的作业
- **配置热更新** — SIGHUP 信号重新加载配置
- **日志轮转** — 按日轮转，可配置保留天数
- **并发控制** — 基于 Semaphore 的 context 级并发限制
- **Prometheus 指标** — `/metrics` 端点

---

## 技术栈

| 功能 | 技术 |
|------|------|
| 核心语言 | Rust |
| 异步运行时 | tokio |
| RPC 框架 | tarpc |
| Web 框架 | axum |
| 数据库 | sqlx (SQLite / PostgreSQL / MySQL) |
| 任务队列 | flume |
| 制品存储 | aws-sdk-s3 |
| CLI | clap |
| 认证 | bcrypt |
| 定时调度 | tokio-cron-scheduler |
| 日志 | tracing + tracing-appender |
| 指标 | prometheus-client |

---

## 快速开始

### 前置要求

- Rust 1.90+
- Cargo

### 构建

```bash
make build          # Release 构建
make build-dev      # Dev 构建（调试用）
```

### 运行

```bash
# Dev 模式（带 debug 日志）
make dev

# 指定配置文件
./bin/rucid --config contrib/ruci.yaml.example
```

### CLI 使用

```bash
# 提交作业
./bin/ruci submit run --file .ruci.yml

# 列出作业
./bin/ruci job list

# 查看状态
./bin/ruci status

# 验证配置
./bin/ruci config validate --config /path/to/ruci.yaml
```

---

## 部署方式

### Docker Compose

```bash
# 启动
docker-compose -f contrib/docker-compose.yml up -d

# 查看日志
docker-compose -f contrib/docker-compose.yml logs -f

# 健康检查
curl http://localhost:8080/health
```

### Systemd

```bash
# 构建并安装
make build
sudo ./contrib/install.sh

# 服务管理
sudo systemctl start rucid
sudo systemctl status rucid
journalctl -u rucid -f
```

详细部署指南见 [docs/DEPLOY.md](docs/DEPLOY.md)。

---

## 配置说明

配置文件按以下优先级加载：

1. `./ruci.yaml`（当前目录）
2. `~/.config/ruci/ruci.yaml`（用户目录）
3. `/etc/ruci/ruci.yaml`（系统目录）
4. `--config` 命令行参数指定

### 完整配置示例

```yaml
# 服务端配置
server:
  host: "0.0.0.0"           # RPC 绑定地址
  port: 7741                # RPC 端口
  web_host: "0.0.0.0"       # Web UI 绑定地址
  web_port: 8080            # Web UI 端口
  rpc_mode: "tcp"           # "tcp" 或 "unix"

# 数据库配置
database:
  url: "sqlite:///var/lib/ruci/db/ruci.db"
  # url: "postgresql://ruci:password@localhost:5432/ruci"
  # url: "mysql://ruci:password@localhost:3306/ruci"

# 制品存储
storage:
  type: "local"             # "local" 或 "rustfs" (S3)
  # type: "rustfs"
  # endpoint: "http://localhost:9000"
  # bucket: "ruci-artifacts"
  # access_key: "${AWS_ACCESS_KEY_ID}"
  # secret_key: "${AWS_SECRET_ACCESS_KEY}"

# 路径配置
paths:
  db_dir: "/var/lib/ruci/db"
  jobs_dir: "/var/lib/ruci/jobs"
  run_dir: "/var/lib/ruci/run"
  archive_dir: "/var/lib/ruci/archive"
  log_dir: "/var/log/ruci"

# Context 资源限制
contexts:
  default:
    max_parallel: 4         # 最大并发作业数
    timeout: 3600           # 作业超时（秒）
    work_dir: "/var/lib/ruci/tmp"

# 定时触发器
triggers:
  # - name: "hourly-cleanup"
  #   cron: "0 * * * *"
  #   job: "cleanup-job-id"
  #   enabled: true

# 日志配置
logging:
  level: "info"             # trace, debug, info, warn, error
  format: "json"            # "json" 或 "pretty"
  file:
    dir: "/var/log/ruci"
    max_size_mb: 100
    max_files: 7

# 归档配置
archive:
  enabled: true
  max_age_days: 30

# Web UI 认证
web:
  enabled: true
  admin_username: "admin"
  admin_password: "admin"   # 生产环境请修改
```

完整配置示例见 [contrib/ruci.yaml.example](contrib/ruci.yaml.example)。

---

## 作业定义

作业通过 YAML 文件（`.ruci.yml` 或 `.ruci.yaml`）定义：

```yaml
name: my-job
context: default
timeout: 300

# VCS 配置（可选，Webhook 触发时自动填充）
vcs:
  url: "https://github.com/owner/repo.git"
  branch: "main"
  submodules: false

checkout: true  # 自动执行 git clone/fetch + checkout

env:
  BUILD_VERSION: "1.0.0"

steps:
  - name: build
    command: make build

  - name: test
    command: make test

  - name: archive
    command: tar czf artifacts.tar.gz build/
    artifacts:
      - artifacts.tar.gz
```

### VCS 集成

当通过 Webhook 触发时，VCS 参数（`clone_url`、`branch`、`commit_sha`）会自动传递给作业。支持的平台：

- **GitHub** — `repository.clone_url`、`ref`、`after`
- **GitLab** — `project.git_http_url`、`ref`、`checkout_sha`
- **Gogs** — `repository.clone_url`、`ref`、`after`

---

## Webhook 集成

Ruci 支持从多个 VCS 平台接收 Webhook：

### 支持的平台

| 平台 | 签名验证 | 事件类型 |
|------|---------|---------|
| GitHub | HMAC-SHA256 | Push、Pull Request |
| GitLab | Token | Push、Merge Request |
| Gogs | HMAC-SHA256 | Push、Pull Request |

### 创建 Webhook

```bash
# 通过 API 创建
curl -X POST http://localhost:8080/api/webhooks \
  -H "Content-Type: application/json" \
  -d '{
    "name": "my-webhook",
    "source": "github",
    "job_id": "abc123",
    "secret": "my-secret",
    "filter": {
      "repository": "owner/repo",
      "branches": ["main", "develop"],
      "events": ["push", "pull_request"]
    },
    "enabled": true
  }'
```

### Webhook 接收端点

```
POST /api/webhooks/:source
```

其中 `:source` 为 `github`、`gitlab` 或 `gogs`。

详细 API 文档见 [docs/API.md](docs/API.md)。

---

## 架构概览

```
┌──────────────────────────────────────────────────────────┐
│                      CLI (ruci)                          │
│      submit / job / run / config / status                │
└─────────────────────┬────────────────────────────────────┘
                      │ tarpc RPC (TCP / Unix Socket)
                      ▼
┌──────────────────────────────────────────────────────────┐
│                    Daemon (rucid)                        │
│                                                          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐   │
│  │ RPC Server  │  │  Web UI     │  │ Job Queue       │   │
│  │ (tarpc)     │  │  (Axum)     │  │ Consumer        │   │
│  └─────────────┘  └─────────────┘  └─────────────────┘   │
└─────────────────────┬────────────────────────────────────┘
                      │
┌─────────────────────┼────────────────────────────────────┐
│              ruci-core                                   │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌────────────┐   │
│  │ db/      │ │ queue/   │ │ storage/ │ │ executor/  │   │
│  │ mysql    │ │ (flume)  │ │ local/s3 │ │ Bash       │   │
│  │ postgres │ │          │ │          │ │            │   │
│  └──────────┘ └──────────┘ └──────────┘ └────────────┘   │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌────────────┐   │
│  │ archive/ │ │ trigger/ │ │  vcs/    │ │   auth/    │   │
│  │          │ │ scheduler│ │  (Git)   │ │  (bcrypt)  │   │
│  └──────────┘ └──────────┘ └──────────┘ └────────────┘   │
└─────────────────────┬────────────────────────────────────┘
                      │
┌─────────────────────┼────────────────────────────────────┐
│              ruci-protocol                               │
│  ┌────────────────────────────────────────────────────┐  │
│  │ RuciRpc service (tarpc)                            │  │
│  │ - queue_job, abort_job, submit_job, list_jobs...   │  │
│  └────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────┘
```

详细架构设计见 [docs/DESIGN.md](docs/DESIGN.md)。

---

## 项目结构

```
rucicd/
├── Cargo.toml                # Workspace 配置
├── Makefile                  # 构建脚本
├── README.md                 # 本文件
├── AGENTS.md                 # 开发规范
├── TODO.md                   # 项目待办
├── CHANGELOG.md              # 变更日志
│
├── ruci/                     # CLI 客户端
│   └── src/main.rs
│
├── rucid/                    # Daemon 服务端
│   └── src/
│       ├── main.rs           # 入口、路由、启动逻辑
│       └── web/
│           ├── handlers.rs   # HTTP handlers、Web UI
│           └── webhooks.rs   # Webhook 处理
│
├── ruci-core/                # 核心库
│   └── src/
│       ├── lib.rs            # AppContext 定义
│       ├── config.rs         # 配置加载与验证
│       ├── db/               # 数据库抽象层
│       │   ├── mod.rs        # Repository trait
│       │   ├── repository.rs # 综合 Repository
│       │   ├── sqlite.rs     # SQLite 实现
│       │   ├── postgres.rs   # PostgreSQL 实现
│       │   └── mysql.rs      # MySQL 实现
│       ├── queue.rs          # Job 队列 (flume)
│       ├── executor.rs       # Bash 执行器
│       ├── storage.rs        # 存储抽象 (local/S3)
│       ├── rpc.rs            # RPC 服务端
│       ├── trigger.rs        # Cron 调度器
│       ├── auth.rs           # 认证服务 (bcrypt)
│       ├── metrics.rs        # Prometheus 指标
│       ├── archive.rs        # 作业归档
│       ├── vcs.rs            # VCS/Git 操作
│       └── error.rs          # 错误类型定义
│
├── ruci-protocol/            # RPC 协议定义
│   └── src/lib.rs            # Service trait、类型
│
├── docs/                     # 文档
│   ├── API.md                # REST API 文档
│   ├── DEPLOY.md             # 部署指南
│   └── DESIGN.md             # 架构设计
│
├── contrib/                  # 部署文件
│   ├── ruci.yaml.example     # 配置示例
│   ├── rucid.service         # systemd 服务
│   ├── docker-compose.yml
│   ├── docker/
│   │   ├── Dockerfile
│   │   └── entrypoint.sh
│   └── install.sh
│
├── bin/                      # 构建产物
└── target/                   # Cargo 构建目录
```

---

## 扩展指南

Ruci 设计了多个扩展点（Plugin Abstraction），允许自定义核心功能：

| 扩展点 | Trait | 位置 | 说明 |
|--------|-------|------|------|
| 执行器 | `Executor` | `executor.rs` | 自定义作业执行方式（如 Docker） |
| 存储 | `Storage` | `storage.rs` | 自定义制品存储后端（如 GCS） |
| VCS | `VcsOperations` | `vcs.rs` | 自定义版本控制系统（如 SVN） |
| 数据库 | `Repository` | `db/repository.rs` | 自定义数据库实现 |

详细扩展指南见 [docs/DESIGN.md](docs/DESIGN.md)。

---

## 测试

```bash
# 运行所有测试
make test

# 测试 + 格式检查 + Clippy
make test-all

# 运行特定模块测试
cargo test -p ruci-core

# 查看测试覆盖率
cargo tarpaulin --all
```

当前测试数：207（ruci-core 195 + ruci-protocol 12）

覆盖模块：config、executor、queue、db、rpc、storage、metrics、error、trigger、archive

---

## 相关文档

| 文档 | 说明 |
|------|------|
| [API.md](docs/API.md) | REST API 接口文档 |
| [DEPLOY.md](docs/DEPLOY.md) | 部署指南（Docker、systemd） |
| [DESIGN.md](docs/DESIGN.md) | 架构设计与扩展指南 |
| [CONTRIBUTING.md](CONTRIBUTING.md) | 贡献指南与代码规范 |
| [AGENTS.md](AGENTS.md) | AI 协作开发规范 |
| [TODO.md](TODO.md) | 项目待办与功能状态 |
| [CHANGELOG.md](CHANGELOG.md) | 版本变更记录 |

---

## 许可证

本项目采用双重许可：

- [MIT License](LICENSE-MIT)
- [Apache License 2.0](LICENSE-APACHE)

由您选择适用的许可证。
