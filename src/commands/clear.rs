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
