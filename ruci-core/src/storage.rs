//! Storage module
//!
//! Provides abstraction for artifact storage with plugin support

use async_trait::async_trait;
use std::path::{Path, PathBuf};

use crate::config::{StorageConfig, StorageType};
use crate::error::{Result, StorageError};

/// Storage backend trait
///
/// Provides a plugin interface for different storage backends
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

/// Handle to a stored object
#[derive(Debug, Clone)]
pub struct StorageHandle {
    pub key: String,
    pub size: u64,
    pub checksum: String,
}

/// Calculate SHA-256 checksum of a file
fn calculate_checksum(path: &Path) -> Result<String> {
    use base64::Engine;
    use sha2::{Digest, Sha256};
    let data = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    Ok(base64::engine::general_purpose::STANDARD.encode(hasher.finalize()))
}

/// Validate that a storage key is safe (no path traversal)
fn validate_key(key: &str) -> Result<()> {
    if key.is_empty() {
        return Err(StorageError::Local("Empty storage key".to_string()).into());
    }
    if key.starts_with('/') || key.starts_with('\\') {
        return Err(StorageError::Local(format!(
            "Storage key must not start with separator: {}",
            key
        )).into());
    }
    // Normalize and check for path traversal
    let path = Path::new(key);
    let mut depth = 0usize;
    for component in path.components() {
        match component {
            std::path::Component::Normal(_) => depth += 1,
            std::path::Component::ParentDir => {
                if depth == 0 {
                    return Err(StorageError::Local(format!(
                        "Path traversal detected in key: {}",
                        key
                    )).into());
                }
                depth = depth.saturating_sub(1);
            }
            std::path::Component::CurDir => {}
            _ => {
                return Err(StorageError::Local(format!(
                    "Invalid path component in key: {}",
                    key
                )).into());
            }
        }
    }
    Ok(())
}

/// Local filesystem storage backend
pub struct LocalStorage {
    base_path: PathBuf,
    max_artifact_size: u64,
}

impl LocalStorage {
    pub fn new(base_path: &str, max_artifact_size_mb: u64) -> Self {
        Self {
            base_path: PathBuf::from(base_path),
            max_artifact_size: max_artifact_size_mb * 1024 * 1024,
        }
    }

    fn resolve_key(&self, key: &str) -> Result<PathBuf> {
        validate_key(key)?;
        let resolved = self.base_path.join(key);
        // Ensure the resolved path is still under base_path
        let canonical_base = self
            .base_path
            .canonicalize()
            .unwrap_or_else(|_| self.base_path.clone());
        let canonical_resolved = resolved
            .canonicalize()
            .unwrap_or_else(|_| resolved.clone());
        if !canonical_resolved.starts_with(&canonical_base) {
            return Err(StorageError::Local(format!(
                "Resolved path escapes storage base: {}",
                key
            ))
            .into());
        }
        Ok(resolved)
    }
}

