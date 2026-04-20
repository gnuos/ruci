# Ruci 待办事项

本文档汇总了 Ruci 项目后续工作的指引。

## 项目概述

Ruci 是一个轻量级的 CI 系统，使用 Rust 编写。

### 当前已完成功能

- [x] 前台运行 + nohup 方式调试daemon
- [x] SIGTERM/SIGINT/Ctrl-C 信号处理
- [x] pid 文件和 socket 路径支持命令行参数覆盖配置文件
- [x] systemd 服务文件 (`contrib/rucid.service`)
- [x] 集成测试（RPC 客户端-服务端通信）
- [x] 健康检查端点 (`/health`)
- [x] 配置验证工具 (`Config::validate()`)
- [x] 客户端 `validate` 子命令
- [x] Prometheus metrics (`/metrics` 端点)
- [x] Docker 支持 (`contrib/docker/Dockerfile`, `docker-compose.yml`)
- [x] Makefile 构建脚本

---

## 功能补充

### 高优先级

#### 1. 单元测试覆盖率
**描述**: 当前项目的单元测试较少，需要补充核心模块的测试。

**涉及模块**:
- `ruci-core/src/config.rs` - 配置解析和验证
- `ruci-core/src/executor.rs` - Job 解析和执行
- `ruci-core/src/queue.rs` - 队列操作
- `ruci-core/src/db/` - 数据库操作

**目标**: 关键模块测试覆盖率 > 70%

**当前状态**: ✅ 已完成
- 所有核心模块测试已补充
- 总测试数: 207 (ruci-core 195 + ruci-protocol 12)
- 已覆盖: config, executor, queue, db, rpc, storage, metrics, error, trigger, archive

---

### 中优先级

#### 2. 作业归档功能
**描述**: 将完成的 job/run 归档到 `archive_dir` 目录。

**当前状态**: ✅ 已完成
- ArchiveManager 实现，支持 tar 归档
- 定期清理过期归档
- 集成到 AppContext

---

#### 3. 日志轮转
**描述**: 当前日志直接写入文件，需要配置日志轮转。

**当前状态**: ✅ 已完成
- 使用 `tracing-appender` 实现日志轮转
- 支持每日轮转和文件保留
- 可配置日志目录和保留天数

---

### 低优先级

#### 4. Web UI 界面
**描述**: 目前只有 REST API，可以添加简单的 HTML 界面。

**当前状态**: ✅ 已完成
- 多用户认证系统（bcrypt 密码哈希）
- Session-based 认证（内存存储）
- Tailwind CSS 深色主题
- SSE 实时日志流
- 页面：登录、作业列表、运行列表、运行详情、队列状态、触发器管理
- 触发器管理：支持启用/禁用定时任务

---

#### 5. S3 存储支持
**描述**: 代码中有 `rustfs` 类型但未完整实现。

**当前状态**: ✅ 已完成（凭证配置）
- [x] 使用配置中的 `access_key` 和 `secret_key` 初始化 S3 客户端
- [x] AWS SDK 支持环境变量、IAM Role 等凭证来源
- [x] S3 存储集成测试（10 个测试，使用 #[ignore] 跳过，需要 MinIO 或 AWS S3）

---

#### 6. 定时触发器管理
**描述**: 在 Web UI 中管理定时触发任务，支持启用/禁用操作。

**当前状态**: ✅ 已完成
- 触发器数据库表（triggers）
- Web UI 触发器管理页面 (`/ui/triggers`)
- API 接口：启用/禁用触发器
- 支持从配置文件和数据库加载触发器

---

#### 7. Webhook 触发器
**描述**: 支持 GitHub、GitLab、Gogs 的 webhook 触发，实现代码推送自动触发 CI 任务。

**当前状态**: ✅ 已完成
- Webhook receiver 端点 (`POST /api/webhooks/:source`)
- 支持 GitHub HMAC-SHA256 签名验证
- 支持 GitLab Token 验证
- 支持 Gogs 签名验证
- 支持事件类型: Push、Pull Request、Merge Request
- 支持分支和仓库过滤（支持 * glob）
- Web UI webhook 管理页面 (`/ui/webhooks`)
- API 接口: 创建、启用、禁用、删除 webhook
- Webhook 触发器数据库表 (`webhook_triggers`)

**VCS 集成扩展** (2026-04-01):
- VCS 模块 (`ruci-core/src/vcs.rs`) 实现统一 VCS 抽象层
- 支持 VcsType: Github, Gitlab, Gogs, Custom
- Job 定义支持 VCS 配置字段 (`vcs.url`, `vcs.branch`, `vcs.submodules`)
- 自动代码拉取: Job 执行前自动执行 `git clone/fetch + checkout`
- Webhook 传递 VCS 参数: clone_url, branch, commit_sha
- 各平台 payload 详细解析:
  - GitHub: `repository.clone_url`, `after` (commit SHA), `ref` (branch)
  - GitLab: `project.git_http_url`, `checkout_sha`, `ref`
  - Gogs: `repository.clone_url`, `after`, `ref`
