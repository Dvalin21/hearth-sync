use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum Job {
    ProcessPhoto { photo_id: String, user_id: String },
    ReprocessAll { user_id: String },
}

pub struct JobQueue {
    pub sender: mpsc::Sender<Job>,
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

/// Worker scaffold. ML inference currently runs synchronously in the route
/// handlers (see src/routes/ml.rs); this queue is reserved for moving that
/// work off the request path. Until wired into AppState + main, it is unused.
pub async fn run_worker(mut rx: mpsc::Receiver<Job>, model_dir: std::path::PathBuf, db: crate::db::DbPool) {
    while let Some(job) = rx.recv().await {
        match job {
            Job::ProcessPhoto { photo_id, user_id } => {
                tracing::info!("Processing photo {} for user {}", photo_id, user_id);
                // Run inference if available; store embedding on success.
                let engine = crate::ml::InferenceEngine::new(model_dir.clone());
                if engine.is_available() {
                    let pool = db.clone();
                    let pid = photo_id.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        // Best-effort: log; full embedding storage requires photo bytes.
                        tracing::debug!("inference available for {} (async path)", pid);
                        let _ = pool;
                    }).await;
                }
            }
            Job::ReprocessAll { user_id } => {
                tracing::info!("Reprocessing all photos for user {}", user_id);
            }
        }
    }
}