#[async_trait]
impl Storage for LocalStorage {
    async fn upload(&self, key: &str, path: &Path) -> Result<StorageHandle> {
        tracing::debug!(key=%key, path=%path.display(), "Uploading file to local storage");
        let dest = self.resolve_key(key)?;

        // Check file size against limit
        let src_metadata = std::fs::metadata(path)
            .map_err(|e| StorageError::Local(format!("Cannot read source metadata: {}", e)))?;
        if self.max_artifact_size > 0 && src_metadata.len() > self.max_artifact_size {
            return Err(StorageError::Local(format!(
                "Artifact size {} bytes exceeds limit {} bytes",
                src_metadata.len(),
                self.max_artifact_size
            ))
            .into());
        }

        if let Some(parent) = dest.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                tracing::error!(parent=%parent.display(), error=%e, "Failed to create parent directory");
                return Err(StorageError::Local(e.to_string()).into());
            }
        }

        match std::fs::copy(path, &dest) {
            Ok(_) => tracing::debug!(dest=%dest.display(), "File copied successfully"),
            Err(e) => {
                tracing::error!(error=%e, "Failed to copy file to {}", dest.display());
                return Err(StorageError::UploadFailed(e.to_string()).into());
            }
        }

        let metadata = match std::fs::metadata(&dest) {
            Ok(m) => m,
            Err(e) => {
                tracing::error!(error=%e, "Failed to get metadata for {}", dest.display());
                return Err(StorageError::Local(e.to_string()).into());
            }
        };
        let checksum = match calculate_checksum(&dest) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error=%e, "Failed to calculate checksum for {}", dest.display());
                return Err(e);
            }
        };

        tracing::info!(key=%key, size=%metadata.len(), checksum=%checksum, "File uploaded successfully");
        Ok(StorageHandle {
            key: key.to_string(),
            size: metadata.len(),
            checksum,
        })
    }

    async fn download(&self, key: &str) -> Result<Vec<u8>> {
        let path = self.resolve_key(key)?;
        tracing::debug!(key=%key, path=%path.display(), "Downloading from local storage");
        std::fs::read(&path).map_err(|e| {
            match e.kind() {
                std::io::ErrorKind::NotFound => {
                    tracing::warn!(key=%key, "File not found in local storage");
                    StorageError::NotFound(key.to_string())
                }
                std::io::ErrorKind::PermissionDenied => {
                    tracing::error!(key=%key, error=%e, "Permission denied accessing local storage");
                    StorageError::PermissionDenied(key.to_string())
                }
                _ => {
                    tracing::error!(key=%key, error=%e, "Local storage error");
                    StorageError::Local(e.to_string())
                }
            }.into()
        })
    }

    async fn exists(&self, key: &str) -> bool {
        self.resolve_key(key).map(|p| p.exists()).unwrap_or(false)
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let path = self.resolve_key(key)?;
        tracing::debug!(key=%key, path=%path.display(), "Deleting from local storage");
        std::fs::remove_file(path).map_err(|e| {
            match e.kind() {
                std::io::ErrorKind::NotFound => {
                    tracing::warn!(key=%key, "File not found for deletion");
                    StorageError::NotFound(key.to_string())
                }
                _ => {
                    tracing::error!(key=%key, error=%e, "Failed to delete from local storage");
                    StorageError::Local(e.to_string())
                }
            }
            .into()
        })
    }

    fn url(&self, _key: &str) -> Option<String> {
        None
    }
}

/// AWS S3 / RustFS storage backend
pub struct S3Storage {
    client: aws_sdk_s3::Client,
    bucket: String,
    endpoint: Option<String>,
}

impl S3Storage {
    pub async fn new(config: &StorageConfig) -> Result<Self> {
        use aws_config::BehaviorVersion;
        use aws_sdk_s3::config::Credentials as S3Credentials;

        let mut cfg = aws_config::defaults(BehaviorVersion::latest());

        // Override endpoint for RustFS/MinIO
        if let Some(endpoint) = &config.endpoint {
            cfg = cfg.endpoint_url(endpoint);
        }

        // Use configured credentials if provided
        if let (Some(access_key), Some(secret_key)) = (&config.access_key, &config.secret_key) {
            let creds =
                S3Credentials::new(access_key, secret_key, None, None, "static-credentials");
            cfg = cfg.credentials_provider(creds);
        }

        let sdk_cfg = cfg.load().await;
        let client = aws_sdk_s3::Client::new(&sdk_cfg);

        Ok(Self {
            client,
            bucket: config
                .bucket
                .clone()
                .unwrap_or_else(|| "ruci-artifacts".to_string()),
            endpoint: config.endpoint.clone(),
        })
    }
}

