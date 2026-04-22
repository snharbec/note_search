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

    let rows = stmt.query_map([], |row| Ok(row.get::<_, String>(0)?))?;

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
