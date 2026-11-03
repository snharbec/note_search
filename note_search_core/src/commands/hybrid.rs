use crate::commands::search::parse_comma_separated;
use crate::database_service::DatabaseService;
use std::env;
use std::process;

/// One search that ranks segments by keyword relevance and, when
/// `EMBEDDING_MODEL` is set, by meaning too (see
/// `DatabaseService::search_hybrid`).
#[allow(clippy::too_many_arguments)]
pub fn handle_hybrid_search(
    phrase: &str,
    tags: &Option<String>,
    links: &Option<String>,
    limit: usize,
    format: &Option<String>,
    absolute_path: bool,
    database: &str,
) {
    let tags_vec = tags
        .as_deref()
        .map(parse_comma_separated)
        .unwrap_or_default();
    let links_vec = links
        .as_deref()
        .map(parse_comma_separated)
        .unwrap_or_default();

    let base_path = if absolute_path {
        env::var("NOTE_SEARCH_DIR").unwrap_or_else(|_| ".".to_string())
    } else {
        String::new()
    };

    let database_service = DatabaseService::new(database);
    match database_service.search_hybrid(phrase, &tags_vec, &links_vec, limit) {
        Ok(results) if results.is_empty() => println!("No matching segments found."),
        Ok(results) => {
            for (result, score) in results {
                println!(
                    "{:.4}  {}",
                    score,
                    result.formatted_string(format, absolute_path, &base_path)
                );
            }
        }
        Err(e) => {
            eprintln!("Database error: {}", e);
            process::exit(1);
        }
    }
}
