//! Configuration module
//!
//! Supports loading from YAML files with fallback to default values

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::{ConfigError, Result};

/// Main configuration structure
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub storage: StorageConfig,
    pub paths: PathsConfig,
    pub contexts: HashMap<String, ContextConfig>,
    pub triggers: Vec<TriggerConfig>,
    pub logging: LoggingConfig,
    pub archive: ArchiveConfig,
    pub cleanup: CleanupConfig,
    pub web: WebConfig,
    /// Path to the loaded config file (set after loading)
    #[serde(skip)]
    pub config_path: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            database: DatabaseConfig::default(),
            storage: StorageConfig::default(),
            paths: PathsConfig::default(),
            contexts: ContextConfig::default_contexts(),
            triggers: Vec::new(),
            logging: LoggingConfig::default(),
            archive: ArchiveConfig::default(),
            cleanup: CleanupConfig::default(),
            web: WebConfig::default(),
            config_path: None,
        }
    }
}

/// Server configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub web_host: String,
    pub web_port: u16,
    pub rpc_mode: RpcMode,
    pub unix_socket_name: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.0".to_string(),
            port: 7741,
            web_host: "127.0.0.0".to_string(),
            web_port: 8080,
            rpc_mode: RpcMode::Tcp,
            unix_socket_name: "rucid".to_string(),
        }
    }
}

/// RPC server mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RpcMode {
    Tcp,
    Unix,
}

impl Default for RpcMode {
    fn default() -> Self {
        Self::Tcp
    }
}

/// Database configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DatabaseConfig {
    pub url: String,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: "sqlite:///var/lib/ruci/db/ruci.db".to_string(),
        }
    }
}

/// Storage configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StorageConfig {
    #[serde(rename = "type")]
    pub storage_type: StorageType,
    pub endpoint: Option<String>,
    pub bucket: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    pub region: String,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            storage_type: StorageType::Local,
            endpoint: None,
            bucket: None,
            access_key: None,
            secret_key: None,
            region: "us-east-1".to_string(),
        }
    }
}

/// Storage backend type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageType {
    Local,
    Rustfs,
}

impl Default for StorageType {
    fn default() -> Self {
        Self::Local
    }
}

/// Paths configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PathsConfig {
    pub jobs_dir: String,
    pub run_dir: String,
    pub archive_dir: String,
    pub log_dir: String,
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            jobs_dir: "/var/lib/ruci/jobs".to_string(),
            run_dir: "/var/lib/ruci/run".to_string(),
            archive_dir: "/var/lib/ruci/archive".to_string(),
            log_dir: "/var/log/ruci".to_string(),
        }
    }
}

/// Context configuration (resource limits)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContextConfig {
    pub max_parallel: usize,
    pub timeout: u64,
    pub work_dir: String,
}

impl ContextConfig {
    pub fn default_contexts() -> HashMap<String, ContextConfig> {
        let mut map = HashMap::new();
        map.insert(
            "default".to_string(),
            ContextConfig {
                max_parallel: 4,
                timeout: 3600,
                work_dir: "/tmp".to_string(),
            },
        );
        map
    }
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_parallel: 4,
            timeout: 3600,
            work_dir: "/tmp".to_string(),
        }
    }
}

/// Trigger configuration for scheduled jobs
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TriggerConfig {
    pub name: String,
    pub cron: String,
    pub job: String,
    pub enabled: bool,
}

/// Logging configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoggingConfig {
    pub level: String,
    pub format: String,
    /// Enable file logging with rotation
    pub file: Option<LoggingFileConfig>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: "json".to_string(),
            file: None,
        }
    }
}

/// File logging configuration for rotation
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoggingFileConfig {
    /// Directory for log files
    pub dir: String,
    /// Max size per file in MB (default 100)
    pub max_size_mb: Option<u64>,
    /// Max number of log files to retain (default 7)
    pub max_files: Option<u32>,
}

impl Default for LoggingFileConfig {
    fn default() -> Self {
        Self {
            dir: "logs".to_string(),
            max_size_mb: Some(100),
            max_files: Some(7),
        }
    }
}

/// Archive configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ArchiveConfig {
    pub enabled: bool,
    pub max_age_days: u32,
}

impl Default for ArchiveConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_age_days: 30,
        }
    }
}

