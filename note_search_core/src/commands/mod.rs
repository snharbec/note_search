pub mod agenda;
pub mod args;
pub mod backlinks;
pub mod browser_history;
pub mod clear;
pub mod convert;
pub mod create_note;
pub mod import;
pub mod info;
pub mod jira;
pub mod linker;
pub mod list_names;
pub mod mapping;
pub mod metadata;
pub mod reconvert;
pub mod search;
pub mod segments;
pub mod similar;

/// Exits the process with an error message if `db_path` doesn't exist.
/// Shared by command handlers that need a database to already be present
/// (as opposed to `import`, which creates one).
pub fn require_db_exists(db_path: &std::path::Path, database: &str) {
    if !db_path.exists() {
        eprintln!("Error: Database '{}' does not exist", database);
        std::process::exit(1);
    }
}
