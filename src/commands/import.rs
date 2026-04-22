use std::fs;
use std::path::Path;
use std::process;
use std::time::{Duration, SystemTime};

use crate::markdown_parser;

pub fn handle_import(default_db: &str, input: &str, output: Option<&str>) {
    let db_path = output.unwrap_or(default_db);
    let input_path = Path::new(input);

    if !input_path.exists() {
        eprintln!("Error: Input directory '{}' does not exist", input);
        process::exit(1);
    }

    if !input_path.is_dir() {
        eprintln!("Error: Input path '{}' is not a directory", input);
        process::exit(1);
    }

    println!(
        "Importing markdown files from '{}' to database '{}'...",
        input, db_path
    );

    // Track file modification times to detect changes
    let mut file_mtimes: std::collections::HashMap<std::path::PathBuf, SystemTime> =
        std::collections::HashMap::new();

    match do_import_with_tracking(input_path, Path::new(db_path), &mut file_mtimes) {
        Ok(count) => {
            println!(
                "Successfully imported {} markdown files to database '{}'",
                count, db_path
            );
        }
        Err(e) => {
            eprintln!("Error importing markdown files: {}", e);
            process::exit(1);
        }
    }
}

pub fn handle_watch_import(default_db: &str, input: &str, output: Option<&str>, interval: u64) {
    let db_path = output.unwrap_or(default_db);
    let input_path = Path::new(input);

    if !input_path.exists() {
        eprintln!("Error: Input directory '{}' does not exist", input);
        process::exit(1);
    }

    if !input_path.is_dir() {
        eprintln!("Error: Input path '{}' is not a directory", input);
        process::exit(1);
    }

    let interval_duration = Duration::from_secs(interval);
    let mut file_mtimes: std::collections::HashMap<std::path::PathBuf, SystemTime> =
        std::collections::HashMap::new();

    println!(
        "Starting watch mode for directory '{}' (checking every {} seconds)",
        input, interval
    );
    println!("Press Ctrl+C to stop watching...\n");

    // Do initial import - imports all files and replaces existing content
    match do_import_with_tracking(input_path, Path::new(db_path), &mut file_mtimes) {
        Ok(count) => {
            if count > 0 {
                println!("Initial import: {} files imported/updated", count);
            }
        }
        Err(e) => {
            eprintln!("Error during initial import: {}", e);
        }
    }

    // Watch loop
    loop {
        std::thread::sleep(interval_duration);

        match do_import_with_tracking(input_path, Path::new(db_path), &mut file_mtimes) {
            Ok(count) => {
                if count > 0 {
                    println!(
                        "Watch update: {} files imported/updated at {}",
                        count,
                        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
                    );
                }
            }
            Err(e) => {
                eprintln!("Error during watch import: {}", e);
            }
        }
    }
}

/// Imports markdown files and tracks modification times
/// Returns count of files that were newly imported or modified
pub fn do_import_with_tracking(
    input_dir: &Path,
    db_path: &Path,
    file_mtimes: &mut std::collections::HashMap<std::path::PathBuf, SystemTime>,
) -> Result<usize, Box<dyn std::error::Error>> {
    use rusqlite::Connection;

    let mut conn = Connection::open(db_path)?;
    markdown_parser::init_database_schema(&conn)?;

    let mut files_to_import: Vec<(std::path::PathBuf, SystemTime)> = Vec::new();

    for entry in walkdir::WalkDir::new(input_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file() && e.path().extension().map_or(false, |ext| ext == "md")
        })
    {
        let path = entry.path().to_path_buf();
        let current_mtime = fs::metadata(&path)?.modified()?;

        let should_import = match file_mtimes.get(&path) {
            Some(&last_mtime) => current_mtime != last_mtime,
            None => true,
        };

        if should_import {
            files_to_import.push((path, current_mtime));
        }
    }

    if files_to_import.is_empty() {
        return Ok(0);
    }

    let tx = conn.transaction()?;
    let mut updated_count = 0;

    for (path, current_mtime) in files_to_import {
        let data = markdown_parser::process_markdown_file(&path, input_dir)?;
        markdown_parser::write_markdown_data_to_sqlite_with_conn(&data, &tx)?;
        file_mtimes.insert(path, current_mtime);
        updated_count += 1;
    }

    tx.commit()?;

    // Remove notes that no longer exist on the filesystem
    let removed = markdown_parser::remove_orphaned_notes(input_dir, &conn)?;
    if removed > 0 {
        println!("Removed {} orphaned notes from database", removed);
    }

    Ok(updated_count)
}
