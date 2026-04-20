# Contributing to Ruci CD

感谢您对 Ruci CD 项目的贡献！本文档提供了开发指南和代码规范。

## 目录

- [开发环境设置](#开发环境设置)
- [代码规范](#代码规范)
- [提交规范](#提交规范)
- [测试](#测试)
- [文档](#文档)
- [Pull Request 流程](#pull-request-流程)

---

## 开发环境设置

### 前置要求

- Rust 1.90+
- Cargo
- Git

### 常用命令

```bash
# 构建
make build          # Release 构建
make build-dev      # Dev 构建

# 测试
make test           # 运行测试
make test-all       # 测试 + fmt + clippy

# 代码格式
make fmt            # 格式化代码
make fmt-check      # 检查格式

# Lint
make clippy         # 运行 clippy

# 清理
make clean          # 清理构建产物
```

---

## 代码规范

### Rust 代码规范

遵循 Rust 官方代码规范和 `rustfmt` 配置：

1. **格式化**: 运行 `cargo fmt --all` 格式化代码
2. **Clippy**: 确保 `cargo clippy --all --all-targets -- -D warnings` 无警告
3. **文档注释**: 为公开 API 添加文档注释 `///`
4. **安全性**: 避免使用 `unwrap()`，使用 `?` 操作符处理错误

### 命名规范

| 类型 | 规范 | 示例 |
|------|------|------|
| 模块 | snake_case | `my_module` |
| 结构体 | PascalCase | `struct MyStruct` |
| 枚举 | PascalCase | `enum MyEnum` |
| 函数 | snake_case | `fn my_function` |
| 变量 | snake_case | `let my_var` |
| 常量 | SCREAMING_SNAKE_CASE | `const MAX_SIZE` |
| 类型 | PascalCase | `type MyResult` |

### 错误处理

```rust
// ✅ 推荐：使用 ? 操作符
fn load_config() -> Result<Config, ConfigError> {
    let content = std::fs::read_to_string(&path)?;
    let config = yaml_serde::from_str(&content)?;
    Ok(config)
}

// ❌ 避免：使用 unwrap()
fn load_config() -> Config {
    let content = std::fs::read_to_string(&path).unwrap()
}
```

### 异步代码

```rust
// ✅ 推荐：使用 async_trait
#[async_trait]
pub trait Executor: Send + Sync {
    async fn execute(&self, ctx: &ExecutionContext) -> Result<ExecutionResult>;
}

// ✅ 使用 tokio 的特性
#[tokio::test]
async fn test_executor() {
    // test code
}
```

---

## 提交规范

### 提交信息格式

```
<类型>(<范围>): <描述>

[可选的详细说明]
```

### 类型

| 类型 | 说明 |
|------|------|
| `feat` | 新功能 |
| `fix` | Bug 修复 |
| `docs` | 文档更新 |
| `style` | 代码格式（不影响功能） |
| `refactor` | 重构（不影响功能） |
| `perf` | 性能优化 |
| `test` | 添加测试 |
| `chore` | 构建/工具变更 |

### 范围

| 范围 | 说明 |
|------|------|
| `core` | ruci-core 核心库 |
| `rpc` | RPC 协议相关 |
| `api` | REST API 相关 |
| `ui` | Web UI 相关 |
| `cli` | CLI 客户端 |
| `db` | 数据库相关 |
| `config` | 配置系统 |
| `auth` | 认证相关 |
| `vcs` | VCS 相关 |

### 示例

```
feat(core): 添加作业归档功能

- 实现 ArchiveManager
- 添加定期清理过期归档
- 集成到 AppContext

Closes #123
```

```
fix(rpc): 修复 queue_job 返回错误的问题

当数据库连接失败时，应该返回结构化错误而不是 panic
```

```
docs(api): 更新 REST API 文档

添加新的 webhook 端点说明
```

---

## 测试

### 运行测试

```bash
# 运行所有测试
cargo test --all

# 运行特定模块测试
cargo test -p ruci-core

# 运行集成测试
cargo test --test integration

# 查看测试覆盖率
cargo tarpaulin --all
```

### 测试规范

1. **单元测试**: 每个公开函数应考虑添加单元测试
2. **集成测试**: 放在 `tests/` 目录
3. **测试命名**: `#[test] fn test_<function_name>_<scenario>()`
4. **断言信息**: 使用有意义的断言信息

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_load_valid_file() {
        let config = Config::load("test-data/valid.yaml").unwrap();
        assert_eq!(config.server.port, 7741, "Default port should be 7741");
    }

    #[test]
    fn test_config_load_missing_file() {
        let result = Config::load("nonexistent.yaml");
        assert!(result.is_err(), "Should return error for missing file");
    }
}
```

---

## 文档

### 代码文档

为所有公开 API 添加文档注释：

```rust
/// 配置加载与验证模块
///
/// # Example
///
/// ```
/// let config = Config::load("ruci.yaml")?;
/// ```
pub mod config {
    /// 从指定路径加载配置
    ///
    /// # Arguments
    ///
    /// * `path` - 配置文件路径
    ///
    /// # Errors
    ///
    /// 如果文件不存在或格式错误，返回 `ConfigError`
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Config> {
        // ...
    }
}
```

### 更新文档

如果你的更改影响了用户，需要更新相应文档：

- **新功能**: 更新 `README.md`
- **API 变更**: 更新 `docs/API.md`
- **部署变更**: 更新 `docs/DEPLOY.md`
- **设计变更**: 更新 `docs/DESIGN.md`

---

## Pull Request 流程

### 1. Fork & Clone

```bash
# Fork 仓库
# Clone 你的 fork
git clone https://github.com/<your-username>/ruci.git
cd ruci

# 添加上游仓库
git remote add upstream https://github.com/gnuos/ruci.git
```

### 2. 创建分支

```bash
# 从 main 创建新分支
git checkout -b feat/my-new-feature

# 或者修复 bug
git checkout -b fix/issue-description
```

### 3. 开发 & 提交

```bash
# 进行更改
git add .
git commit -m "feat(core): 添加新功能"

# 推送到你的 fork
git push origin feat/my-new-feature
```

### 4. 创建 Pull Request

1. 打开 GitHub 仓库
2. 点击 "New Pull Request"
3. 选择你的分支
4. 填写 PR 描述：
   - 描述解决的问题或添加的功能
   - 包含相关的 issue 编号
   - 说明测试方式

### 5. PR 检查清单

- [ ] 代码遵循 Rust 规范
- [ ] `cargo fmt` 已运行
- [ ] `cargo clippy` 无警告
- [ ] `cargo test --all` 通过
- [ ] 文档已更新
- [ ] 添加了必要的测试
- [ ] 提交信息符合规范

### 合并后

```bash
# 切换回 main
git checkout main

# 拉取上游更改
git pull upstream main

# 删除已合并的分支
git branch -d feat/my-new-feature
git push origin --delete feat/my-new-feature
```

---

## 开发工作流

### 快速迭代

```bash
# 1. 修改代码
# 2. 运行测试
cargo test -p <affected-crate>

# 3. 格式化
cargo fmt --all

# 4. Clippy 检查
cargo clippy --all-targets -- -D warnings

# 5. 构建
cargo build

# 6. 本地测试
make dev
```

### 大改动

如果改动涉及超过 4 个文件，先创建 issue 讨论：

1. 描述你想做什么
2. 说明实现方案
3. 讨论潜在影响

---

## 获取帮助

- [GitHub Issues](https://github.com/gnuos/ruci/issues)
- [GitHub Discussions](https://github.com/gnuos/ruci/discussions)

---

## 许可证

通过贡献代码，您同意您的贡献将按照项目的 MIT OR Apache-2.0 许可证授权。
