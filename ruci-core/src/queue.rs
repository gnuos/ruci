//! Job queue module
//!
//! Implements producer-consumer pattern using flume

use flume::{Receiver, Sender};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{QueueError, Result};

/// Job queue request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueRequest {
    pub job_id: String,
    pub params: HashMap<String, String>,
    pub run_id: String,
    pub build_num: u64,
}

/// Job queue using flume for async producer-consumer
pub struct JobQueue {
    sender: Sender<QueueRequest>,
    receiver: Receiver<QueueRequest>,
}

impl JobQueue {
    /// Create a new job queue
    pub fn new() -> Self {
        let (sender, receiver) = flume::unbounded();
        Self { sender, receiver }
    }

    /// Enqueue a job (producer side)
    pub async fn enqueue(&self, req: QueueRequest) -> Result<()> {
        let job_id = req.job_id.clone();
        let run_id = req.run_id.clone();
        tracing::debug!(job_id=%job_id, run_id=%run_id, queue_len=%self.len(), "Enqueuing job");

        self.sender.send_async(req).await.map_err(|e| {
            tracing::error!(job_id=%job_id, error=%e, "Failed to enqueue job");
            crate::error::Error::Queue(QueueError::SendFailed(format!("Queue send failed: {}", e)))
        })?;

        tracing::info!(job_id=%job_id, run_id=%run_id, "Job enqueued successfully");
        Ok(())
    }

    /// Dequeue a job (consumer side)
    pub async fn dequeue(&self) -> Option<QueueRequest> {
        let req = self.receiver.recv_async().await.ok();
        if let Some(ref r) = req {
            tracing::debug!(job_id=%r.job_id, run_id=%r.run_id, queue_len=%self.len(), "Dequeued job");
        }
        req
    }

    /// Get queue length
    pub fn len(&self) -> usize {
        self.sender.len()
    }

    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.sender.is_empty()
    }

    /// Rehydrate the queue from a list of queued requests (used on startup recovery)
    pub async fn rehydrate(&self, requests: Vec<QueueRequest>) -> Result<()> {
        for req in requests {
            self.enqueue(req).await?;
        }
        Ok(())
    }
}

impl Default for JobQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Wrapper for sending job requests
#[derive(Clone)]
pub struct JobQueueSender {
    sender: Sender<QueueRequest>,
}

impl JobQueueSender {
    pub fn new(queue: &JobQueue) -> Self {
        Self {
            sender: queue.sender.clone(),
        }
    }

