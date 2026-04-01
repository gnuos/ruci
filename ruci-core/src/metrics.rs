//! Metrics module
//!
//! Prometheus metrics for monitoring rucid

use prometheus_client::metrics::{counter::Counter, gauge::Gauge, histogram::Histogram};
use prometheus_client::registry::Registry;
use std::sync::Arc;

/// Metrics collector for rucid
pub struct Metrics {
    pub registry: Arc<Registry>,

    // Job metrics
    pub jobs_total: Counter,
    pub jobs_running: Gauge,
    pub jobs_queued: Gauge,
    pub job_duration_seconds: Histogram,

    // RPC metrics
    pub rpc_requests_total: Counter,
    pub rpc_request_duration_seconds: Histogram,

    // System metrics
    pub uptime_seconds: Gauge,
}

impl Metrics {
    pub fn new() -> Self {
        let mut registry = Registry::default();

        let jobs_total = Counter::default();
        registry.register(
            "jobs_total",
            "Total number of jobs submitted",
            jobs_total.clone(),
        );

        let jobs_running = Gauge::default();
        registry.register(
            "jobs_running",
            "Number of currently running jobs",
            jobs_running.clone(),
        );

        let jobs_queued = Gauge::default();
        registry.register("jobs_queued", "Number of queued jobs", jobs_queued.clone());

        let job_duration_seconds = Histogram::new(
            [
                0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0, 300.0, 600.0, 1800.0, 3600.0,
            ]
            .into_iter(),
        );
        registry.register(
            "job_duration_seconds",
            "Job execution duration in seconds",
            job_duration_seconds.clone(),
        );

        let rpc_requests_total = Counter::default();
        registry.register(
            "rpc_requests_total",
            "Total number of RPC requests",
            rpc_requests_total.clone(),
        );

        let rpc_request_duration_seconds =
            Histogram::new([0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0].into_iter());
        registry.register(
            "rpc_request_duration_seconds",
            "RPC request duration in seconds",
            rpc_request_duration_seconds.clone(),
        );

        let uptime_seconds = Gauge::default();
        registry.register(
            "uptime_seconds",
            "Rucid uptime in seconds",
            uptime_seconds.clone(),
        );

        Self {
            registry: Arc::new(registry),
            jobs_total,
            jobs_running,
            jobs_queued,
            job_duration_seconds,
            rpc_requests_total,
            rpc_request_duration_seconds,
            uptime_seconds,
        }
    }

    /// Increment the total jobs counter
    pub fn inc_jobs_total(&self) {
        self.jobs_total.inc();
    }

    /// Increment the jobs running gauge
    pub fn inc_jobs_running(&self) {
        self.jobs_running.inc();
    }

    /// Decrement the jobs running gauge
    pub fn dec_jobs_running(&self) {
        self.jobs_running.dec();
    }

    /// Set the queued jobs count
    pub fn set_jobs_queued(&self, count: i64) {
        self.jobs_queued.set(count);
    }

    /// Record job duration
    pub fn observe_job_duration(&self, duration_secs: f64) {
        self.job_duration_seconds.observe(duration_secs);
    }

    /// Increment RPC requests
    pub fn inc_rpc_requests(&self) {
        self.rpc_requests_total.inc();
    }

    /// Observe RPC request duration
    pub fn observe_rpc_duration(&self, duration_secs: f64) {
        self.rpc_request_duration_seconds.observe(duration_secs);
    }

    /// Set uptime
    pub fn set_uptime(&self, secs: i64) {
        self.uptime_seconds.set(secs);
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_new() {
        let metrics = Metrics::new();
        // Verify registry is accessible (clone works)
        let _ = metrics.registry.clone();
    }

    #[test]
    fn test_metrics_default() {
        let metrics = Metrics::default();
        let _ = metrics.registry.clone();
    }

    #[test]
    fn test_inc_jobs_total() {
        let metrics = Metrics::new();
        metrics.inc_jobs_total();
        metrics.inc_jobs_total();
        // Counter increments without error
    }

    #[test]
    fn test_inc_jobs_running() {
        let metrics = Metrics::new();
        metrics.inc_jobs_running();
        metrics.inc_jobs_running();
        metrics.dec_jobs_running();
        // Gauge increments and decrements without error
    }

    #[test]
    fn test_set_jobs_queued() {
        let metrics = Metrics::new();
        metrics.set_jobs_queued(5);
        metrics.set_jobs_queued(0);
        // Gauge sets without error
    }

    #[test]
    fn test_observe_job_duration() {
        let metrics = Metrics::new();
        metrics.observe_job_duration(0.5);
        metrics.observe_job_duration(10.0);
        metrics.observe_job_duration(3600.0);
        // Histogram observes without error
    }

    #[test]
    fn test_inc_rpc_requests() {
        let metrics = Metrics::new();
        metrics.inc_rpc_requests();
        metrics.inc_rpc_requests();
        metrics.inc_rpc_requests();
        // Counter increments without error
    }

    #[test]
    fn test_observe_rpc_duration() {
        let metrics = Metrics::new();
        metrics.observe_rpc_duration(0.001);
        metrics.observe_rpc_duration(1.0);
        metrics.observe_rpc_duration(5.0);
        // Histogram observes without error
    }

    #[test]
    fn test_set_uptime() {
        let metrics = Metrics::new();
        metrics.set_uptime(0);
        metrics.set_uptime(3600);
        metrics.set_uptime(86400);
        // Gauge sets without error
    }

    #[test]
    fn test_metrics_operations_sequence() {
        // Test a realistic sequence of operations
        let metrics = Metrics::new();

        metrics.inc_jobs_total();
        metrics.set_jobs_queued(3);
        metrics.inc_jobs_running();
        metrics.observe_job_duration(1.5);
        metrics.inc_rpc_requests();
        metrics.observe_rpc_duration(0.05);
        metrics.dec_jobs_running();
        metrics.set_uptime(100);

        // All operations completed without error
    }
}
