use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum Job {
    ProcessPhoto { photo_id: String, user_id: String },
    ReprocessAll { user_id: String },
}

pub struct JobQueue {
    sender: mpsc::Sender<Job>,
}

impl JobQueue {
    pub fn new() -> (Self, mpsc::Receiver<Job>) {
        let (tx, rx) = mpsc::channel(1000);
        (Self { sender: tx }, rx)
    }

    pub async fn enqueue(&self, job: Job) -> Result<(), mpsc::error::SendError<Job>> {
        self.sender.send(job).await
    }
}

/// Worker that processes jobs. In production, this would call ML inference.
pub async fn run_worker(mut rx: mpsc::Receiver<Job>) {
    while let Some(job) = rx.recv().await {
        match job {
            Job::ProcessPhoto { photo_id, user_id } => {
                tracing::info!("Processing photo {} for user {}", photo_id, user_id);
                // TODO: call inference engine
            }
            Job::ReprocessAll { user_id } => {
                tracing::info!("Reprocessing all photos for user {}", user_id);
            }
        }
    }
}
