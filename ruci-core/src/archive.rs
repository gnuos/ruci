//! Archive module
//!
//! Handles archiving of completed runs and cleanup of old archives

use std::path::{Path, PathBuf};
use tokio::fs;

use crate::config::ArchiveConfig;
use crate::error::Result;

/// Archive information
#[derive(Debug, Clone)]
pub struct ArchiveInfo {
    pub run_id: String,
    pub path: PathBuf,
    pub is_tar: bool,
    pub size_bytes: u64,
    pub created_at: std::time::SystemTime,
}

/// Archive manager for handling run archiving and cleanup
pub struct ArchiveManager {
    archive_dir: PathBuf,
    _config: ArchiveConfig,
}

impl ArchiveManager {
    /// Create a new ArchiveManager
    pub fn new(archive_dir: &str, config: ArchiveConfig) -> Self {
        Self {
            archive_dir: PathBuf::from(archive_dir),
            _config: config,
        }
    }

    /// Get the archive directory path
    pub fn archive_dir(&self) -> &Path {
        &self.archive_dir
    }

    /// Archive a run's logs and artifacts
    ///
    /// Creates a tar archive at `archive_dir/run_id.tar` containing:
    /// - `run.json` - Run metadata
    /// - `logs.txt` - Execution logs
    pub async fn archive_run(
        &self,
        run_id: &str,
        run_info: &RunArchiveInfo,
        logs: &str,
    ) -> Result<ArchiveInfo> {
        let run_dir = self.archive_dir.join(run_id);

        // Create temp directory for this run's archive contents
        fs::create_dir_all(&run_dir).await?;

        // Write run metadata
        let metadata_path = run_dir.join("run.json");
        let metadata_json = serde_json::to_string_pretty(run_info)?;
        fs::write(&metadata_path, metadata_json).await?;

        // Write logs
        let logs_path = run_dir.join("logs.txt");
        fs::write(&logs_path, logs).await?;

        // Create tar archive
        let tar_path = self.archive_dir.join(format!("{}.tar", run_id));
        self.create_tar(&run_dir, &tar_path).await?;

        // Calculate size
        let size_bytes = Self::calculate_file_size(&tar_path).await?;

        // Clean up temp directory
        fs::remove_dir_all(&run_dir).await?;

        Ok(ArchiveInfo {
            run_id: run_id.to_string(),
            path: tar_path,
            is_tar: true,
            size_bytes,
            created_at: std::time::SystemTime::now(),
        })
    }

    /// Create a tar archive from a directory using blocking IO
    async fn create_tar(&self, source_dir: &Path, dest_path: &Path) -> Result<()> {
        let source_dir = source_dir.to_path_buf();
        let dest_path = dest_path.to_path_buf();

        tokio::task::spawn_blocking(move || {
            let file = std::fs::File::create(&dest_path)?;
            let mut builder = tar::Builder::new(file);

            // Walk the directory and add all files
            Self::add_dir_to_tar(&mut builder, &source_dir, &source_dir)?;

            builder.finish()?;
            Ok::<(), std::io::Error>(())
        })
        .await
        .map_err(|e| crate::Error::Other(format!("task join error: {}", e)))??;

        Ok(())
    }