    pub async fn enqueue(&self, req: QueueRequest) -> Result<()> {
        let job_id = req.job_id.clone();
        let run_id = req.run_id.clone();
        tracing::debug!(job_id=%job_id, run_id=%run_id, "Enqueuing job via sender");

        self.sender.send_async(req).await.map_err(|e| {
            tracing::error!(job_id=%job_id, error=%e, "Failed to enqueue job");
            crate::error::Error::Queue(QueueError::SendFailed(format!("Queue send failed: {}", e)))
        })?;

        tracing::info!(job_id=%job_id, run_id=%run_id, "Job enqueued successfully");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_request(job_id: &str, run_id: &str) -> QueueRequest {
        QueueRequest {
            job_id: job_id.to_string(),
            run_id: run_id.to_string(),
            params: HashMap::new(),
            build_num: 1,
        }
    }

    #[tokio::test]
    async fn test_job_queue_new() {
        let queue = JobQueue::new();
        assert!(queue.is_empty());
        assert_eq!(queue.len(), 0);
    }

    #[tokio::test]
    async fn test_job_queue_enqueue_dequeue() {
        let queue = JobQueue::new();
        let req = create_test_request("job1", "run1");

        queue.enqueue(req).await.expect("Failed to enqueue");

        assert!(!queue.is_empty());
        assert_eq!(queue.len(), 1);

        let dequeued = queue.dequeue().await;
        assert!(dequeued.is_some());
        assert_eq!(dequeued.unwrap().job_id, "job1");
    }

    #[tokio::test]
    async fn test_job_queue_multiple_items() {
        let queue = JobQueue::new();

        for i in 0..5 {
            let req = create_test_request(&format!("job{}", i), &format!("run{}", i));
            queue.enqueue(req).await.expect("Failed to enqueue");
        }

        assert_eq!(queue.len(), 5);

        for i in 0..5 {
            let dequeued = queue.dequeue().await.unwrap();
            assert_eq!(dequeued.job_id, format!("job{}", i));
        }

        assert!(queue.is_empty());
    }

    #[tokio::test]
    async fn test_job_queue_sender() {
        let queue = JobQueue::new();
        let sender = JobQueueSender::new(&queue);

        let req = create_test_request("job-sender", "run-sender");
        sender
            .enqueue(req)
            .await
            .expect("Failed to enqueue via sender");

        let dequeued = queue.dequeue().await.unwrap();
        assert_eq!(dequeued.job_id, "job-sender");
    }

    #[test]
    fn test_queue_request_serialization() {
        let req = create_test_request("job1", "run1");
        let serialized = serde_json::to_string(&req).expect("Failed to serialize");
        let deserialized: QueueRequest =
            serde_json::from_str(&serialized).expect("Failed to deserialize");

        assert_eq!(deserialized.job_id, req.job_id);
        assert_eq!(deserialized.run_id, req.run_id);
        assert_eq!(deserialized.build_num, req.build_num);
    }

    #[test]
    fn test_queue_request_serialization_with_params() {
        let mut params = HashMap::new();
        params.insert("key1".to_string(), "value1".to_string());
        params.insert("key2".to_string(), "value2".to_string());

        let req = QueueRequest {
            job_id: "job-params".to_string(),
            run_id: "run-params".to_string(),
            params,
            build_num: 42,
        };

        let serialized = serde_json::to_string(&req).expect("Failed to serialize");
        let deserialized: QueueRequest =
            serde_json::from_str(&serialized).expect("Failed to deserialize");

        assert_eq!(deserialized.job_id, "job-params");
        assert_eq!(deserialized.run_id, "run-params");
        assert_eq!(deserialized.build_num, 42);
        assert_eq!(deserialized.params.get("key1"), Some(&"value1".to_string()));
        assert_eq!(deserialized.params.get("key2"), Some(&"value2".to_string()));
    }

    #[tokio::test]
    async fn test_job_queue_sender_clone() {
        let queue = JobQueue::new();
        let sender1 = JobQueueSender::new(&queue);
        let sender2 = sender1.clone();

        // Both senders should be able to enqueue
        let req1 = create_test_request("job-clone-1", "run-clone-1");
        let req2 = create_test_request("job-clone-2", "run-clone-2");

        sender1
            .enqueue(req1)
            .await
            .expect("Failed to enqueue from sender1");
        sender2
            .enqueue(req2)
            .await
            .expect("Failed to enqueue from sender2");

        assert_eq!(queue.len(), 2);

        // Dequeue both
        let d1 = queue.dequeue().await.unwrap();
        let d2 = queue.dequeue().await.unwrap();

        // Order may vary due to async, but both should be received
        assert_eq!(queue.len(), 0);
    }

    #[tokio::test]
    async fn test_job_queue_len_consistency() {
        let queue = JobQueue::new();
        assert_eq!(queue.len(), 0);
        assert!(queue.is_empty());

        // Add items
        for i in 0..10 {
            let req = create_test_request(&format!("job{}", i), &format!("run{}", i));
            queue.enqueue(req).await.expect("Failed to enqueue");
            assert_eq!(queue.len(), i + 1);
        }

        // Remove items
        for i in 0..10 {
            queue.dequeue().await.expect("Failed to dequeue");
            assert_eq!(queue.len(), 9 - i);
        }

        assert!(queue.is_empty());
    }

    #[tokio::test]
    async fn test_job_queue_dequeue_empty() {
        let queue = JobQueue::new();
        // Dequeue from empty queue should return None within timeout
        let result =
            tokio::time::timeout(std::time::Duration::from_millis(100), queue.dequeue()).await;

        // Should timeout (return Err) because queue is empty
        assert!(result.is_err() || result.unwrap().is_none());
    }

    #[test]
    fn test_queue_request_debug() {
        let req = create_test_request("job-debug", "run-debug");
        let debug_str = format!("{:?}", req);
        assert!(debug_str.contains("job-debug"));
        assert!(debug_str.contains("run-debug"));
    }

    #[tokio::test]
    async fn test_job_queue_interleaved_enqueue_dequeue() {
        let queue = JobQueue::new();

        // Enqueue first item
        let req1 = create_test_request("job-1", "run-1");
        queue.enqueue(req1).await.expect("Failed to enqueue");

        // Dequeue first item
        let d1 = queue.dequeue().await.unwrap();
        assert_eq!(d1.job_id, "job-1");

        // Enqueue two more
        let req2 = create_test_request("job-2", "run-2");
        let req3 = create_test_request("job-3", "run-3");
        queue.enqueue(req2).await.expect("Failed to enqueue");
        queue.enqueue(req3).await.expect("Failed to enqueue");

        // Dequeue second item
        let d2 = queue.dequeue().await.unwrap();
        assert_eq!(d2.job_id, "job-2");

        // Dequeue third item
        let d3 = queue.dequeue().await.unwrap();
        assert_eq!(d3.job_id, "job-3");

        assert!(queue.is_empty());
    }

    #[tokio::test]
    async fn test_job_queue_multiple_dequeue() {
        let queue = JobQueue::new();

        // Enqueue 3 items
        for i in 0..3 {
            let req = create_test_request(&format!("job{}", i), &format!("run{}", i));
            queue.enqueue(req).await.expect("Failed to enqueue");
        }

        // Dequeue all using known count
        assert_eq!(queue.len(), 3);
        for i in 0..3 {
            let dequeued = queue.dequeue().await.unwrap();
            assert_eq!(dequeued.job_id, format!("job{}", i));
        }
        assert!(queue.is_empty());
    }

    #[tokio::test]
    async fn test_job_queue_enqueue_after_empty() {
        let queue = JobQueue::new();

        // Enqueue and dequeue
        let req1 = create_test_request("job1", "run1");
        queue.enqueue(req1).await.expect("Failed to enqueue");
        queue.dequeue().await.unwrap();

        // Enqueue again after queue becomes empty
        let req2 = create_test_request("job2", "run2");
        queue.enqueue(req2).await.expect("Failed to enqueue");
        assert_eq!(queue.len(), 1);

        let dequeued = queue.dequeue().await.unwrap();
        assert_eq!(dequeued.job_id, "job2");
    }

    #[test]
    fn test_queue_request_with_empty_params() {
        let req = QueueRequest {
            job_id: "job-empty".to_string(),
            run_id: "run-empty".to_string(),
            params: HashMap::new(),
            build_num: 1,
        };

        assert!(req.params.is_empty());
        assert_eq!(req.job_id, "job-empty");
    }

    #[test]
    fn test_queue_request_params() {
        let mut params = HashMap::new();
        params.insert("BRANCH".to_string(), "main".to_string());
        params.insert("COMMIT".to_string(), "abc123".to_string());

        let req = QueueRequest {
            job_id: "job-params".to_string(),
            run_id: "run-params".to_string(),
            params,
            build_num: 5,
        };

        assert_eq!(req.params.len(), 2);
        assert_eq!(req.params.get("BRANCH"), Some(&"main".to_string()));
        assert_eq!(req.params.get("COMMIT"), Some(&"abc123".to_string()));
    }

    #[tokio::test]
    async fn test_job_queue_sender_multi_clone() {
        let queue = JobQueue::new();
        let sender1 = JobQueueSender::new(&queue);
        let sender2 = sender1.clone();
        let sender3 = sender1.clone();

        // All senders can enqueue
        for i in 0..3 {
            let req = create_test_request(&format!("job{}", i), &format!("run{}", i));
            sender1
                .enqueue(req)
                .await
                .expect("Failed to enqueue from sender1");
        }

        let req4 = create_test_request("job3", "run3");
        sender2
            .enqueue(req4)
            .await
            .expect("Failed to enqueue from sender2");

        let req5 = create_test_request("job4", "run4");
        sender3
            .enqueue(req5)
            .await
            .expect("Failed to enqueue from sender3");

        assert_eq!(queue.len(), 5);

        // Dequeue all using known count
        for _ in 0..5 {
            queue.dequeue().await.unwrap();
        }
        assert!(queue.is_empty());
    }

    #[tokio::test]
    async fn test_job_queue_large_params() {
        let queue = JobQueue::new();

        let mut params = HashMap::new();
        for i in 0..100 {
            params.insert(format!("KEY_{}", i), format!("VALUE_{}", i));
        }

        let req = QueueRequest {
            job_id: "job-large".to_string(),
            run_id: "run-large".to_string(),
            params,
            build_num: 1,
        };

        queue.enqueue(req).await.expect("Failed to enqueue");

        let dequeued = queue.dequeue().await.unwrap();
        assert_eq!(dequeued.params.len(), 100);
    }

    #[test]
    fn test_queue_request_clone() {
        let req = create_test_request("job-clone", "run-clone");
        let cloned = req.clone();

        assert_eq!(cloned.job_id, req.job_id);
        assert_eq!(cloned.run_id, req.run_id);
        assert_eq!(cloned.build_num, req.build_num);
    }

    #[tokio::test]
    async fn test_job_queue_fifo_order() {
        let queue = JobQueue::new();

        // Enqueue in order
        for i in 0..10 {
            let req = create_test_request(&format!("job{}", i), &format!("run{}", i));
            queue.enqueue(req).await.expect("Failed to enqueue");
        }

        // Dequeue should be FIFO
        for i in 0..10 {
            let dequeued = queue.dequeue().await.unwrap();
            assert_eq!(dequeued.job_id, format!("job{}", i));
        }
    }
}
