use std::fs;
use std::path::Path;
use std::process;
use std::time::{Duration, Instant, SystemTime};

use crate::commands::browser_history;
use crate::markdown_parser;

pub fn handle_import(
    default_db: &str,
    input: &str,
    output: Option<&str>,
    browser_history: bool,
    progress: bool,
) {
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

    match do_import_with_tracking(input_path, Path::new(db_path), &mut file_mtimes, progress) {
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

    if browser_history {
        println!("\nCreating browser history note...");
        let note_dir = std::env::var("NOTE_SEARCH_DIR").unwrap_or_else(|_| input.to_string());
        if let Err(e) = browser_history::do_browser_history(None, 1, Some(&note_dir), true) {
            eprintln!("Warning: Browser history failed: {}", e);
        }
    }
}

pub fn handle_watch_import(
    default_db: &str,
    input: &str,
    output: Option<&str>,
    interval: u64,
    browser_history: bool,
    browser_history_interval: u64,
) {
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
    let mut last_browser_history = Instant::now();

    println!(
        "Starting watch mode for directory '{}' (checking every {} seconds)",
        input, interval
    );
    if browser_history {
        println!(
            "Browser history will be created every {} seconds",
            browser_history_interval
        );
    }
    println!("Press Ctrl+C to stop watching...\n");

    // Do initial browser history if requested
    if browser_history {
        println!("\nCreating initial browser history note...");
        let note_dir = std::env::var("NOTE_SEARCH_DIR").unwrap_or_else(|_| input.to_string());
        if let Err(e) = browser_history::do_browser_history(None, 1, Some(&note_dir), true) {
            eprintln!("Warning: Browser history failed: {}", e);
        }
        last_browser_history = Instant::now();
    }

    // Do initial import - imports all files and replaces existing content
    match do_import_with_tracking(input_path, Path::new(db_path), &mut file_mtimes, false) {
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
    let browser_history_duration = Duration::from_secs(browser_history_interval);
    loop {
        std::thread::sleep(interval_duration);

        match do_import_with_tracking(input_path, Path::new(db_path), &mut file_mtimes, false) {
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

        if browser_history && last_browser_history.elapsed() >= browser_history_duration {
            println!("\nCreating browser history note...");
            let note_dir = std::env::var("NOTE_SEARCH_DIR").unwrap_or_else(|_| input.to_string());
            if let Err(e) = browser_history::do_browser_history(None, 1, Some(&note_dir), true) {
                eprintln!("Warning: Browser history failed: {}", e);
            }
            last_browser_history = Instant::now();
        }
    }
}

/// Imports markdown files and tracks modification times
/// Returns count of files that were newly imported or modified
pub fn do_import_with_tracking(
    input_dir: &Path,
    db_path: &Path,
    file_mtimes: &mut std::collections::HashMap<std::path::PathBuf, SystemTime>,
    progress: bool,
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut conn = crate::commands::open_db_with_schema(db_path)?;

    // Before the "nothing changed" early return, so deleting a note alone
    // is noticed. Forget deleted files too: one restored with its old mtime
    // must be imported again.
    file_mtimes.retain(|path, _| path.exists());
    let removed = markdown_parser::remove_orphaned_notes(input_dir, &conn)?;
    if removed > 0 {
        println!("Removed {} deleted notes from database", removed);
    }

    let mut files_to_import: Vec<(std::path::PathBuf, SystemTime)> = Vec::new();

    for entry in walkdir::WalkDir::new(input_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|ext| ext == "md"))
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
    let mapping_config = crate::commands::mapping::MappingConfig::load();
    let jira_min_words = crate::commands::mapping::jira_segment_min_words();

    let bar = if progress {
        let bar = indicatif::ProgressBar::new(files_to_import.len() as u64);
        bar.set_style(
            indicatif::ProgressStyle::with_template("{percent}% [{bar:40}] {pos}/{len}")
                .unwrap()
                .progress_chars("=> "),
        );
        Some(bar)
    } else {
        None
    };

    for (path, current_mtime) in files_to_import {
        let data = markdown_parser::process_markdown_file_with_config(
            &path,
            input_dir,
            &mapping_config,
            jira_min_words,
        )?;
        markdown_parser::write_markdown_data_to_sqlite_with_conn(&data, &tx)?;
        file_mtimes.insert(path, current_mtime);
        updated_count += 1;
        if let Some(bar) = &bar {
            bar.inc(1);
        }
    }

    if let Some(bar) = &bar {
        bar.finish_and_clear();
    }

    tx.commit()?;

    Ok(updated_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note_count(db: &Path) -> i64 {
        rusqlite::Connection::open(db)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM markdown_data", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn test_deleted_note_is_removed_without_other_changes() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = dir.path().join("db.sqlite");
        let notes = dir.path().join("notes");
        fs::create_dir(&notes).unwrap();
        fs::write(notes.join("a.md"), "# A\n").unwrap();
        fs::write(notes.join("b.md"), "# B\n").unwrap();
        let mut mtimes = std::collections::HashMap::new();

        assert_eq!(do_import_with_tracking(&notes, &db, &mut mtimes, false).unwrap(), 2);
        let backup = fs::read(notes.join("b.md")).unwrap();
        let mtime = fs::metadata(notes.join("b.md")).unwrap().modified().unwrap();
        fs::remove_file(notes.join("b.md")).unwrap();

        // Nothing else changed: the deleted note still goes.
        assert_eq!(do_import_with_tracking(&notes, &db, &mut mtimes, false).unwrap(), 0);
        assert_eq!(note_count(&db), 1);

        // Restored with its old mtime (e.g. from the trash): imported again.
        fs::write(notes.join("b.md"), backup).unwrap();
        fs::File::options()
            .write(true)
            .open(notes.join("b.md"))
            .unwrap()
            .set_modified(mtime)
            .unwrap();
        assert_eq!(do_import_with_tracking(&notes, &db, &mut mtimes, false).unwrap(), 1);
        assert_eq!(note_count(&db), 2);
    }
}