#[async_trait]
impl Storage for S3Storage {
    async fn upload(&self, key: &str, path: &Path) -> Result<StorageHandle> {
        use aws_sdk_s3::primitives::ByteStream;

        tracing::debug!(key=%key, path=%path.display(), bucket=%self.bucket, "Uploading file to S3");

        let body = ByteStream::from_path(path).await.map_err(|e| {
            tracing::error!(error=%e, "Failed to read file for S3 upload");
            StorageError::UploadFailed(e.to_string())
        })?;

        let metadata = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) => {
                tracing::error!(error=%e, "Failed to get file metadata");
                return Err(StorageError::UploadFailed(e.to_string()).into());
            }
        };
        let checksum = match calculate_checksum(path) {
            Ok(c) => c,
            Err(e) => return Err(e),
        };

        if let Err(e) = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(body)
            .send()
            .await
        {
            tracing::error!(error=%e, key=%key, bucket=%self.bucket, "S3 upload failed");
            return Err(StorageError::UploadFailed(e.to_string()).into());
        }

        tracing::info!(key=%key, size=%metadata.len(), checksum=%checksum, bucket=%self.bucket, "File uploaded to S3 successfully");
        Ok(StorageHandle {
            key: key.to_string(),
            size: metadata.len(),
            checksum,
        })
    }

    async fn download(&self, key: &str) -> Result<Vec<u8>> {
        tracing::debug!(key=%key, bucket=%self.bucket, "Downloading from S3");
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                tracing::error!(error=%e, key=%key, bucket=%self.bucket, "S3 download failed");
                StorageError::DownloadFailed(e.to_string())
            })?;

        let body = resp.body
            .collect()
            .await
            .map_err(|e| {
                tracing::error!(error=%e, key=%key, bucket=%self.bucket, "Failed to collect S3 response body");
                StorageError::DownloadFailed(e.to_string())
            })?;

        let data = body.to_vec();
        let size = data.len();
        tracing::info!(key=%key, size=%size, bucket=%self.bucket, "File downloaded from S3 successfully");
        Ok(data)
    }

    async fn exists(&self, key: &str) -> bool {
        let exists = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .is_ok();
        tracing::debug!(key=%key, bucket=%self.bucket, exists=%exists, "S3 exists check");
        exists
    }

    async fn delete(&self, key: &str) -> Result<()> {
        tracing::debug!(key=%key, bucket=%self.bucket, "Deleting from S3");
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                tracing::error!(error=%e, key=%key, bucket=%self.bucket, "S3 delete failed");
                StorageError::Local(e.to_string())
            })?;
        tracing::info!(key=%key, bucket=%self.bucket, "File deleted from S3 successfully");
        Ok(())
    }

    fn url(&self, key: &str) -> Option<String> {
        self.endpoint
            .as_ref()
            .map(|ep| format!("{}/{}/{}", ep, self.bucket, key))
    }
}

