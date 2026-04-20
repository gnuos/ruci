# Ruci CD 设计文档

## 项目概述

Ruci CD 是一个轻量级的持续交付系统，使用 Rust 编写，支持 Web UI、REST API 和 CLI 客户端。

- **CLI 客户端**: `ruci`
- **Daemon 服务端**: `rucid`
- **设计目标**: 高性能、模块化、现代化、可扩展

---

## 项目结构

```
ruci/
├── Cargo.toml              # Workspace 配置
│
├── ruci/                   # CLI 客户端
│   ├── Cargo.toml
│   └── src/main.rs         # CLI 入口 (clap)
│
├── rucid/                  # Daemon 服务端
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs         # 主入口、路由、启动逻辑
│       └── web/
│           ├── handlers.rs  # HTTP handlers、Web UI 页面
│           └── webhooks.rs  # Webhook 处理
│
├── ruci-core/              # 核心库
│   ├── Cargo.toml
│   ├── src/
│   │   ├── lib.rs          # AppContext 定义
│   │   ├── config.rs       # 配置加载与验证
│   │   ├── db/             # 数据库抽象层
│   │   │   ├── mod.rs      # Repository trait 定义
│   │   │   ├── sqlite.rs   # SQLite 实现
│   │   │   ├── postgres.rs # PostgreSQL 实现
│   │   │   └── mysql.rs    # MySQL 实现
│   │   ├── queue.rs        # Job 队列 (flume)
│   │   ├── executor.rs     # Bash 执行器
│   │   ├── storage.rs      # 存储抽象 (local/S3)
│   │   ├── rpc.rs          # RPC 服务端
│   │   ├── trigger.rs      # Cron 触发器调度
│   │   ├── auth.rs         # 认证服务 (bcrypt)
│   │   ├── metrics.rs      # Prometheus 指标
│   │   ├── archive.rs      # 作业归档
│   │   └── vcs.rs          # VCS/Git 操作
│
├── ruci-protocol/          # RPC 协议定义
│   ├── Cargo.toml
│   └── src/lib.rs          # Service trait、类型定义
│
├── contrib/                # 部署文件
│   ├── examples/           # 作业配置示例
│   ├── ruci.yaml.example   # 配置示例
│   ├── rucid.service       # systemd 服务
│   ├── docker-compose.yml
│   ├── docker/
│   └── install.sh
│
├── docs/                   # 文档
│   ├── API.md              # API 文档
│   ├── DEPLOY.md           # 部署指南
│   └── DESIGN.md           # 本文档
│
└── Makefile                # 构建脚本
```

---

## 技术栈

| 功能 | 库 | 版本 |
|------|-----|------|
| 异步运行时 | tokio | 1.40 |
| RPC 框架 | tarpc | 0.37 |
| Web 框架 | axum | 0.7 |
| 数据库 | sqlx | (sqlite, postgres, mysql) |
| S3 存储 | aws-sdk-s3 | 1 |
| CLI | clap | 4.5 |
| 认证 | bcrypt | - |
| 日志 | tracing + tracing-appender | - |
| 调度 | tokio-cron-scheduler | 0.10 |
| 指标 | prometheus-client | 0.22 |

---

## 架构设计

### 整体架构