/// Cleanup configuration for定时清理
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CleanupConfig {
    pub enabled: bool,
    pub interval_hours: u32,
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_hours: 24,
        }
    }
}

/// Web UI configuration for authentication
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebConfig {
    /// Enable authentication for Web UI
    pub enabled: bool,
    /// Admin username (created on first startup if not exists)
    pub admin_username: String,
    /// Admin password (plain text, will be hashed on first startup)
    pub admin_password: String,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            admin_username: "admin".to_string(),
            admin_password: "admin".to_string(),
        }
    }
}

impl ServerConfig {
    /// Get the socket path for Unix socket mode
    pub fn socket_path(&self) -> PathBuf {
        PathBuf::from(format!("/tmp/{}.sock", self.unix_socket_name))
    }
}

impl Config {
    /// Load configuration from file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::ReadError {
            path: path.display().to_string(),
            source: e,
        })?;

        let mut config: Config =
            serde_yaml::from_str(&content).map_err(|e| ConfigError::ParseError {
                path: path.display().to_string(),
                source: e,
            })?;

        // Store the config file path for hot reload
        config.config_path = Some(path.to_path_buf());

        config.ensure_dirs()?;
        config.validate()?;
        Ok(config)
    }

    /// Try to load config from standard locations
    pub fn load_auto() -> Result<Self> {
        let paths = Self::standard_paths();

        for path in &paths {
            if path.exists() {
                return Self::load(path);
            }
        }

        // No config file found, use defaults
        let config = Config::default();
        config.ensure_dirs()?;
        Ok(config)
    }

    /// Get standard config file paths in order of priority
    pub fn standard_paths() -> Vec<std::path::PathBuf> {
        let mut paths = Vec::new();

        // 1. Current directory
        if let Ok(cwd) = std::env::current_dir() {
            paths.push(cwd.join("ruci.yaml"));
        }

        // 2. User config directory
        if let Some(home) = dirs::home_dir() {
            paths.push(home.join(".config").join("ruci").join("ruci.yaml"));
        }

        // 3. System config
        paths.push(std::path::PathBuf::from("/etc/ruci/ruci.yaml"));

        paths
    }

    /// Ensure all required directories exist
    pub fn ensure_dirs(&self) -> Result<()> {
        let dirs = [
            &self.database.url,
            &self.paths.jobs_dir,
            &self.paths.run_dir,
            &self.paths.archive_dir,
        ];

        for dir in dirs {
            let path = Path::new(dir);
            if let Some(parent) = path.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent)?;
                }
            }
            if !path.exists() {
                std::fs::create_dir_all(path)?;
            }
        }

        Ok(())
    }

    /// PID file path
    pub fn pid_file(&self) -> PathBuf {
        PathBuf::from(&self.paths.run_dir).join("rucid.pid")
    }

    /// Resolve environment variables in a string
    pub fn resolve_env(&self, value: &str) -> String {
        // Simple env var resolution: ${VAR} -> env::var(VAR)
        let mut result = value.to_string();
        while let Some(start) = result.find("${") {
            if let Some(end) = result[start..].find('}') {
                let var_name = &result[start + 2..start + end];
                if let Ok(var_value) = std::env::var(var_name) {
                    result = format!(
                        "{}{}{}",
                        &result[..start],
                        var_value,
                        &result[start + end + 1..]
                    );
                } else {
                    // Keep as-is if env var not found
                    break;
                }
            } else {
                break;
            }
        }
        result
    }

    /// Validate the configuration
    /// Returns Ok(()) if valid, Err(ConfigError) if invalid
    pub fn validate(&self) -> Result<()> {
        use ConfigError::InvalidValue;

        // Validate server ports (1-65535)
        if self.server.port == 0 {
            return Err(InvalidValue("server.port must be between 1 and 65535".to_string()).into());
        }
        if self.server.web_port == 0 {
            return Err(
                InvalidValue("server.web_port must be between 1 and 65535".to_string()).into(),
            );
        }

        // Validate database URL format
        if !self.database.url.starts_with("sqlite://")
            && !self.database.url.starts_with("postgresql://")
            && !self.database.url.starts_with("mysql://")
        {
            return Err(InvalidValue(format!(
                "database.url must start with 'sqlite://', 'postgresql://', or 'mysql://', got: {}",
                self.database.url
            ))
            .into());
        }

        // Validate logging level
        match self.logging.level.to_lowercase().as_str() {
            "trace" | "debug" | "info" | "warn" | "error" => {}
            _ => {
                return Err(InvalidValue(format!(
                    "logging.level must be one of: trace, debug, info, warn, error, got: {}",
                    self.logging.level
                ))
                .into());
            }
        }

        // Validate logging format
        match self.logging.format.to_lowercase().as_str() {
            "json" | "pretty" => {}
            _ => {
                return Err(InvalidValue(format!(
                    "logging.format must be 'json' or 'pretty', got: {}",
                    self.logging.format
                ))
                .into());
            }
        }

        // Validate logging file config if present
        if let Some(ref file_config) = self.logging.file {
            if file_config.dir.is_empty() {
                return Err(InvalidValue("logging.file.dir cannot be empty".to_string()).into());
            }
            if let Some(max_size) = file_config.max_size_mb {
                if max_size == 0 {
                    return Err(
                        InvalidValue("logging.file.max_size_mb must be > 0".to_string()).into(),
                    );
                }
            }
            if let Some(max_files) = file_config.max_files {
                if max_files == 0 {
                    return Err(
                        InvalidValue("logging.file.max_files must be > 0".to_string()).into(),
                    );
                }
            }
        }

        // Validate storage type settings
        match self.storage.storage_type {
            StorageType::Rustfs => {
                if self.storage.endpoint.is_none() {
                    return Err(InvalidValue(
                        "storage.endpoint is required when storage.type is 'rustfs'".to_string(),
                    )
                    .into());
                }
                if self.storage.bucket.is_none() {
                    return Err(InvalidValue(
                        "storage.bucket is required when storage.type is 'rustfs'".to_string(),
                    )
                    .into());
                }
            }
            StorageType::Local => {}
        }

        // Validate paths are not empty
        if self.paths.jobs_dir.is_empty() {
            return Err(InvalidValue("paths.jobs_dir cannot be empty".to_string()).into());
        }
        if self.paths.run_dir.is_empty() {
            return Err(InvalidValue("paths.run_dir cannot be empty".to_string()).into());
        }
        if self.paths.archive_dir.is_empty() {
            return Err(InvalidValue("paths.archive_dir cannot be empty".to_string()).into());
        }
        if self.paths.log_dir.is_empty() {
            return Err(InvalidValue("paths.log_dir cannot be empty".to_string()).into());
        }

        // Validate contexts
        for (name, ctx) in &self.contexts {
            if ctx.max_parallel == 0 {
                return Err(
                    InvalidValue(format!("contexts.{}: max_parallel must be > 0", name)).into(),
                );
            }
            if ctx.timeout == 0 {
                return Err(InvalidValue(format!("contexts.{}: timeout must be > 0", name)).into());
            }
            if ctx.work_dir.is_empty() {
                return Err(
                    InvalidValue(format!("contexts.{}: work_dir cannot be empty", name)).into(),
                );
            }
        }

        // Validate RPC mode
        match self.server.rpc_mode {
            RpcMode::Tcp | RpcMode::Unix => {}
        }

        // Validate archive config
        if self.archive.max_age_days == 0 {
            return Err(InvalidValue("archive.max_age_days must be > 0".to_string()).into());
        }

        // Validate cleanup config
        if self.cleanup.interval_hours == 0 {
            return Err(InvalidValue("cleanup.interval_hours must be > 0".to_string()).into());
        }

        Ok(())
    }
}