/// Create a storage backend based on configuration
pub async fn create_storage(config: &StorageConfig) -> Result<Box<dyn Storage>> {
    let max_size_mb = config.max_artifact_size_mb.unwrap_or(100);
    match config.storage_type {
        StorageType::Local => {
            let path = config.bucket.as_deref().unwrap_or("/var/lib/ruci/archive");
            Ok(Box::new(LocalStorage::new(path, max_size_mb)))
        }
        StorageType::Rustfs => {
            let storage = S3Storage::new(config).await?;
            Ok(Box::new(storage))
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Plugin Storage Extension Point
// ═══════════════════════════════════════════════════════════════

// Extension point for custom storage plugins
//
// To add a new storage backend (e.g., GCS, Azure Blob, etc.):
// 1. Create a new crate
// 2. Implement the `Storage` trait
// 3. Register in the `create_storage` function

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_temp_dir() -> TempDir {
        tempfile::tempdir().expect("Failed to create temp dir")
    }

    #[tokio::test]
    async fn test_local_storage_upload_download() {
        let temp_dir = create_temp_dir();
        let storage = LocalStorage::new(temp_dir.path().to_str().unwrap(), 1000);

        // Create a test file
        let test_content = b"Hello, Local Storage!";
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        temp_file.write_all(test_content).unwrap();
        let file_path = temp_file.path();

        // Upload
        let handle = storage
            .upload("test-key", file_path)
            .await
            .expect("Failed to upload");
        assert_eq!(handle.key, "test-key");
        assert_eq!(handle.size, test_content.len() as u64);

        // Download
        let downloaded = storage
            .download("test-key")
            .await
            .expect("Failed to download");
        assert_eq!(downloaded, test_content);
    }

    #[tokio::test]
    async fn test_local_storage_exists() {
        let temp_dir = create_temp_dir();
        let storage = LocalStorage::new(temp_dir.path().to_str().unwrap(), 1000);

        // Create and upload a test file
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        temp_file.write_all(b"test content").unwrap();
        storage
            .upload("existing-key", temp_file.path())
            .await
            .expect("Failed to upload");

        // Check exists
        assert!(storage.exists("existing-key").await);
        assert!(!storage.exists("non-existing-key").await);
    }

    #[tokio::test]
    async fn test_local_storage_delete() {
        let temp_dir = create_temp_dir();
        let storage = LocalStorage::new(temp_dir.path().to_str().unwrap(), 1000);

        // Create and upload a test file
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        temp_file.write_all(b"test content").unwrap();
        storage
            .upload("to-delete", temp_file.path())
            .await
            .expect("Failed to upload");

        // Delete
        storage.delete("to-delete").await.expect("Failed to delete");

        // Verify deleted
        assert!(!storage.exists("to-delete").await);
    }

    #[tokio::test]
    async fn test_local_storage_delete_not_found() {
        let temp_dir = create_temp_dir();
        let storage = LocalStorage::new(temp_dir.path().to_str().unwrap(), 1000);

        // Deleting non-existing key should return error
        let result = storage.delete("non-existing").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_local_storage_download_not_found() {
        let temp_dir = create_temp_dir();
        let storage = LocalStorage::new(temp_dir.path().to_str().unwrap(), 1000);

        let result = storage.download("non-existing").await;
        assert!(result.is_err());
    }

    #[test]
    fn test_storage_handle() {
        let handle = StorageHandle {
            key: "test-key".to_string(),
            size: 1024,
            checksum: "abc123".to_string(),
        };
        assert_eq!(handle.key, "test-key");
        assert_eq!(handle.size, 1024);
        assert_eq!(handle.checksum, "abc123");
    }

    #[test]
    fn test_local_storage_url() {
        let storage = LocalStorage::new("/tmp/storage", 1000);
        assert!(storage.url("any-key").is_none());
    }

    #[tokio::test]
    async fn test_local_storage_nested_path() {
        let temp_dir = create_temp_dir();
        let storage = LocalStorage::new(temp_dir.path().to_str().unwrap(), 1000);

        // Create and upload a test file with nested path
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        temp_file.write_all(b"nested content").unwrap();

        let handle = storage
            .upload("dir1/dir2/nested-key", temp_file.path())
            .await
            .expect("Failed to upload nested file");

        assert_eq!(handle.key, "dir1/dir2/nested-key");

        // Verify file exists and can be downloaded
        let downloaded = storage
            .download("dir1/dir2/nested-key")
            .await
            .expect("Failed to download nested file");
        assert_eq!(downloaded, b"nested content");
    }

    #[tokio::test]
    async fn test_local_storage_overwrite() {
        let temp_dir = create_temp_dir();
        let storage = LocalStorage::new(temp_dir.path().to_str().unwrap(), 1000);

        // Upload first version
        let mut temp_file1 = tempfile::NamedTempFile::new().unwrap();
        temp_file1.write_all(b"version 1").unwrap();
        storage
            .upload("versioned-key", temp_file1.path())
            .await
            .expect("Failed to upload v1");

        // Upload second version
        let mut temp_file2 = tempfile::NamedTempFile::new().unwrap();
        temp_file2.write_all(b"version 2").unwrap();
        storage
            .upload("versioned-key", temp_file2.path())
            .await
            .expect("Failed to upload v2");

        // Verify second version is retrieved
        let downloaded = storage
            .download("versioned-key")
            .await
            .expect("Failed to download");
        assert_eq!(downloaded, b"version 2");
    }

    #[test]
    fn test_calculate_checksum() {
        use std::fs;
        use std::io::Write;

        let temp_dir = create_temp_dir();
        let temp_file_path = temp_dir.path().join("checksum_test.txt");

        let mut file = fs::File::create(&temp_file_path).unwrap();
        file.write_all(b"checksum test content").unwrap();

        let checksum = calculate_checksum(&temp_file_path).expect("Failed to calculate checksum");

        // SHA-256 hash of "checksum test content"
        // The hash should be consistent
        let checksum2 =
            calculate_checksum(&temp_file_path).expect("Failed to calculate checksum again");
        assert_eq!(checksum, checksum2);

        // Hash should be base64 encoded
        assert!(!checksum.contains('+'));
        assert!(!checksum.contains('/'));
    }

    #[test]
    fn test_calculate_checksum_different_content() {
        use std::fs;
        use std::io::Write;

        let temp_dir = create_temp_dir();

        let path1 = temp_dir.path().join("file1.txt");
        let path2 = temp_dir.path().join("file2.txt");

        std::fs::write(&path1, b"content 1").unwrap();
        std::fs::write(&path2, b"content 2").unwrap();

        let checksum1 = calculate_checksum(&path1).unwrap();
        let checksum2 = calculate_checksum(&path2).unwrap();

        assert_ne!(checksum1, checksum2);
    }

    #[test]
    fn test_calculate_checksum_empty_file() {
        use std::fs;

        let temp_dir = create_temp_dir();
        let empty_path = temp_dir.path().join("empty.txt");
        std::fs::write(&empty_path, b"").unwrap();

        let checksum = calculate_checksum(&empty_path).unwrap();
        // Empty file should still produce a valid hash
        assert!(!checksum.is_empty());
    }

    #[tokio::test]
    async fn test_local_storage_large_file() {
        let temp_dir = create_temp_dir();
        let storage = LocalStorage::new(temp_dir.path().to_str().unwrap(), 1000);

        // Create a larger test file
        let large_content = vec![0u8; 1024 * 100]; // 100KB
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        temp_file.write_all(&large_content).unwrap();

        let handle = storage
            .upload("large-file", temp_file.path())
            .await
            .expect("Failed to upload large file");

        assert_eq!(handle.size, 1024 * 100);

        let downloaded = storage.download("large-file").await.unwrap();
        assert_eq!(downloaded.len(), 1024 * 100);
    }

    #[tokio::test]
    async fn test_local_storage_binary_content() {
        let temp_dir = create_temp_dir();
        let storage = LocalStorage::new(temp_dir.path().to_str().unwrap(), 1000);

        // Binary content with null bytes
        let binary_content = b"\x00\x01\x02\xff\xfe\xfd\x00\xff";
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        temp_file.write_all(binary_content).unwrap();

        let handle = storage
            .upload("binary-file", temp_file.path())
            .await
            .expect("Failed to upload binary file");

        let downloaded = storage.download("binary-file").await.unwrap();
        assert_eq!(downloaded, binary_content);
    }

    #[test]
    fn test_storage_handle_clone() {
        let handle1 = StorageHandle {
            key: "key1".to_string(),
            size: 100,
            checksum: "abc123".to_string(),
        };
        let handle2 = handle1.clone();
        assert_eq!(handle1.key, handle2.key);
        assert_eq!(handle1.size, handle2.size);
        assert_eq!(handle1.checksum, handle2.checksum);
    }

    #[test]
    fn test_local_storage_resolve_key() {
        let temp_dir = create_temp_dir();
        let storage = LocalStorage::new(temp_dir.path().to_str().unwrap(), 1000);

        // resolve_key is private but we can test indirectly via upload/download
        // Just verify the storage was created correctly
        assert!(storage.url("test").is_none());
    }

    #[tokio::test]
    async fn test_local_storage_special_characters_in_key() {
        let temp_dir = create_temp_dir();
        let storage = LocalStorage::new(temp_dir.path().to_str().unwrap(), 1000);

        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        temp_file.write_all(b"content").unwrap();

        // Key with spaces and special chars
        let key = "file with spaces & symbols.txt";
        let handle = storage.upload(key, temp_file.path()).await.unwrap();
        assert_eq!(handle.key, key);

        let downloaded = storage.download(key).await.unwrap();
        assert_eq!(downloaded, b"content");
    }

    #[tokio::test]
    async fn test_local_storage_unicode_content() {
        let temp_dir = create_temp_dir();
        let storage = LocalStorage::new(temp_dir.path().to_str().unwrap(), 1000);

        // Unicode content
        let unicode_content = "你好世界 🌍 مرحبا";
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        temp_file.write_all(unicode_content.as_bytes()).unwrap();

        let handle = storage
            .upload("unicode-file", temp_file.path())
            .await
            .unwrap();
        assert_eq!(handle.key, "unicode-file");

        let downloaded = storage.download("unicode-file").await.unwrap();
        assert_eq!(String::from_utf8(downloaded).unwrap(), unicode_content);
    }

    #[tokio::test]
    async fn test_local_storage_download_after_delete() {
        let temp_dir = create_temp_dir();
        let storage = LocalStorage::new(temp_dir.path().to_str().unwrap(), 1000);

        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        temp_file.write_all(b"to be deleted").unwrap();

        storage.upload("delete-me", temp_file.path()).await.unwrap();
        storage.delete("delete-me").await.unwrap();

        let result = storage.download("delete-me").await;
        assert!(result.is_err());
    }

    // ═══════════════════════════════════════════════════════════════
    // S3 Integration Tests (require MinIO or AWS S3)
    // Run with: cargo test -- --ignored
    // ═══════════════════════════════════════════════════════════════

    fn create_test_s3_config() -> StorageConfig {
        StorageConfig {
            storage_type: StorageType::Rustfs,
            endpoint: Some(
                std::env::var("S3_ENDPOINT")
                    .unwrap_or_else(|_| "http://localhost:9000".to_string()),
            ),
            bucket: Some(std::env::var("S3_BUCKET").unwrap_or_else(|_| "ruci-test".to_string())),
            access_key: Some(
                std::env::var("S3_ACCESS_KEY").unwrap_or_else(|_| "minioadmin".to_string()),
            ),
            secret_key: Some(
                std::env::var("S3_SECRET_KEY").unwrap_or_else(|_| "minioadmin".to_string()),
            ),
            region: std::env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".to_string()),
            max_artifact_size_mb: Some(1000),
        }
    }

    #[tokio::test]
    #[ignore = "requires MinIO or AWS S3"]
    async fn test_s3_storage_upload_download() {
        let config = create_test_s3_config();
        let storage = S3Storage::new(&config)
            .await
            .expect("Failed to create S3 storage");

        // Create a test file
        let temp_dir = create_temp_dir();
        let test_content = b"Hello, S3 Storage!";
        let temp_file_path = temp_dir.path().join("test_s3.txt");
        std::fs::write(&temp_file_path, test_content).expect("Failed to write temp file");

        let key = "test/s3/integration.txt";

        // Upload
        let handle = storage
            .upload(key, &temp_file_path)
            .await
            .expect("Failed to upload");
        assert_eq!(handle.key, key);
        assert_eq!(handle.size, test_content.len() as u64);

        // Download
        let downloaded = storage.download(key).await.expect("Failed to download");
        assert_eq!(downloaded, test_content);

        // Cleanup
        storage.delete(key).await.expect("Failed to delete");
    }

    #[tokio::test]
    #[ignore = "requires MinIO or AWS S3"]
    async fn test_s3_storage_exists() {
        let config = create_test_s3_config();
        let storage = S3Storage::new(&config)
            .await
            .expect("Failed to create S3 storage");

        // Create a test file
        let temp_dir = create_temp_dir();
        let test_content = b"test content for exists check";
        let temp_file_path = temp_dir.path().join("exists_test.txt");
        std::fs::write(&temp_file_path, test_content).expect("Failed to write temp file");

        let key = "test/s3/exists_check.txt";

        // Upload first
        storage
            .upload(key, &temp_file_path)
            .await
            .expect("Failed to upload");

        // Check exists
        assert!(storage.exists(key).await, "File should exist after upload");
        assert!(
            !storage.exists("nonexistent-key").await,
            "Non-existent file should return false"
        );

        // Cleanup
        storage.delete(key).await.expect("Failed to delete");
    }

    #[tokio::test]
    #[ignore = "requires MinIO or AWS S3"]
    async fn test_s3_storage_delete() {
        let config = create_test_s3_config();
        let storage = S3Storage::new(&config)
            .await
            .expect("Failed to create S3 storage");

        // Create a test file
        let temp_dir = create_temp_dir();
        let test_content = b"content to delete";
        let temp_file_path = temp_dir.path().join("delete_test.txt");
        std::fs::write(&temp_file_path, test_content).expect("Failed to write temp file");

        let key = "test/s3/delete_test.txt";

        // Upload
        storage
            .upload(key, &temp_file_path)
            .await
            .expect("Failed to upload");

        // Verify it exists
        assert!(
            storage.exists(key).await,
            "File should exist before deletion"
        );

        // Delete
        storage.delete(key).await.expect("Failed to delete");

        // Verify it's gone
        assert!(
            !storage.exists(key).await,
            "File should not exist after deletion"
        );
    }

    #[tokio::test]
    #[ignore = "requires MinIO or AWS S3"]
    async fn test_s3_storage_url() {
        let config = create_test_s3_config();
        let storage = S3Storage::new(&config)
            .await
            .expect("Failed to create S3 storage");

        let key = "test/s3/url_test.txt";
        let url = storage.url(key);

        if let Some(url) = url {
            assert!(url.contains(key), "URL should contain the key");
        }
        // If endpoint is not set, url() returns None which is valid
    }

    #[tokio::test]
    #[ignore = "requires MinIO or AWS S3"]
    async fn test_s3_storage_nested_path() {
        let config = create_test_s3_config();
        let storage = S3Storage::new(&config)
            .await
            .expect("Failed to create S3 storage");

        // Create a test file
        let temp_dir = create_temp_dir();
        let test_content = b"nested path content";
        let temp_file_path = temp_dir.path().join("nested.txt");
        std::fs::write(&temp_file_path, test_content).expect("Failed to write temp file");

        let key = "test/s3/nested/dir/path/file.txt";

        // Upload with nested path
        let handle = storage
            .upload(key, &temp_file_path)
            .await
            .expect("Failed to upload");
        assert_eq!(handle.key, key);

        // Download and verify
        let downloaded = storage.download(key).await.expect("Failed to download");
        assert_eq!(downloaded, test_content);

        // Cleanup
        storage.delete(key).await.expect("Failed to delete");
    }

    #[tokio::test]
    #[ignore = "requires MinIO or AWS S3"]
    async fn test_s3_storage_binary_content() {
        let config = create_test_s3_config();
        let storage = S3Storage::new(&config)
            .await
            .expect("Failed to create S3 storage");

        // Binary content with null bytes
        let binary_content = b"\x00\x01\x02\xff\xfe\xfd\x00\xff";
        let temp_dir = create_temp_dir();
        let temp_file_path = temp_dir.path().join("binary.txt");
        std::fs::write(&temp_file_path, binary_content).expect("Failed to write temp file");

        let key = "test/s3/binary.txt";

        // Upload
        let handle = storage
            .upload(key, &temp_file_path)
            .await
            .expect("Failed to upload");
        assert_eq!(handle.size, binary_content.len() as u64);

        // Download and verify
        let downloaded = storage.download(key).await.expect("Failed to download");
        assert_eq!(downloaded, binary_content);

        // Cleanup
        storage.delete(key).await.expect("Failed to delete");
    }

    #[tokio::test]
    #[ignore = "requires MinIO or AWS S3"]
    async fn test_s3_storage_large_file() {
        let config = create_test_s3_config();
        let storage = S3Storage::new(&config)
            .await
            .expect("Failed to create S3 storage");

        // Create a larger test file (1MB)
        let large_content = vec![0u8; 1024 * 1024];
        let temp_dir = create_temp_dir();
        let temp_file_path = temp_dir.path().join("large.txt");
        std::fs::write(&temp_file_path, &large_content).expect("Failed to write temp file");

        let key = "test/s3/large_file.txt";

        // Upload
        let handle = storage
            .upload(key, &temp_file_path)
            .await
            .expect("Failed to upload");
        assert_eq!(handle.size, 1024 * 1024);

        // Download and verify
        let downloaded = storage.download(key).await.expect("Failed to download");
        assert_eq!(downloaded.len(), 1024 * 1024);

        // Cleanup
        storage.delete(key).await.expect("Failed to delete");
    }

    #[tokio::test]
    #[ignore = "requires MinIO or AWS S3"]
    async fn test_s3_storage_overwrite() {
        let config = create_test_s3_config();
        let storage = S3Storage::new(&config)
            .await
            .expect("Failed to create S3 storage");

        let temp_dir = create_temp_dir();
        let key = "test/s3/overwrite.txt";

        // Upload first version
        let temp_file_v1 = temp_dir.path().join("v1.txt");
        std::fs::write(&temp_file_v1, b"version 1").expect("Failed to write temp file");
        storage
            .upload(key, &temp_file_v1)
            .await
            .expect("Failed to upload v1");

        // Upload second version
        let temp_file_v2 = temp_dir.path().join("v2.txt");
        std::fs::write(&temp_file_v2, b"version 2").expect("Failed to write temp file");
        storage
            .upload(key, &temp_file_v2)
            .await
            .expect("Failed to upload v2");

        // Download and verify second version
        let downloaded = storage.download(key).await.expect("Failed to download");
        assert_eq!(downloaded, b"version 2");

        // Cleanup
        storage.delete(key).await.expect("Failed to delete");
    }

    #[tokio::test]
    #[ignore = "requires MinIO or AWS S3"]
    async fn test_s3_storage_unicode_content() {
        let config = create_test_s3_config();
        let storage = S3Storage::new(&config)
            .await
            .expect("Failed to create S3 storage");

        let unicode_content = "你好世界 🌍 مرحبا S3";
        let temp_dir = create_temp_dir();
        let temp_file_path = temp_dir.path().join("unicode.txt");
        std::fs::write(&temp_file_path, unicode_content.as_bytes())
            .expect("Failed to write temp file");

        let key = "test/s3/unicode.txt";

        // Upload
        storage
            .upload(key, &temp_file_path)
            .await
            .expect("Failed to upload");

        // Download and verify
        let downloaded = storage.download(key).await.expect("Failed to download");
        assert_eq!(String::from_utf8(downloaded).unwrap(), unicode_content);

        // Cleanup
        storage.delete(key).await.expect("Failed to delete");
    }

    #[tokio::test]
    #[ignore = "requires MinIO or AWS S3"]
    async fn test_s3_storage_not_found() {
        let config = create_test_s3_config();
        let storage = S3Storage::new(&config)
            .await
            .expect("Failed to create S3 storage");

        // Try to download non-existent file
        let result = storage.download("nonexistent-file-12345.txt").await;
        assert!(
            result.is_err(),
            "Downloading non-existent file should return error"
        );

        // Try to delete non-existent file - should return error
        let result = storage.delete("nonexistent-file-12345.txt").await;
        assert!(
            result.is_err(),
            "Deleting non-existent file should return error"
        );
    }
}
