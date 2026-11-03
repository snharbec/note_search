use serde::Deserialize;
use std::sync::LazyLock;
use std::time::Duration;

const DEFAULT_OLLAMA_HOST: &str = "http://localhost:11434";

/// Env var naming the Ollama embedding model to use (e.g. `nomic-embed-text`).
/// Unset means embeddings are disabled - no calls are made on import.
const EMBEDDING_MODEL_ENV: &str = "EMBEDDING_MODEL";

/// Shared client so repeated embed_text calls (one per changed segment
/// during import) reuse a connection instead of paying a fresh TCP/TLS
/// handshake each time.
static HTTP_CLIENT: LazyLock<reqwest::blocking::Client> = LazyLock::new(|| {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("failed to build reqwest client")
});

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
/// erroring. Long texts should go through `embed_chunked` instead, so their
/// tail isn't lost to that truncation.
pub fn embed_text(text: &str) -> Result<Vec<f32>, String> {
    embed_inputs(&[text])?
        .pop()
        .ok_or_else(|| "Ollama returned no embeddings".to_string())
}

/// Embed `text` as one vector per `CHUNK_CHARS`-sized chunk (one Ollama
/// call), concatenated. Similarity scoring takes the best-matching chunk,
/// so a phrase that only matches the end of a long section still finds it.
pub fn embed_chunked(text: &str) -> Result<Vec<f32>, String> {
    let chunks = chunk_text(text, CHUNK_CHARS);
    let refs: Vec<&str> = chunks.iter().map(String::as_str).collect();
    Ok(embed_inputs(&refs)?.concat())
}

fn embed_inputs(inputs: &[&str]) -> Result<Vec<Vec<f32>>, String> {
    let model = std::env::var(EMBEDDING_MODEL_ENV)
        .map_err(|_| format!("{} not set", EMBEDDING_MODEL_ENV))?;
    let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| DEFAULT_OLLAMA_HOST.to_string());
    let url = format!("{}/api/embed", host.trim_end_matches('/'));

    let response = HTTP_CLIENT
        .post(&url)
        .json(&serde_json::json!({ "model": model, "input": inputs }))
        .send()
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("Ollama returned status {}", response.status()));
    }

    let parsed: EmbeddingResponse = response.json().map_err(|e| e.to_string())?;
    if parsed.embeddings.len() != inputs.len() {
        return Err("Ollama returned the wrong number of embeddings".to_string());
    }
    Ok(parsed.embeddings)
}

/// Max characters per embedded chunk - about 500 tokens, comfortably inside
/// the context window of common Ollama embedding models.
pub const CHUNK_CHARS: usize = 2000;

/// Split `text` into chunks of at most `max_chars` characters, breaking at
/// paragraph boundaries where possible and hard-splitting longer paragraphs.
fn chunk_text(text: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for para in text.split("\n\n") {
        let chars: Vec<char> = para.chars().collect();
        for piece in chars.chunks(max_chars) {
            let piece: String = piece.iter().collect();
            if !current.is_empty()
                && current.chars().count() + 2 + piece.chars().count() > max_chars
            {
                chunks.push(std::mem::take(&mut current));
            }
            if !current.is_empty() {
                current.push_str("\n\n");
            }
            current.push_str(&piece);
        }
    }
    if !current.is_empty() || chunks.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Best cosine similarity between `query` and any chunk vector in
/// `stored` (one or more vectors of `query.len()` concatenated, as written
/// by `embed_chunked`). 0.0 if `stored` isn't a whole number of vectors of
/// that size, e.g. the embedding model changed since import.
pub fn best_chunk_similarity(query: &[f32], stored: &[f32]) -> f32 {
    if query.is_empty() || stored.is_empty() || !stored.len().is_multiple_of(query.len()) {
        return 0.0;
    }
    stored
        .chunks(query.len())
        .map(|chunk| cosine_similarity(query, chunk))
        .fold(f32::MIN, f32::max)
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
    fn test_chunk_text_splits_long_text_at_paragraphs() {
        let para = "a".repeat(1500);
        let text = format!("{}\n\n{}\n\n{}", para, para, "short");
        let chunks = chunk_text(&text, 2000);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], para);
        assert_eq!(chunks[1], format!("{}\n\nshort", para));
        assert!(chunks.iter().all(|c| c.chars().count() <= 2000));
    }

    #[test]
    fn test_chunk_text_hard_splits_oversized_paragraph() {
        let chunks = chunk_text(&"ü".repeat(4500), 2000);
        assert_eq!(chunks.iter().map(|c| c.chars().count()).collect::<Vec<_>>(), vec![2000, 2000, 500]);
    }

    #[test]
    fn test_chunk_text_short_text_is_one_chunk() {
        assert_eq!(chunk_text("hello", 2000), vec!["hello".to_string()]);
    }

    #[test]
    fn test_best_chunk_similarity_takes_best_chunk() {
        let stored = [0.0, 1.0, 1.0, 0.0];
        assert!((best_chunk_similarity(&[1.0, 0.0], &stored) - 1.0).abs() < 1e-6);
        assert_eq!(best_chunk_similarity(&[1.0, 0.0, 0.0], &stored), 0.0);
    }

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
