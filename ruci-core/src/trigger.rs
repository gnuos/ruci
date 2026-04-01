//! Trigger scheduler module
//!
//! Implements cron-based job scheduling using tokio-cron-scheduler

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio_cron_scheduler::{Job, JobScheduler};

use crate::config::TriggerConfig;
use crate::db::Repository;
use crate::queue::{JobQueue, QueueRequest};

/// Trigger scheduler for cron-based job execution
pub struct TriggerScheduler {
    scheduler: JobScheduler,
    config: Arc<Vec<TriggerConfig>>,
    db: Arc<dyn Repository>,
    queue: Arc<JobQueue>,
    jobs_dir: String,
}

impl TriggerScheduler {
    /// Create a new trigger scheduler
    pub async fn new(
        config: Arc<Vec<TriggerConfig>>,
        db: Arc<dyn Repository>,
        queue: Arc<JobQueue>,
        jobs_dir: String,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let scheduler = JobScheduler::new().await?;

        let scheduler = Self {
            scheduler,
            config,
            db,
            queue,
            jobs_dir,
        };

        Ok(scheduler)
    }

    /// Start the scheduler with all configured triggers
    pub async fn start(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        for trigger in self.config.iter() {
            if !trigger.enabled {
                tracing::info!(trigger = %trigger.name, "Trigger is disabled, skipping");
                continue;
            }

            // Validate cron expression
            if let Err(e) = cron::Schedule::from_str(&trigger.cron) {
                tracing::error!(
                    trigger = %trigger.name,
                    cron = %trigger.cron,
                    error = %e,
                    "Invalid cron expression"
                );
                continue;
            }

            let trigger_name = trigger.name.clone();
            let job_id = trigger.job.clone();
            let db = self.db.clone();
            let queue = self.queue.clone();
            let jobs_dir = self.jobs_dir.clone();
            let cron_expr = trigger.cron.clone();

            let job = Job::new_async(cron_expr.as_str(), move |_uuid, _l| {
                let db = db.clone();
                let queue = queue.clone();
                let job_id = job_id.clone();
                let trigger_name = trigger_name.clone();
                let jobs_dir = jobs_dir.clone();

                Box::pin(async move {
                    tracing::info!(trigger = %trigger_name, job_id = %job_id, "Trigger fired");

                    // Check if job file exists
                    let job_path = format!("{}/{}.yaml", jobs_dir, job_id);
                    if !std::path::Path::new(&job_path).exists() {
                        tracing::error!(
                            trigger = %trigger_name,
                            job_id = %job_id,
                            "Job file not found, skipping trigger"
                        );
                        return;
                    }

                    // Get next build number
                    let build_num = match db.next_build_num(&job_id).await {
                        Ok(num) => num,
                        Err(e) => {
                            tracing::error!(
                                trigger = %trigger_name,
                                job_id = %job_id,
                                error = %e,
                                "Failed to get next build number"
                            );
                            return;
                        }
                    };

                    // Generate run_id
                    let run_id = format!("{}-{}", job_id, uuid::Uuid::new_v4());

                    // Create queue request
                    let request = QueueRequest {
                        job_id: job_id.clone(),
                        run_id: run_id.clone(),
                        params: HashMap::new(),
                        build_num: build_num as u64,
                    };

                    // Enqueue the job
                    if let Err(e) = queue.enqueue(request).await {
                        tracing::error!(
                            trigger = %trigger_name,
                            job_id = %job_id,
                            run_id = %run_id,
                            error = %e,
                            "Failed to enqueue triggered job"
                        );
                    } else {
                        tracing::info!(
                            trigger = %trigger_name,
                            job_id = %job_id,
                            run_id = %run_id,
                            build_num = %build_num,
                            "Triggered job enqueued"
                        );
                    }
                })
            })?;

            if let Err(e) = self.scheduler.add(job).await {
                tracing::error!(
                    trigger = %trigger.name,
                    error = %e,
                    "Failed to add trigger to scheduler"
                );
            } else {
                tracing::info!(
                    trigger = %trigger.name,
                    cron = %trigger.cron,
                    job_id = %trigger.job,
                    "Trigger registered"
                );
            }
        }

        self.scheduler.start().await?;
        tracing::info!("Trigger scheduler started");
        Ok(())
    }

    /// Shutdown the scheduler
    pub async fn shutdown(&mut self) -> anyhow::Result<()> {
        self.scheduler.shutdown().await?;
        Ok(())
    }
}

/// Validate a cron expression
pub fn validate_cron(cron_expr: &str) -> Result<(), String> {
    cron::Schedule::from_str(cron_expr)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_cron_valid() {
        // cron crate uses 6-field format: second minute hour day month weekday
        assert!(validate_cron("0 0 * * * *").is_ok()); // Every hour at minute 0
        assert!(validate_cron("0 0 0 * * *").is_ok()); // Every day at midnight
        assert!(validate_cron("0 */5 * * * *").is_ok()); // Every 5 minutes
        assert!(validate_cron("0 0 0 * * SUN").is_ok()); // Every Sunday at midnight
        assert!(validate_cron("0 0 0 1 * *").is_ok()); // First day of every month at midnight
        assert!(validate_cron("0 0 12 * * *").is_ok()); // Every day at noon
    }

    #[test]
    fn test_validate_cron_invalid() {
        assert!(validate_cron("invalid").is_err());
        assert!(validate_cron("* * * * *").is_err()); // Too few fields (5 instead of 6)
        assert!(validate_cron("* * * * * * * *").is_err()); // Too many fields
        assert!(validate_cron("61 * * * * *").is_err()); // Invalid second (0-60)
        assert!(validate_cron("0 60 * * * *").is_err()); // Invalid minute (0-59)
        assert!(validate_cron("0 0 25 * * *").is_err()); // Invalid hour (0-23)
    }

    #[test]
    fn test_validate_cron_with_seconds() {
        // 6-field cron with explicit seconds
        assert!(validate_cron("0 0 0 * * * *").is_ok()); // Every day at midnight (7 fields - too many)
        assert!(validate_cron("0 0 0 * * *").is_ok()); // Every day at midnight with explicit seconds
    }
}
