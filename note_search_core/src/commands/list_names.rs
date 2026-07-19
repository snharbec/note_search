use std::path::Path;
use std::process;

pub fn handle_list_names(database: &str) {
    let db_path = Path::new(database);

    if !db_path.exists() {
        eprintln!("Error: Database '{}' does not exist", database);
        process::exit(1);
    }

    match get_note_names(db_path) {
        Ok(names) => {
            if names.is_empty() {
                println!("No notes found in database.");
            } else {
                for name in names {
                    println!("{}", name);
                }
            }
        }
        Err(e) => {
            eprintln!("Error getting note names: {}", e);
            process::exit(1);
        }
    }
}

pub fn get_note_names(db_path: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    use rusqlite::Connection;
    use std::collections::HashSet;

    let conn = Connection::open(db_path)?;
    let mut names = HashSet::new();

    // Get unique filenames from markdown_data table, extract basename without path and .md extension
    let mut stmt =
        conn.prepare("SELECT DISTINCT filename FROM markdown_data WHERE filename IS NOT NULL")?;

    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;

    for row in rows {
        let filename = row?;
        // Extract basename without path
        let base = Path::new(&filename)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| filename.clone());
        // Remove .md extension if present
        let name = base.trim_end_matches(".md").to_string();
        names.insert(name);
    }

    // Convert HashSet to sorted Vec
    let mut result: Vec<String> = names.into_iter().collect();
    result.sort();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown_parser::{write_markdown_data_to_sqlite, Header, MarkdownData};
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn test_get_note_names() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");

        // Insert some dummy data
        let files = vec!["test1.md", "subdir/test2.md", "other/test3.md"];
        for file in files {
            let data = MarkdownData {
                filename: file.to_string(),
                created: 1234567890,
                updated: 1234567890,
                title: "Test".to_string(),
                header: Header {
                    fields: HashMap::new(),
                },
                todo: vec![],
                link: vec![],
                body: "".to_string(),
                elements: vec![],
            };
            write_markdown_data_to_sqlite(&data, &db_path)?;
        }

        let names = get_note_names(&db_path)?;
        assert_eq!(names.len(), 3);
        assert_eq!(names, vec!["test1", "test2", "test3"]);

        Ok(())
    }
}