```
┌──────────────────────────────────────────────────────────┐
│                         CLI (ruci)                       │
│    submit / job / run / config / status / daemon         │
└─────────────────────┬────────────────────────────────────┘
                      │ tarpc RPC (TCP 或 Unix Socket)
                      ▼
┌──────────────────────────────────────────────────────────┐
│                     Daemon (rucid)                       │
│                                                          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐   │
│  │ RPC Server  │  │  Web UI     │  │ Job Queue       │   │
│  │ (tarpc)     │  │  (Axum)     │  │ Consumer        │   │
│  └─────────────┘  └─────────────┘  └─────────────────┘   │
│         │                │                   │           │
│         └────────────────┼───────────────────┘           │
│                          ▼                               │
│              ┌─────────────────────┐                     │
│              │   AppContext        │                     │
│              │   (ruci-core/lib)   │                     │
│              └─────────────────────┘                     │
└─────────────────────┬────────────────────────────────────┘
                      │
┌─────────────────────┼────────────────────────────────────┐
│                ruci-core                                 │
│  ┌──────────┐ ┌────┴────┐ ┌──────────┐ ┌─────────────┐   │
│  │ db/      │ │ queue/  │ │ storage/ │ │ executor/   │   │
│  │ sqlite   │ │         │ │ local/s3 │ │ BashExecutor│   │
│  │ postgres │ │         │ │          │ │             │   │
│  └──────────┘ └─────────┘ └──────────┘ └─────────────┘   │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌────────────┐   │
│  │ archive/ │ │ trigger/ │ │  vcs/    │ │   auth/    │   │
│  │          │ │ scheduler│ │  (Git)   │ │  (bcrypt)  │   │
│  └──────────┘ └──────────┘ └──────────┘ └────────────┘   │
└─────────────────────┬────────────────────────────────────┘
                      │
┌─────────────────────┼────────────────────────────────────┐
│              ruci-protocol                               │
│  ┌────────────────────────────────────────────────────┐  │
│  │ RuciRpc service (tarpc::service)                   │  │
│  │ - queue_job, abort_job, submit_job, get_run...     │  │
│  └────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────┘
```

### 组件说明

#### AppContext

核心共享上下文，在 `ruci-core/src/lib.rs` 定义：

```rust
pub struct AppContext {
    pub config: Config,
    pub db: Arc<dyn Database>,
    pub queue: JobQueue,
    pub storage: Arc<dyn Storage>,
    pub metrics: Metrics,
    pub archive: ArchiveManager,
    pub auth: AuthService,
}
```

#### JobQueue

使用 `flume` 库实现的生产者-消费者队列：

```rust
pub struct JobQueue {
    sender: Sender<QueueRequest>,
    receiver: AsyncReceiver<QueueRequest>,
}
```

#### Database

使用 Repository 模式，支持 SQLite、PostgreSQL 和 MySQL：

```rust
pub trait Repository: Send + Sync {
    async fn list_jobs() -> Result<Vec<JobInfo>>;
    async fn get_job(id: &str) -> Result<Option<JobInfo>>;
    async fn create_run(run: &Run) -> Result<()>;
    async fn update_run_status() -> Result<()>;
    // ... 其他方法
}
```

---

## 数据库 Schema

### Jobs 表

```sql
CREATE TABLE jobs (
    id TEXT PRIMARY KEY,              -- SHA-256 hash
    original_name TEXT NOT NULL,      -- 原始文件名
    name TEXT NOT NULL,               -- 作业名称
    submitted_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    config_yaml TEXT NOT NULL,        -- 完整 YAML 内容
    content_hash TEXT NOT NULL        -- SHA-256 hash
);
```

### Runs 表

```sql
CREATE TABLE runs (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL,
    build_num INTEGER NOT NULL,
    status TEXT NOT NULL,             -- QUEUED, RUNNING, SUCCESS, FAILED, ABORTED
    started_at TIMESTAMP,
    finished_at TIMESTAMP,
    exit_code INTEGER,
    params TEXT,                      -- JSON 格式参数字符串
    FOREIGN KEY (job_id) REFERENCES jobs(id)
);
```

### 其他表

- `artifacts` - 制品信息
- `triggers` - 定时触发器配置
- `webhook_triggers` - Webhook 触发器配置
- `vcs_credentials` - VCS 认证凭据
- `users` - Web UI 用户
- `sessions` - 用户会话

---

## 作业定义格式

