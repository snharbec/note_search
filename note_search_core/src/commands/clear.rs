use std::io::{self, Write};
use std::path::Path;
use std::process;

pub fn handle_clear(database: &str, yes: bool) {
    let db_path = Path::new(database);

    if !db_path.exists() {
        eprintln!("Error: Database '{}' does not exist", database);
        process::exit(1);
    }

    // Confirm unless --yes flag is provided
    if !yes {
        print!(
            "Are you sure you want to clear all data from '{}'? [y/N]: ",
            database
        );
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        let input = input.trim().to_lowercase();
        if input != "y" && input != "yes" {
            println!("Clear operation cancelled.");
            return;
        }
    }

    match clear_database(db_path) {
        Ok(()) => {
            println!("Successfully cleared all data from database '{}'", database);
        }
        Err(e) => {
            eprintln!("Error clearing database: {}", e);
            process::exit(1);
        }
    }
}

pub fn clear_database(db_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use rusqlite::Connection;

    let conn = Connection::open(db_path)?;

    // Delete all data from both tables
    conn.execute("DELETE FROM todo_entries", [])?;
    conn.execute("DELETE FROM markdown_data", [])?;

    // Vacuum to reclaim space
    conn.execute("VACUUM", [])?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown_parser::{write_markdown_data_to_sqlite, Header, MarkdownData};
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn test_clear_database() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");

        // Insert some dummy data
        let data = MarkdownData {
            filename: "test.md".to_string(),
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

        // Verify it exists
        let conn = rusqlite::Connection::open(&db_path)?;
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM markdown_data", [], |row| row.get(0))?;
        assert_eq!(count, 1);

        // Clear
        clear_database(&db_path)?;

        // Verify it is gone
        let count_after: i64 =
            conn.query_row("SELECT COUNT(*) FROM markdown_data", [], |row| row.get(0))?;
        assert_eq!(count_after, 0);

        Ok(())
    }
}