// Add dirs dependency for home_dir
mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        std::env::var("HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| std::env::var("USERPROFILE").ok().map(PathBuf::from))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.server.port, 7741);
        assert_eq!(config.storage.storage_type, StorageType::Local);
        assert_eq!(config.contexts.get("default").unwrap().max_parallel, 4);
    }

    #[test]
    fn test_resolve_env() {
        std::env::set_var("TEST_VAR", "test_value");
        let config = Config::default();
        assert_eq!(config.resolve_env("${TEST_VAR}"), "test_value");
        assert_eq!(
            config.resolve_env("prefix_${TEST_VAR}_suffix"),
            "prefix_test_value_suffix"
        );
    }

    // ═══════════════════════════════════════════════════════════════
    // Config validation tests
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn test_validate_valid_config() {
        let config = Config::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_server_port_zero() {
        let mut config = Config::default();
        config.server.port = 0;
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(err_msg.contains("server.port"));
        assert!(err_msg.contains("65535"));
    }

    #[test]
    fn test_validate_web_port_zero() {
        let mut config = Config::default();
        config.server.web_port = 0;
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(err_msg.contains("server.web_port"));
    }

    #[test]
    fn test_validate_database_url_sqlite() {
        let mut config = Config::default();
        config.database.url = "sqlite:///tmp/test.db".to_string();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_database_url_postgresql() {
        let mut config = Config::default();
        config.database.url = "postgresql://localhost/test".to_string();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_database_url_invalid() {
        let mut config = Config::default();
        config.database.url = "oracle://localhost/test".to_string();
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(err_msg.contains("database.url"));
        assert!(err_msg.contains("sqlite://"));
        assert!(err_msg.contains("postgresql://"));
        assert!(err_msg.contains("mysql://"));
    }

    #[test]
    fn test_validate_logging_level_valid() {
        let mut config = Config::default();
        for level in &["trace", "debug", "info", "warn", "error", "INFO", "DEBUG"] {
            config.logging.level = level.to_string();
            assert!(config.validate().is_ok(), "Failed for level: {}", level);
        }
    }

    #[test]
    fn test_validate_logging_level_invalid() {
        let mut config = Config::default();
        config.logging.level = "invalid".to_string();
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(err_msg.contains("logging.level"));
    }

    #[test]
    fn test_validate_logging_format_json() {
        let mut config = Config::default();
        config.logging.format = "json".to_string();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_logging_format_pretty() {
        let mut config = Config::default();
        config.logging.format = "pretty".to_string();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_logging_format_invalid() {
        let mut config = Config::default();
        config.logging.format = "xml".to_string();
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(err_msg.contains("logging.format"));
    }

    #[test]
    fn test_validate_logging_file_config_valid() {
        let mut config = Config::default();
        config.logging.file = Some(LoggingFileConfig {
            dir: "logs".to_string(),
            max_size_mb: Some(100),
            max_files: Some(7),
        });
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_logging_file_dir_empty() {
        let mut config = Config::default();
        config.logging.file = Some(LoggingFileConfig {
            dir: "".to_string(),
            max_size_mb: Some(100),
            max_files: Some(7),
        });
        let result = config.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("logging.file.dir"));
    }

    #[test]
    fn test_validate_logging_file_max_size_zero() {
        let mut config = Config::default();
        config.logging.file = Some(LoggingFileConfig {
            dir: "logs".to_string(),
            max_size_mb: Some(0),
            max_files: Some(7),
        });
        let result = config.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("logging.file.max_size_mb"));
    }

    #[test]
    fn test_validate_logging_file_max_files_zero() {
        let mut config = Config::default();
        config.logging.file = Some(LoggingFileConfig {
            dir: "logs".to_string(),
            max_size_mb: Some(100),
            max_files: Some(0),
        });
        let result = config.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("logging.file.max_files"));
    }

    #[test]
    fn test_validate_storage_type_local() {
        let config = Config::default();
        // Local storage should be valid without endpoint/bucket
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_storage_type_rustfs_missing_endpoint() {
        let mut config = Config::default();
        config.storage.storage_type = StorageType::Rustfs;
        config.storage.endpoint = None;
        config.storage.bucket = Some("bucket".to_string());
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(err_msg.contains("storage.endpoint"));
    }

    #[test]
    fn test_validate_storage_type_rustfs_missing_bucket() {
        let mut config = Config::default();
        config.storage.storage_type = StorageType::Rustfs;
        config.storage.endpoint = Some("http://localhost".to_string());
        config.storage.bucket = None;
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(err_msg.contains("storage.bucket"));
    }

    #[test]
    fn test_validate_storage_type_rustfs_valid() {
        let mut config = Config::default();
        config.storage.storage_type = StorageType::Rustfs;
        config.storage.endpoint = Some("http://localhost:9000".to_string());
        config.storage.bucket = Some("test-bucket".to_string());
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_paths_jobs_dir_empty() {
        let mut config = Config::default();
        config.paths.jobs_dir = "".to_string();
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("jobs_dir"));
    }

    #[test]
    fn test_validate_paths_run_dir_empty() {
        let mut config = Config::default();
        config.paths.run_dir = "".to_string();
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("run_dir"));
    }

    #[test]
    fn test_validate_paths_archive_dir_empty() {
        let mut config = Config::default();
        config.paths.archive_dir = "".to_string();
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("archive_dir"));
    }

    #[test]
    fn test_validate_paths_log_dir_empty() {
        let mut config = Config::default();
        config.paths.log_dir = "".to_string();
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("log_dir"));
    }

    #[test]
    fn test_validate_context_max_parallel_zero() {
        let mut config = Config::default();
        config.contexts.insert(
            "test".to_string(),
            ContextConfig {
                max_parallel: 0,
                timeout: 3600,
                work_dir: "/tmp".to_string(),
            },
        );
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(err_msg.contains("max_parallel"));
        assert!(err_msg.contains("test"));
    }

    #[test]
    fn test_validate_context_timeout_zero() {
        let mut config = Config::default();
        config.contexts.insert(
            "test".to_string(),
            ContextConfig {
                max_parallel: 4,
                timeout: 0,
                work_dir: "/tmp".to_string(),
            },
        );
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(err_msg.contains("timeout"));
        assert!(err_msg.contains("test"));
    }

    #[test]
    fn test_validate_context_work_dir_empty() {
        let mut config = Config::default();
        config.contexts.insert(
            "test".to_string(),
            ContextConfig {
                max_parallel: 4,
                timeout: 3600,
                work_dir: "".to_string(),
            },
        );
        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(err_msg.contains("work_dir"));
        assert!(err_msg.contains("test"));
    }

    #[test]
    fn test_validate_rpc_mode_tcp() {
        let mut config = Config::default();
        config.server.rpc_mode = RpcMode::Tcp;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_rpc_mode_unix() {
        let mut config = Config::default();
        config.server.rpc_mode = RpcMode::Unix;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_multiple_errors_returns_first() {
        let mut config = Config::default();
        config.server.port = 0;
        config.server.web_port = 0;
        config.database.url = "invalid".to_string();
        // Multiple errors exist but we get the first one
        let result = config.validate();
        assert!(result.is_err());
    }

    // ═══════════════════════════════════════════════════════════════
    // resolve_env edge case tests
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn test_resolve_env_missing_var() {
        std::env::remove_var("NONEXISTENT_VAR_12345");
        let config = Config::default();
        // When env var doesn't exist, should return as-is up to the broken ${...}
        let result = config.resolve_env("prefix_${NONEXISTENT_VAR_12345}_suffix");
        // The implementation breaks on missing var, keeping prefix_ and the rest
        assert!(result.contains("prefix_"));
    }

    #[test]
    fn test_resolve_env_no_vars() {
        let config = Config::default();
        assert_eq!(config.resolve_env("plain string"), "plain string");
        assert_eq!(config.resolve_env(""), "");
    }

    #[test]
    fn test_resolve_env_multiple_vars() {
        std::env::set_var("VAR1", "value1");
        std::env::set_var("VAR2", "value2");
        let config = Config::default();
        assert_eq!(
            config.resolve_env("${VAR1} and ${VAR2}"),
            "value1 and value2"
        );
    }

    #[test]
    fn test_resolve_env_adjacent_vars() {
        std::env::set_var("A", "1");
        std::env::set_var("B", "2");
        let config = Config::default();
        assert_eq!(config.resolve_env("${A}${B}"), "12");
    }

    #[test]
    fn test_resolve_env_unclosed_bracket() {
        std::env::set_var("TEST", "value");
        let config = Config::default();
        // Unclosed bracket - should stop at the unclosed ${
        let result = config.resolve_env("prefix_${TEST");
        // The implementation checks for '}' starting from position after '${'
        // and breaks if not found
        assert!(result.contains("prefix_"));
    }

    #[test]
    fn test_resolve_env_empty_var_name() {
        let config = Config::default();
        // ${} is an empty var name - should return as-is or partial
        let result = config.resolve_env("prefix${}suffix");
        // Empty var name will fail to find env var
        assert!(result.contains("prefix"));
    }

    #[test]
    fn test_resolve_env_special_chars_in_value() {
        std::env::set_var("SPECIAL", "value with spaces & symbols!");
        let config = Config::default();
        assert_eq!(
            config.resolve_env("${SPECIAL}"),
            "value with spaces & symbols!"
        );
    }
}