- 支持通过 webhook 参数覆盖 Job 定义的 VCS 配置

**VCS 凭据管理** (2026-04-01):
- 新增 `vcs_credentials` 数据库表存储 VCS 认证凭据
- `VcsCredentialInfo` 结构体: id, name, vcs_type, username, credential
- `WebhookTriggerInfo` 新增 `credential_id` 字段与凭据关联
- `VcsCredentialRepository` trait: upsert, get, list, delete 操作
- SQLite、PostgreSQL 和 MySQL 均已实现

---

## 技术债务

### 1. 错误处理完善
**位置**: 多个模块

**问题**:
- 部分 RPC 方法的错误处理可以更完善
- 错误信息可以更详细

**当前状态**: ✅ 已完成
- QueueResponse 和 JobSubmitResponse 添加了 error_code 和 error_message 字段
- RPC 处理器使用 ErrorCode 枚举返回结构化错误信息
- 消除了 .unwrap() 导致的 panic，改为返回错误响应
- 错误日志包含更详细的上下文信息

---

### 2. 配置热更新
**问题**: 目前修改配置需要重启 daemon。

**当前状态**: ✅ 已完成
- 支持 SIGHUP 信号重新加载配置
- 跟踪配置文件路径，收到 SIGHUP 时重新加载配置
- 实现优雅关闭和重启机制
- 新增 `Config::config_path` 字段存储配置文件路径
- 收到 SIGHUP 后会先优雅关闭服务，再重新加载配置并启动

---

### 3. 优雅关闭与队列恢复
**问题**: 服务停止时，正在执行的作业无法优雅处理；服务重启后队列中的作业丢失。

**当前状态**: ✅ 已完成
- **优雅关闭**:
  - 使用 `GracefulShutdown` 协调器跟踪运行中的作业
  - 收到停止信号后，不再接受新作业，但等待正在执行的作业完成（最多30秒）
  - 超时后强制终止运行中的作业（使用 SIGTERM/SIGKILL）
  - 更新数据库中 RUNNING 状态的作业为 ABORTED
- **队列恢复**:
  - 服务启动时自动从数据库恢复 QUEUED 状态的作业
  - 作业参数（params）持久化到数据库，支持恢复时完整还原
  - 新增 `runs.params` 字段存储 JSON 格式的参数

---

### 4. 并发限制
**问题**: `context.max_parallel` 配置未实际生效。

**当前状态**: ✅ 已完成
- 使用 `tokio::sync::Semaphore` 实现每个 context 的并发控制
- 根据 `max_parallel` 配置初始化每个 context 的信号量
- 作业执行前获取 permit，执行后自动释放
- 支持多个 context 独立并发控制

---

## 文档

### 现有文档

| 文档 | 说明 |
|------|------|
| README.md | 项目介绍、安装、使用说明 |
| API.md | REST API 接口文档 |
| DEPLOY.md | 部署指南（Docker、systemd） |
| DESIGN.md | 架构设计文档 |
| CONTRIBUTING.md | 贡献指南 |

---

## 项目结构

```
/root/works/rucicd/
├── ruci/              # CLI 客户端
├── rucid/             # Daemon 服务端
├── ruci-core/         # 核心库
├── ruci-protocol/     # RPC 协议定义
├── contrib/           # 部署相关文件
│   ├── examples/      # 作业配置示例
│   ├── docker/
│   ├── docker-compose.yml
│   ├── rucid.service
│   ├── ruci.yaml.example
│   └── install.sh
├── docs/              # 文档
│   ├── API.md
│   ├── DEPLOY.md
│   └── DESIGN.md
├── Makefile           # 构建脚本
├── Cargo.toml         # Rust workspace 配置
├── README.md          # 项目介绍
├── CONTRIBUTING.md    # 贡献指南
└── TODO.md            # 本文件
```

---

## 快速开始

```bash
# 构建
make build

# 启动 daemon
./bin/rucid --config contrib/ruci.yaml.example

# 或使用 dev 模式（带 debug 日志）
make dev

# 提交作业
bin/ruci submit run --file test-job.yaml

# 查看状态
bin/ruci status
```

---

## 参考资料

- [Cargo Profile 配置](https://doc.rust-lang.org/cargo/reference/profiles.html)
- [Prometheus metrics 格式](https://prometheus.io/docs/instrumenting/exposition_formats/)
- [axum Web 框架](https://docs.rs/axum/latest/axum/)