```yaml
name: my-job
context: default
timeout: 300

# VCS 配置 (可选，自动从 webhook 获取)
vcs:
  url: "https://github.com/owner/repo.git"
  repository: "owner/repo"
  branch: "main"
  submodules: false

checkout: true  # 是否自动执行 git checkout

env:
  KEY: "value"

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

---

## REST API

### 端点列表

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/health` | 健康检查 |
| GET | `/api/status` | 服务状态 |
| GET | `/api/jobs` | 列出所有作业 |
| GET | `/api/runs` | 列出运行中的作业 |
| POST | `/api/webhooks/:source` | 接收 webhook |
| POST | `/api/triggers/:name/enable` | 启用触发器 |
| POST | `/api/triggers/:name/disable` | 禁用触发器 |
| POST | `/api/webhooks` | 创建 webhook |
| GET | `/stream/logs/:run_id` | SSE 日志流 |

详见 [API.md](API.md)

---

## RPC 协议

使用 tarpc 框架，JSON 编解码：

```rust
#[service]
pub trait RuciRpc {
    async fn queue_job(job_id: String, params: HashMap<String, String>) -> QueueResponse;
    async fn abort_job(run_id: String) -> ();
    async fn submit_job(yaml_content: String) -> JobSubmitResponse;
    async fn list_jobs() -> Vec<JobInfo>;
    async fn get_run(run_id: String) -> Option<RunInfo>;
    async fn get_run_log(run_id: String) -> String;
    // ...
}
```

---

## 配置系统

### 配置文件加载顺序

1. `./ruci.yaml` (当前目录)
2. `~/.config/ruci/ruci.yaml` (用户目录)
3. `/etc/ruci/ruci.yaml` (系统目录)

### 主要配置项

```yaml
server:
  host: "127.0.0.0"
  port: 7741
  web_host: "127.0.0.0"
  web_port: 8080

database:
  url: "sqlite:///var/lib/ruci/db/ruci.db"

storage:
  type: "local"  # 或 "rustfs" (S3)

contexts:
  default:
    max_parallel: 4
    timeout: 3600
    work_dir: "/tmp"

web:
  enabled: true
  admin_username: "admin"
  admin_password: "admin"
```

详见 [ruci.yaml.example](../contrib/ruci.yaml.example)

---

## 关键路径

| 用途 | 路径 |
|------|------|
| 系统配置 | `/etc/ruci/ruci.yaml` |
| 用户配置 | `~/.config/ruci/ruci.yaml` |
| 本地配置 | `./ruci.yaml` |
| 数据库 | `/var/lib/ruci/db/ruci.db` |
| RPC TCP | `127.0.0.0:7741` |
| Web UI | `127.0.0.0:8080` |

---

## 错误处理

使用 `thiserror` 定义错误类型：

```rust
#[derive(Error, Debug)]
pub enum Error {
    #[error("Config error: {0}")]
    Config(String),

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Queue error: {0}")]
    Queue(String),

    #[error("Executor error: {0}")]
    Executor(String),

    #[error("Job not found: {0}")]
    JobNotFound(String),
}
```

---

## 扩展指南

Ruci 设计了多个扩展点（Plugin Abstraction），允许开发者自定义核心功能。

### 扩展点概览

| 扩展点 | Trait | 位置 | 说明 |
|--------|-------|------|------|
| 执行器 | `Executor` | `executor.rs` | 自定义作业执行方式 |
| 存储 | `Storage` | `storage.rs` | 自定义制品存储后端 |
| VCS | `VcsOperations` | `vcs.rs` | 自定义版本控制系统 |
| 数据库 | `Repository` | `db/repository.rs` | 自定义数据库实现 |

---

### 1. 执行器扩展 (Executor)

执行器负责实际运行作业步骤。

```rust
// 位置: ruci-core/src/executor.rs

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
```

**现有实现**: `BashExecutor` - 本地 Bash 脚本执行

**扩展示例**: Docker 执行器

