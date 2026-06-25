use crate::commands::backlinks::get_backlinks;
use crate::database_service::DatabaseService;
use rusqlite::Connection;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::process;

pub fn handle_info(database: &str, filename: &str) {
    let _db_service = DatabaseService::new(database);
    let db_path = Path::new(database);

    if !db_path.exists() {
        eprintln!("Error: Database '{}' does not exist", database);
        process::exit(1);
    }

    // First try exact match
    match get_note_info(db_path, filename) {
        Ok(info) => print_note_info(db_path, info),
        Err(_) => {
            // If not found, try searching for any filename that ends with /filename or equals filename
            match search_filename_matches(db_path, filename) {
                Ok(matches) => {
                    if matches.is_empty() {
                        eprintln!("Error: Document '{}' not found", filename);
                        process::exit(1);
                    } else if matches.len() == 1 {
                        match get_note_info(db_path, &matches[0]) {
                            Ok(info) => print_note_info(db_path, info),
                            Err(e) => eprintln!("Error getting note info: {}", e),
                        }
                    } else {
                        println!("Multiple documents found for '{}':", filename);
                        for m in matches {
                            println!("  - {}", m);
                        }
                        println!("\nPlease specify the full path.");
                    }
                }
                Err(e) => {
                    eprintln!("Error searching for document: {}", e);
                    process::exit(1);
                }
            }
        }
    }
}

fn print_note_info(db_path: &Path, info: NoteInfo) {
    println!("Filename: {}", info.filename);
    println!("Title: {}", info.title.unwrap_or_else(|| "N/A".to_string()));
    println!("Created: {}", info.created);
    println!("Updated: {}", info.updated);

    println!("\nAttributes:");
    if let Some(fields) = info.header_fields {
        if let Ok(map) = serde_json::from_str::<HashMap<String, Value>>(&fields) {
            for (k, v) in map {
                println!("  {}: {}", k, v);
            }
        }
    } else {
        println!("  None");
    }

    println!("\nLinks:");
    if let Some(links_json) = info.links {
        if let Ok(links) = serde_json::from_str::<Vec<String>>(&links_json) {
            if links.is_empty() {
                println!("  None");
            } else {
                for link in links {
                    println!("  - {}", link);
                }
            }
        }
    } else {
        println!("  None");
    }

    println!("\nBacklinks:");
    match get_backlinks(db_path, &info.filename) {
        Ok(backlinks) => {
            if backlinks.is_empty() {
                println!("  None");
            } else {
                for backlink in backlinks {
                    println!("  - {}", backlink);
                }
            }
        }
        Err(e) => eprintln!("  Error: {}", e),
    }
}

fn search_filename_matches(
    db_path: &Path,
    filename: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let conn = Connection::open(db_path)?;
    let mut stmt =
        conn.prepare("SELECT filename FROM markdown_data WHERE filename = ? OR filename LIKE ?")?;
    let pattern = format!("%/{}", filename);
    let rows = stmt.query_map([filename, &pattern], |row| row.get(0))?;

    let mut matches = Vec::new();
    for row in rows {
        matches.push(row?);
    }
    Ok(matches)
}

pub struct NoteInfo {
    pub filename: String,
    pub title: Option<String>,
    pub created: i64,
    pub updated: i64,
    pub header_fields: Option<String>,
    pub links: Option<String>,
}

fn get_note_info(db_path: &Path, filename: &str) -> Result<NoteInfo, Box<dyn std::error::Error>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT filename, title, created, updated, header_fields, links FROM markdown_data WHERE filename = ?",
    )?;

    let mut rows = stmt.query([filename])?;
    if let Some(row) = rows.next()? {
        Ok(NoteInfo {
            filename: row.get(0)?,
            title: row.get(1)?,
            created: row.get(2)?,
            updated: row.get(3)?,
            header_fields: row.get(4)?,
            links: row.get(5)?,
        })
    } else {
        Err(format!("Document '{}' not found", filename).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown_parser::{write_markdown_data_to_sqlite, Header, MarkdownData};
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn test_search_filename_matches() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");

        let data = MarkdownData {
            filename: "projects/test.md".to_string(),
            created: 1234567890,
            updated: 1234567890,
            title: "Test".to_string(),
            header: Header {
                fields: HashMap::new(),
            },
            todo: vec![],
            link: vec![],
            body: "".to_string(),
        };

        write_markdown_data_to_sqlite(&data, &db_path)?;

        let matches = search_filename_matches(&db_path, "test.md")?;
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0], "projects/test.md");

        let matches_exact = search_filename_matches(&db_path, "projects/test.md")?;
        assert_eq!(matches_exact.len(), 1);
        assert_eq!(matches_exact[0], "projects/test.md");

        let matches_none = search_filename_matches(&db_path, "nonexistent.md")?;
        assert!(matches_none.is_empty());

        Ok(())
    }
}
