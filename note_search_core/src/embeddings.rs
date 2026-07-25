use serde::Deserialize;
use std::time::Duration;

const DEFAULT_OLLAMA_HOST: &str = "http://localhost:11434";
const EMBEDDING_MODEL: &str = "nomic-embed-text";

#[derive(Deserialize)]
struct EmbeddingResponse {
    embeddings: Vec<Vec<f32>>,
}

/// Compute an embedding vector for `text` using a local Ollama instance
/// running the `nomic-embed-text` model. The host defaults to
/// `http://localhost:11434` and can be overridden with the `OLLAMA_HOST`
/// environment variable.
///
/// Uses `/api/embed` (not the older `/api/embeddings`) because it silently
/// truncates input that exceeds the model's context window instead of
/// erroring - segments built from unheaded/PDF-converted notes can easily
/// run to hundreds of KB in one chunk, well past `nomic-embed-text`'s
/// context length.
pub fn embed_text(text: &str) -> Result<Vec<f32>, String> {
    let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| DEFAULT_OLLAMA_HOST.to_string());
    let url = format!("{}/api/embed", host.trim_end_matches('/'));

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let response = client
        .post(&url)
        .json(&serde_json::json!({ "model": EMBEDDING_MODEL, "input": text }))
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
}
