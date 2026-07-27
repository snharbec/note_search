use crate::commands::search::parse_comma_separated;
use crate::database_service::DatabaseService;
use crate::embeddings::embed_text;
use std::env;
use std::process;

/// Search segments by meaning rather than keyword: embed `phrase` with the
/// same local Ollama model used at import time, then rank every segment
/// that already has a stored embedding by cosine similarity. `--tags`/
/// `--links` restrict the candidate set before ranking, same AND semantics
/// as `segments --tags`/`--links`.
#[allow(clippy::too_many_arguments)]
pub fn handle_similar_search(
    phrase: &str,
    tags: &Option<String>,
    links: &Option<String>,
    limit: usize,
    format: &Option<String>,
    absolute_path: bool,
    database: &str,
) {
    let phrase_embedding = match embed_text(phrase) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error computing embedding for phrase: {}", e);
            process::exit(1);
        }
    };

    let tags_vec = tags.as_deref().map(parse_comma_separated).unwrap_or_default();
    let links_vec = links.as_deref().map(parse_comma_separated).unwrap_or_default();

    let base_path = if absolute_path {
        env::var("NOTE_SEARCH_DIR").unwrap_or_else(|_| ".".to_string())
    } else {
        String::new()
    };

    let database_service = DatabaseService::new(database);
    match database_service.search_similar_segments(&phrase_embedding, &tags_vec, &links_vec, limit)
    {
        Ok(results) => {
            if results.is_empty() {
                println!("No matching segments found.");
            } else {
                for (result, score) in results {
                    println!(
                        "{:.4}  {}",
                        score,
                        result.formatted_string(format, absolute_path, &base_path)
                    );
                }
            }
        }
        Err(e) => {
            eprintln!("Database error: {}", e);
            process::exit(1);
        }
    }
}