    /// Recursively add directory contents to tar archive
    fn add_dir_to_tar(
        builder: &mut tar::Builder<std::fs::File>,
        base_dir: &Path,
        current: &Path,
    ) -> std::io::Result<()> {
        let entries = std::fs::read_dir(current)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let relative_path = path.strip_prefix(base_dir).unwrap_or(&path);

            if path.is_dir() {
                Self::add_dir_to_tar(builder, base_dir, &path)?;
            } else {
                builder.append_path_with_name(&path, &relative_path)?;
            }
        }
        Ok(())
    }

    /// Cleanup old archives beyond max_age_days
    ///
    /// Returns the number of archives deleted.
    pub async fn cleanup_old_archives(&self, max_age_days: u32) -> Result<usize> {
        let mut deleted_count = 0;
        let max_age_secs = max_age_days as u64 * 24 * 60 * 60;

        let mut entries = fs::read_dir(&self.archive_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            // Only process .tar files
            if !path.is_file() {
                continue;
            }

            if let Some(ext) = path.extension() {
                if ext != "tar" {
                    continue;
                }
            } else {
                continue;
            }

            if let Ok(metadata) = entry.metadata().await {
                if let Ok(modified) = metadata.modified() {
                    let age = std::time::SystemTime::now()
                        .duration_since(modified)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);

                    if age > max_age_secs {
                        fs::remove_file(&path).await?;
                        deleted_count += 1;
                        tracing::info!(path = %path.display(), "Deleted old archive");
                    }
                }
            }
        }

        Ok(deleted_count)
    }

    /// List all archives
    pub async fn list_archives(&self) -> Result<Vec<ArchiveInfo>> {
        let mut archives = Vec::new();
        let mut entries = fs::read_dir(&self.archive_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            // Only include .tar files
            if !path.is_file() {
                continue;
            }

            if let Some(ext) = path.extension() {
                if ext != "tar" {
                    continue;
                }
            } else {
                continue;
            }

            let run_id = path.file_stem().unwrap().to_string_lossy().to_string();

            if let Ok(metadata) = entry.metadata().await {
                let size_bytes = metadata.len();
                let created_at = metadata.modified().unwrap_or(std::time::SystemTime::now());

                archives.push(ArchiveInfo {
                    run_id,
                    path,
                    is_tar: true,
                    size_bytes,
                    created_at,
                });
            }
        }

        archives.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(archives)
    }

    /// Get archive info for a specific run
    pub async fn get_archive(&self, run_id: &str) -> Result<Option<ArchiveInfo>> {
        let archives = self.list_archives().await?;
        Ok(archives.into_iter().find(|a| a.run_id == run_id))
    }

    /// Delete a specific archive
    pub async fn delete_archive(&self, run_id: &str) -> Result<bool> {
        let archives = self.list_archives().await?;

        for archive in archives {
            if archive.run_id == run_id {
                fs::remove_file(&archive.path).await?;
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Calculate size of a file
    async fn calculate_file_size(path: &Path) -> Result<u64> {
        let metadata = fs::metadata(path).await?;
        Ok(metadata.len())
    }
}

/// Run information for archiving
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunArchiveInfo {
    pub run_id: String,
    pub job_id: String,
    pub job_name: String,
    pub build_num: u64,
    pub status: String,
    pub exit_code: Option<i32>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub artifact_names: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> ArchiveConfig {
        ArchiveConfig {
            enabled: true,
            max_age_days: 30,
        }
    }

    #[tokio::test]
    async fn test_archive_manager_new() {
        let manager = ArchiveManager::new("/tmp/archive", create_test_config());
        assert_eq!(manager.archive_dir().to_string_lossy(), "/tmp/archive");
    }

    #[tokio::test]
    async fn test_list_archives_empty() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = ArchiveManager::new(temp_dir.path().to_str().unwrap(), create_test_config());

        let archives = manager.list_archives().await.unwrap();
        assert!(archives.is_empty());
    }

    #[tokio::test]
    async fn test_archive_run() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = ArchiveManager::new(temp_dir.path().to_str().unwrap(), create_test_config());

        let run_info = RunArchiveInfo {
            run_id: "run-123".to_string(),
            job_id: "job-456".to_string(),
            job_name: "test-job".to_string(),
            build_num: 1,
            status: "SUCCESS".to_string(),
            exit_code: Some(0),
            started_at: Some("2024-01-01T00:00:00Z".to_string()),
            finished_at: Some("2024-01-01T00:01:00Z".to_string()),
            artifact_names: vec![],
        };

        let archive = manager
            .archive_run("run-123", &run_info, "Build logs here")
            .await
            .unwrap();

        assert_eq!(archive.run_id, "run-123");
        assert!(archive.is_tar);
        assert!(archive.path.is_file());
        assert!(archive.path.to_string_lossy().ends_with(".tar"));
    }

    #[tokio::test]
    async fn test_delete_archive() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = ArchiveManager::new(temp_dir.path().to_str().unwrap(), create_test_config());

        let run_info = RunArchiveInfo {
            run_id: "run-to-delete".to_string(),
            job_id: "job-delete".to_string(),
            job_name: "delete-job".to_string(),
            build_num: 1,
            status: "SUCCESS".to_string(),
            exit_code: Some(0),
            started_at: None,
            finished_at: None,
            artifact_names: vec![],
        };

        manager
            .archive_run("run-to-delete", &run_info, "Logs")
            .await
            .unwrap();

        // Verify it exists
        let archives = manager.list_archives().await.unwrap();
        assert_eq!(archives.len(), 1);

        // Delete it
        let deleted = manager.delete_archive("run-to-delete").await.unwrap();
        assert!(deleted);

        // Verify it's gone
        let archives = manager.list_archives().await.unwrap();
        assert!(archives.is_empty());
    }

    #[tokio::test]
    async fn test_delete_nonexistent_archive() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = ArchiveManager::new(temp_dir.path().to_str().unwrap(), create_test_config());

        let deleted = manager.delete_archive("nonexistent").await.unwrap();
        assert!(!deleted);
    }
}
