use serde::Deserialize;
use std::time::Duration;

const DEFAULT_OLLAMA_HOST: &str = "http://localhost:11434";

/// Env var naming the Ollama embedding model to use (e.g. `nomic-embed-text`).
/// Unset means embeddings are disabled - no calls are made on import.
const EMBEDDING_MODEL_ENV: &str = "EMBEDDING_MODEL";

#[derive(Deserialize)]
struct EmbeddingResponse {
    embeddings: Vec<Vec<f32>>,
}

/// Whether embedding generation is configured, i.e. `EMBEDDING_MODEL` is set.
pub fn embeddings_enabled() -> bool {
    std::env::var(EMBEDDING_MODEL_ENV).is_ok()
}

/// Compute an embedding vector for `text` using a local Ollama instance
/// running the model named by the `EMBEDDING_MODEL` environment variable
/// (e.g. `nomic-embed-text`). The host defaults to `http://localhost:11434`
/// and can be overridden with the `OLLAMA_HOST` environment variable.
///
/// Uses `/api/embed` (not the older `/api/embeddings`) because it silently
/// truncates input that exceeds the model's context window instead of
/// erroring - segments built from unheaded/PDF-converted notes can easily
/// run to hundreds of KB in one chunk, well past the model's context length.
pub fn embed_text(text: &str) -> Result<Vec<f32>, String> {
    let model = std::env::var(EMBEDDING_MODEL_ENV)
        .map_err(|_| format!("{} not set", EMBEDDING_MODEL_ENV))?;
    let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| DEFAULT_OLLAMA_HOST.to_string());
    let url = format!("{}/api/embed", host.trim_end_matches('/'));

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let response = client
        .post(&url)
        .json(&serde_json::json!({ "model": model, "input": text }))
        .send()
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("Ollama returned status {}", response.status()));
    }

    let mut parsed: EmbeddingResponse = response.json().map_err(|e| e.to_string())?;
    parsed
        .embeddings
        .pop()
        .ok_or_else(|| "Ollama returned no embeddings".to_string())
}

/// Serialize an embedding vector to a compact little-endian byte blob, for
/// storage in a SQLite BLOB column.
pub fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(embedding.len() * 4);
    for value in embedding {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// Inverse of `embedding_to_bytes`.
pub fn bytes_to_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

/// Cosine similarity between two embedding vectors, in [-1.0, 1.0] for
/// non-zero vectors (nomic-embed-text vectors are typically all positive
/// similarity in practice, but this doesn't assume that). Returns 0.0 for
/// mismatched lengths or a zero vector, rather than panicking or dividing by
/// zero.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedding_bytes_round_trip() {
        let embedding = vec![0.0_f32, 1.5, -2.25, f32::MIN, f32::MAX];
        let bytes = embedding_to_bytes(&embedding);
        assert_eq!(bytes.len(), embedding.len() * 4);
        assert_eq!(bytes_to_embedding(&bytes), embedding);
    }

    #[test]
    fn test_embedding_to_bytes_empty() {
        assert!(embedding_to_bytes(&[]).is_empty());
    }

    #[test]
    fn test_cosine_similarity_identical_vectors() {
        let v = vec![1.0_f32, 2.0, 3.0];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal_vectors() {
        let a = vec![1.0_f32, 0.0];
        let b = vec![0.0_f32, 1.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_opposite_vectors() {
        let a = vec![1.0_f32, 0.0];
        let b = vec![-1.0_f32, 0.0];
        assert!((cosine_similarity(&a, &b) - (-1.0)).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_mismatched_lengths_returns_zero() {
        assert_eq!(cosine_similarity(&[1.0, 2.0], &[1.0]), 0.0);
    }

    #[test]
    fn test_cosine_similarity_zero_vector_returns_zero() {
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
    }
}