```rust
pub struct DockerExecutor {
    image: String,
    network: Option<String>,
}

#[async_trait]
impl Executor for DockerExecutor {
    async fn execute(&self, ctx: &ExecutionContext, job: &Job) -> Result<ExecutionResult> {
        // 构建 docker run 命令
        let mut cmd = vec!["docker", "run", "--rm"];
        if let Some(net) = &self.network {
            cmd.push("--network");
            cmd.push(net);
        }
        cmd.push(&self.image);
        cmd.push(&job.steps[0].command);

        // 执行并返回结果
        // ...
        Ok(ExecutionResult {
            exit_code: 0,
            logs: "".to_string(),
            artifacts: vec![],
        })
    }

    async fn abort(&self, run_id: &str) -> Result<()> {
        // docker kill <container>
        Ok(())
    }
}
```

**注册方式**: 在 `rucid/src/main.rs` 中替换默认执行器：

```rust
let executor: Arc<dyn Executor> = Arc::new(DockerExecutor::new("my-image"));
```

---

### 2. 存储扩展 (Storage)

存储负责制品的上传、下载、删除。

```rust
// 位置: ruci-core/src/storage.rs

#[async_trait]
pub trait Storage: Send + Sync {
    /// Upload a file to storage
    async fn upload(&self, key: &str, path: &Path) -> Result<StorageHandle>;

    /// Download a file from storage
    async fn download(&self, key: &str) -> Result<Vec<u8>>;

    /// Check if an object exists
    async fn exists(&self, key: &str) -> bool;

    /// Delete an object
    async fn delete(&self, key: &str) -> Result<()>;

    /// Get the URL for an object (if public)
    fn url(&self, key: &str) -> Option<String>;
}
```

**现有实现**:
- `LocalStorage` - 本地文件系统
- `S3Storage` - AWS S3 / MinIO

**扩展示例**: Google Cloud Storage

```rust
pub struct GcsStorage {
    bucket: String,
    client: GcsClient,
}

#[async_trait]
impl Storage for GcsStorage {
    async fn upload(&self, key: &str, path: &Path) -> Result<StorageHandle> {
        let content = std::fs::read(path)?;
        self.client.upload(&self.bucket, key, &content).await?;
        Ok(StorageHandle {
            key: key.to_string(),
            size: content.len() as u64,
            checksum: calculate_checksum(path)?,
        })
    }

    async fn download(&self, key: &str) -> Result<Vec<u8>> {
        self.client.download(&self.bucket, key).await
    }

    async fn exists(&self, key: &str) -> bool {
        self.client.head(&self.bucket, key).await.is_ok()
    }

    async fn delete(&self, key: &str) -> Result<()> {
        self.client.delete(&self.bucket, key).await
    }

    fn url(&self, key: &str) -> Option<String> {
        Some(format!("https://storage.googleapis.com/{}/{}", self.bucket, key))
    }
}
```

**注册方式**: 在 `storage.rs` 的 `create_storage` 函数中添加：

```rust
pub async fn create_storage(config: &StorageConfig) -> Result<Box<dyn Storage>> {
    match config.storage_type {
        StorageType::Local => Ok(Box::new(LocalStorage::new(...))),
        StorageType::Rustfs => Ok(Box::new(S3Storage::new(config).await?)),
        StorageType::Gcs => Ok(Box::new(GcsStorage::new(config).await?)),  // 新增
    }
}
```

---

### 3. VCS 扩展 (VcsOperations)

VCS 负责代码的克隆和检出。

```rust
// 位置: ruci-core/src/vcs.rs

#[async_trait]
pub trait VcsOperations: Send + Sync {
    /// Clone repository
    async fn clone(&self, url: &str, branch: &str, work_dir: &Path, submodules: bool) -> Result<()>;

    /// Fetch updates
    async fn fetch(&self, url: &str, branch: &str, work_dir: &Path) -> Result<()>;

    /// Checkout specific ref (branch, tag, or commit)
    async fn checkout(&self, work_dir: &Path, ref_: &str) -> Result<()>;

    /// Get current commit SHA
    async fn get_current_commit(&self, work_dir: &Path) -> Result<String>;
}
```

**现有实现**: `GitOperations` - Git 操作

**扩展示例**: SVN 支持

