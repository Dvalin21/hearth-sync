pub mod embeddings;
pub mod clustering;
pub mod jobs;
pub mod inference;

pub use embeddings::{cosine_similarity, store_embedding, get_embedding, search_similar, vec_to_bytes, bytes_to_vec};
pub use clustering::cluster_faces;
pub use jobs::{Job, JobQueue, run_worker};
pub use inference::{FaceDetection, InferenceEngine};
