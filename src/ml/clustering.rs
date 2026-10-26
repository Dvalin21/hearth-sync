use crate::ml::embeddings::cosine_similarity;

/// DBSCAN clustering for face embeddings.
/// Returns cluster labels: -1 for noise, 0..n for clusters.
pub fn cluster_faces(embeddings: &[Vec<f32>], eps: f32, min_points: usize) -> Vec<i32> {
    let n = embeddings.len();
    if n == 0 {
        return vec![];
    }
    let mut labels = vec![-1i32; n];
    let mut cluster_id = 0i32;

    // Precompute neighbor lists
    let neighbors: Vec<Vec<usize>> = (0..n)
        .map(|i| {
            (0..n)
                .filter(|&j| i != j && cosine_similarity(&embeddings[i], &embeddings[j]) >= eps)
                .collect()
        })
        .collect();

    for i in 0..n {
        if labels[i] != -1 {
            continue;
        }
        if neighbors[i].len() < min_points {
            continue; // noise
        }
        // Start new cluster
        labels[i] = cluster_id;
        let mut queue = vec![i];
        while let Some(q) = queue.pop() {
            
            if neighbors[q].len() < min_points {
                continue;
            }
            for &nb in &neighbors[q] {
                if labels[nb] == -1 {
                    labels[nb] = cluster_id;
                    queue.push(nb);
                }
            }
        }
        cluster_id += 1;
    }
    labels
}