```rust
pub struct SvnOperations {
    // SVN 配置
}

#[async_trait]
impl VcsOperations for SvnOperations {
    async fn clone(&self, url: &str, branch: &str, work_dir: &Path, _submodules: bool) -> Result<()> {
        // svn checkout URL work_dir
        tokio::process::Command::new("svn")
            .args(["checkout", &format!("{}/{}", url, branch)])
            .current_dir(work_dir)
            .output()
            .await?;
        Ok(())
    }

    async fn fetch(&self, url: &str, branch: &str, work_dir: &Path) -> Result<()> {
        // svn update
        Ok(())
    }

    async fn checkout(&self, work_dir: &Path, ref_: &str) -> Result<()> {
        // svn switch ref_
        Ok(())
    }

    async fn get_current_commit(&self, work_dir: &Path) -> Result<String> {
        // svn info --xml
        Ok("".to_string())
    }
}
```

---

### 4. 数据库扩展 (Repository)

数据库层使用 Repository 模式，提供数据访问抽象。

```rust
// 位置: ruci-core/src/db/repository.rs

pub trait Repository: JobRepository + RunRepository + ArtifactRepository
                    + UserRepository + TriggerRepository + WebhookRepository
                    + VcsCredentialRepository + Send + Sync {
    // 综合所有子 trait
}

pub trait JobRepository: Send + Sync {
    async fn create_job(&self, job: &Job) -> Result<()>;
    async fn get_job(&self, id: &str) -> Result<Option<Job>>;
    async fn list_jobs(&self) -> Result<Vec<Job>>;
    async fn delete_job(&self, id: &str) -> Result<()>;
}

pub trait RunRepository: Send + Sync {
    async fn create_run(&self, run: &Run) -> Result<()>;
    async fn get_run(&self, id: &str) -> Result<Option<Run>>;
    async fn list_runs_by_status(&self, status: &str) -> Result<Vec<Run>>;
    async fn update_run_status(&self, id: &str, status: &str, exit_code: Option<i32>) -> Result<()>;
}

// ... 其他 Repository 类似
```

**现有实现**:
- `SqliteRepository` - SQLite 实现
- `PostgresRepository` - PostgreSQL 实现
- `MysqlRepository` - MySQL 实现

---

### 5. 添加新的扩展点

如需添加新的扩展点：

1. 在 `ruci-core/src/` 下创建新模块（如 `my_plugin.rs`）
2. 定义 trait：

```rust
#[async_trait]
pub trait MyPlugin: Send + Sync {
    async fn do_something(&self, arg: &str) -> Result<String>;
}
```

3. 在 `lib.rs` 中导出：

```rust
pub mod my_plugin;
pub use my_plugin::MyPlugin;
```

4. 在 `AppContext` 中集成：

```rust
pub struct AppContext {
    // ... 现有字段
    pub my_plugin: Arc<dyn MyPlugin>,
}
```

5. 在 `rucid/src/main.rs` 中初始化

---

### 配置扩展

某些扩展可以在配置文件中指定：

```yaml
# 在 ruci.yaml.example 中
my_plugin:
  option1: "value1"
  option2: "value2"
```

---

## 开发指南

### 构建

```bash
make build          # Release 构建
make build-dev      # Dev 构建
make test           # 运行测试
make test-all       # 测试 + fmt + clippy
```

### 添加新模块

1. 在 `ruci-core/src/` 下创建模块文件
2. 在 `lib.rs` 中添加 `pub mod xxx;`
3. 在 `AppContext` 中集成新模块

### 添加新 RPC 方法

1. 在 `ruci-protocol/src/lib.rs` 的 `RuciRpc` trait 中添加方法
2. 在 `ruci-core/src/rpc.rs` 中实现

---

## 参考资料

- [tarpc 文档](https://docs.rs/tarpc/latest/tarpc/)
- [axum 文档](https://docs.rs/axum/latest/axum/)
- [sqlx 文档](https://docs.rs/sqlx/latest/sqlx/)
- [Prometheus metrics 格式](https://prometheus.io/docs/instrumenting/exposition_formats/)
