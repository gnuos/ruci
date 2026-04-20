//! Error types for Ruci
//!
//! Uses thiserror for clean error definitions with source chaining

use thiserror::Error;

/// Result type alias using RuciError
pub type Result<T> = std::result::Result<T, Error>;

/// Main error enum for Ruci
///
/// Organized by subsystem for easy error categorization and future extension
#[derive(Error, Debug)]
pub enum Error {
    // ─────────────────────────────────────────────────────────────
    // Config errors
    // ─────────────────────────────────────────────────────────────
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    // ─────────────────────────────────────────────────────────────
    // Database errors
    // ─────────────────────────────────────────────────────────────
    #[error("Database error: {0}")]
    Database(#[from] DbError),

    // ─────────────────────────────────────────────────────────────
    // Queue errors
    // ─────────────────────────────────────────────────────────────
    #[error("Queue error: {0}")]
    Queue(#[from] QueueError),

    // ─────────────────────────────────────────────────────────────
    // Executor errors
    // ─────────────────────────────────────────────────────────────
    #[error("Executor error: {0}")]
    Executor(#[from] ExecutorError),

    // ─────────────────────────────────────────────────────────────
    // Storage errors
    // ─────────────────────────────────────────────────────────────
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    // ─────────────────────────────────────────────────────────────
    // RPC errors
    // ─────────────────────────────────────────────────────────────
    #[error("RPC error: {0}")]
    Rpc(#[from] RpcError),

    // ─────────────────────────────────────────────────────────────
    // Business logic errors
    // ─────────────────────────────────────────────────────────────
    #[error("Job not found: {0}")]
    JobNotFound(String),

    #[error("Run not found: {0}")]
    RunNotFound(String),

    #[error("Artifact not found: {0}")]
    ArtifactNotFound(String),

    #[error("Job '{0}' is already running")]
    JobAlreadyRunning(String),

    #[error("Invalid parameters: {0}")]
    InvalidParams(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    Parse(#[from] yaml_serde::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Other error: {0}")]
    Other(String),
}

/// Configuration errors
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to read config file '{path}': {source}")]
    ReadError {
        path: String,
        source: std::io::Error,
    },

    #[error("Failed to parse config file '{path}': {source}")]
    ParseError {
        path: String,
        source: yaml_serde::Error,
    },

    #[error("Config file not found: {paths:?}")]
    NotFound { paths: Vec<String> },

    #[error("Invalid config value: {0}")]
    InvalidValue(String),

    #[error("Environment variable not set: {0}")]
    EnvVarNotSet(String),
}

/// Database errors
#[derive(Error, Debug)]
pub enum DbError {
    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Query error: {0}")]
    Query(String),

    #[error("Transaction error: {0}")]
    Transaction(String),

    #[error("Migration error: {0}")]
    Migration(String),
}

/// Queue errors
#[derive(Error, Debug)]
pub enum QueueError {
    #[error("Failed to send to queue: {0}")]
    SendFailed(String),

    #[error("Queue receiver dropped")]
    ReceiverDropped,

    #[error("Queue is full")]
    Full,
}

/// Executor errors
#[derive(Error, Debug)]
pub enum ExecutorError {
    #[error("Failed to spawn process: {0}")]
    SpawnFailed(String),

    #[error("Process exited with code {code}: {output}")]
    ProcessExited { code: i32, output: String },

    #[error("Timeout after {seconds} seconds")]
    Timeout { seconds: u64 },

    #[error("Job aborted")]
    Aborted,

    #[error("Context not found: {0}")]
    ContextNotFound(String),

    #[error("Invalid step definition: {0}")]
    InvalidStep(String),
}

/// Storage errors
#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Local storage error: {0}")]
    Local(String),

    #[error("S3 error: {0}")]
    S3(String),

    #[error("File not found: {0}")]
    NotFound(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Upload failed: {0}")]
    UploadFailed(String),

    #[error("Download failed: {0}")]
    DownloadFailed(String),
}

/// RPC errors
#[derive(Error, Debug)]
pub enum RpcError {
    #[error("Server error: {0}")]
    Server(String),

    #[error("Client error: {0}")]
    Client(String),

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Codec error: {0}")]
    Codec(String),

    #[error("Timeout")]
    Timeout,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════════════════════════════
    // Error Display tests
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn test_config_error_display() {
        let err = ConfigError::ReadError {
            path: "/etc/ruci.yaml".to_string(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"),
        };
        let display = err.to_string();
        assert!(display.contains("/etc/ruci.yaml"));
        assert!(display.contains("Failed to read config file"));
    }

    #[test]
    fn test_config_error_parse_display() {
        let yaml_err = yaml_serde::from_str::<crate::Config>("invalid: [").unwrap_err();
        let err = ConfigError::ParseError {
            path: "test.yaml".to_string(),
            source: yaml_err,
        };
        let display = err.to_string();
        assert!(display.contains("test.yaml"));
        assert!(display.contains("Failed to parse config file"));
    }

    #[test]
    fn test_config_error_not_found_display() {
        let err = ConfigError::NotFound {
            paths: vec!["/a.yaml".to_string(), "/b.yaml".to_string()],
        };
        let display = err.to_string();
        assert!(display.contains("/a.yaml"));
        assert!(display.contains("/b.yaml"));
    }

    #[test]
    fn test_config_error_invalid_value_display() {
        let err = ConfigError::InvalidValue("server.port must be > 0".to_string());
        assert_eq!(
            err.to_string(),
            "Invalid config value: server.port must be > 0"
        );
    }

    #[test]
    fn test_config_error_env_var_not_set_display() {
        let err = ConfigError::EnvVarNotSet("HOME".to_string());
        assert_eq!(err.to_string(), "Environment variable not set: HOME");
    }

    #[test]
    fn test_db_error_display() {
        let err = DbError::Connection("failed to connect".to_string());
        assert_eq!(err.to_string(), "Connection error: failed to connect");

        let err = DbError::Query("syntax error".to_string());
        assert_eq!(err.to_string(), "Query error: syntax error");

        let err = DbError::Transaction("deadlock".to_string());
        assert_eq!(err.to_string(), "Transaction error: deadlock");

        let err = DbError::Migration("version mismatch".to_string());
        assert_eq!(err.to_string(), "Migration error: version mismatch");
    }

    #[test]
    fn test_queue_error_display() {
        let err = QueueError::SendFailed("channel closed".to_string());
        assert_eq!(err.to_string(), "Failed to send to queue: channel closed");

        let err = QueueError::ReceiverDropped;
        assert_eq!(err.to_string(), "Queue receiver dropped");

        let err = QueueError::Full;
        assert_eq!(err.to_string(), "Queue is full");
    }

    #[test]
    fn test_executor_error_display() {
        let err = ExecutorError::SpawnFailed("command not found".to_string());
        assert_eq!(
            err.to_string(),
            "Failed to spawn process: command not found"
        );

        let err = ExecutorError::ProcessExited {
            code: 1,
            output: "error".to_string(),
        };
        assert!(err.to_string().contains("1"));
        assert!(err.to_string().contains("error"));

        let err = ExecutorError::Timeout { seconds: 300 };
        assert_eq!(err.to_string(), "Timeout after 300 seconds");

        let err = ExecutorError::Aborted;
        assert_eq!(err.to_string(), "Job aborted");

        let err = ExecutorError::ContextNotFound("docker".to_string());
        assert_eq!(err.to_string(), "Context not found: docker");

        let err = ExecutorError::InvalidStep("missing command".to_string());
        assert_eq!(err.to_string(), "Invalid step definition: missing command");
    }

    #[test]
    fn test_storage_error_display() {
        let err = StorageError::Local("disk full".to_string());
        assert_eq!(err.to_string(), "Local storage error: disk full");

        let err = StorageError::S3("access denied".to_string());
        assert_eq!(err.to_string(), "S3 error: access denied");

        let err = StorageError::NotFound("/tmp/file".to_string());
        assert_eq!(err.to_string(), "File not found: /tmp/file");

        let err = StorageError::PermissionDenied("/etc/passwd".to_string());
        assert_eq!(err.to_string(), "Permission denied: /etc/passwd");

        let err = StorageError::UploadFailed("network error".to_string());
        assert_eq!(err.to_string(), "Upload failed: network error");

        let err = StorageError::DownloadFailed("timeout".to_string());
        assert_eq!(err.to_string(), "Download failed: timeout");
    }

    #[test]
    fn test_rpc_error_display() {
        let err = RpcError::Server("internal error".to_string());
        assert_eq!(err.to_string(), "Server error: internal error");

        let err = RpcError::Client("bad request".to_string());
        assert_eq!(err.to_string(), "Client error: bad request");

        let err = RpcError::ConnectionFailed("refused".to_string());
        assert_eq!(err.to_string(), "Connection failed: refused");

        let err = RpcError::Codec("invalid encoding".to_string());
        assert_eq!(err.to_string(), "Codec error: invalid encoding");

        let err = RpcError::Timeout;
        assert_eq!(err.to_string(), "Timeout");
    }

    #[test]
    fn test_main_error_display() {
        let err = crate::Error::JobNotFound("job-123".to_string());
        assert_eq!(err.to_string(), "Job not found: job-123");

        let err = crate::Error::RunNotFound("run-456".to_string());
        assert_eq!(err.to_string(), "Run not found: run-456");

        let err = crate::Error::ArtifactNotFound("artifact-789".to_string());
        assert_eq!(err.to_string(), "Artifact not found: artifact-789");

        let err = crate::Error::JobAlreadyRunning("deploy".to_string());
        assert_eq!(err.to_string(), "Job 'deploy' is already running");

        let err = crate::Error::InvalidParams("missing field".to_string());
        assert_eq!(err.to_string(), "Invalid parameters: missing field");
    }

    // ═══════════════════════════════════════════════════════════════
    // Error source chaining tests
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn test_error_from_config_error() {
        let config_err = ConfigError::InvalidValue("test".to_string());
        let err: crate::Error = config_err.into();
        match err {
            crate::Error::Config(_) => {}
            _ => panic!("Expected crate::Error::Config"),
        }
    }

    #[test]
    fn test_error_from_db_error() {
        let db_err = DbError::Connection("test".to_string());
        let err: crate::Error = db_err.into();
        match err {
            crate::Error::Database(_) => {}
            _ => panic!("Expected crate::Error::Database"),
        }
    }

    #[test]
    fn test_error_from_queue_error() {
        let queue_err = QueueError::Full;
        let err: crate::Error = queue_err.into();
        match err {
            crate::Error::Queue(_) => {}
            _ => panic!("Expected crate::Error::Queue"),
        }
    }

    #[test]
    fn test_error_from_executor_error() {
        let exec_err = ExecutorError::Aborted;
        let err: crate::Error = exec_err.into();
        match err {
            crate::Error::Executor(_) => {}
            _ => panic!("Expected crate::Error::Executor"),
        }
    }

    #[test]
    fn test_error_from_storage_error() {
        let storage_err = StorageError::NotFound("test".to_string());
        let err: crate::Error = storage_err.into();
        match err {
            crate::Error::Storage(_) => {}
            _ => panic!("Expected crate::Error::Storage"),
        }
    }

    #[test]
    fn test_error_from_rpc_error() {
        let rpc_err = RpcError::Timeout;
        let err: crate::Error = rpc_err.into();
        match err {
            crate::Error::Rpc(_) => {}
            _ => panic!("Expected crate::Error::Rpc"),
        }
    }

    #[test]
    fn test_error_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: crate::Error = io_err.into();
        match err {
            crate::Error::Io(_) => {}
            _ => panic!("Expected crate::Error::Io"),
        }
    }

    #[test]
    fn test_error_from_parse_error() {
        let yaml_err = yaml_serde::from_str::<crate::Config>("invalid: [").unwrap_err();
        let err: crate::Error = yaml_err.into();
        match err {
            crate::Error::Parse(_) => {}
            _ => panic!("Expected crate::Error::Parse"),
        }
    }

    #[test]
    fn test_error_from_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid json").unwrap_err();
        let err: crate::Error = json_err.into();
        match err {
            crate::Error::Json(_) => {}
            _ => panic!("Expected crate::Error::Json"),
        }
    }

    // ═══════════════════════════════════════════════════════════════
    // Error matching tests
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn test_error_chain_config_error() {
        let config_err = ConfigError::InvalidValue("port must be positive".to_string());
        let err: crate::Error = config_err.into();

        // crate::Error implements std::error::Error
        // With #[from] on Config variant, source() returns Some(ConfigError)
        // because the ConfigError itself becomes the source
        let source = std::error::Error::source(&err);
        assert!(source.is_some());
    }

    #[test]
    fn test_error_chain_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let err: crate::Error = io_err.into();

        // Io error should have source (the io_err itself asdyn Error)
        let source = std::error::Error::source(&err);
        assert!(source.is_some());
    }

    // ═══════════════════════════════════════════════════════════════
    // Result type alias tests
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn test_result_type_ok() {
        let result: Result<i32> = Ok(42);
        assert!(result.is_ok());
        assert_eq!(result.ok().unwrap(), 42);
    }

    #[test]
    fn test_result_type_err() {
        let err = ConfigError::InvalidValue("test".to_string());
        let result: Result<i32> = Err(err.into());
        assert!(result.is_err());
    }

    // ═══════════════════════════════════════════════════════════════
    // Business error source chain tests
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn test_job_not_found_error() {
        let err = crate::Error::JobNotFound("job-123".to_string());
        assert_eq!(err.to_string(), "Job not found: job-123");

        // Business errors don't have a source
        let source = std::error::Error::source(&err);
        assert!(source.is_none());
    }

    #[test]
    fn test_run_not_found_error() {
        let err = crate::Error::RunNotFound("run-456".to_string());
        assert_eq!(err.to_string(), "Run not found: run-456");

        let source = std::error::Error::source(&err);
        assert!(source.is_none());
    }

    #[test]
    fn test_artifact_not_found_error() {
        let err = crate::Error::ArtifactNotFound("artifact-789".to_string());
        assert_eq!(err.to_string(), "Artifact not found: artifact-789");

        let source = std::error::Error::source(&err);
        assert!(source.is_none());
    }

    #[test]
    fn test_job_already_running_error() {
        let err = crate::Error::JobAlreadyRunning("deploy".to_string());
        assert_eq!(err.to_string(), "Job 'deploy' is already running");

        let source = std::error::Error::source(&err);
        assert!(source.is_none());
    }

    #[test]
    fn test_invalid_params_error() {
        let err = crate::Error::InvalidParams("missing field 'name'".to_string());
        assert_eq!(err.to_string(), "Invalid parameters: missing field 'name'");

        let source = std::error::Error::source(&err);
        assert!(source.is_none());
    }

    #[test]
    fn test_error_display_consistency() {
        // All error types should produce non-empty display strings
        let errors: Vec<Box<dyn std::error::Error>> = vec![
            Box::new(ConfigError::InvalidValue("test".to_string())),
            Box::new(DbError::Connection("test".to_string())),
            Box::new(QueueError::SendFailed("test".to_string())),
            Box::new(ExecutorError::Aborted),
            Box::new(StorageError::NotFound("test".to_string())),
            Box::new(RpcError::Timeout),
            Box::new(crate::Error::JobNotFound("test".to_string())),
        ];

        for err in errors {
            let display = err.to_string();
            assert!(!display.is_empty(), "Error display should not be empty");
        }
    }

    #[test]
    fn test_error_debug_consistency() {
        // Debug and Display should both work for all error types
        let config_err = ConfigError::InvalidValue("test".to_string());
        let _debug = format!("{:?}", config_err);
        let _display = config_err.to_string();

        let main_err: crate::Error = config_err.into();
        let _debug = format!("{:?}", main_err);
        let _display = main_err.to_string();
    }

    #[test]
    fn test_error_chain_io_error_with_context() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: crate::Error = io_err.into();

        // Error should be Io variant
        match err {
            crate::Error::Io(_) => {}
            _ => panic!("Expected Error::Io"),
        }

        // Should have a source (the io error itself)
        let source = std::error::Error::source(&err);
        assert!(source.is_some());
    }

    #[test]
    fn test_error_chain_parse_error_with_context() {
        let yaml_err = yaml_serde::from_str::<crate::Config>("invalid: [").unwrap_err();
        let err: crate::Error = yaml_err.into();

        // Error should be Parse variant
        match err {
            crate::Error::Parse(_) => {}
            _ => panic!("Expected Error::Parse"),
        }

        // Should have a source
        let source = std::error::Error::source(&err);
        assert!(source.is_some());
    }

    #[test]
    fn test_error_chain_json_error_with_context() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid").unwrap_err();
        let err: crate::Error = json_err.into();

        match err {
            crate::Error::Json(_) => {}
            _ => panic!("Expected Error::Json"),
        }

        let source = std::error::Error::source(&err);
        assert!(source.is_some());
    }
}
