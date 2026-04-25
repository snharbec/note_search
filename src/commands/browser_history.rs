use chrono::{DateTime, Local, NaiveDate, Utc};
use rusqlite::Connection;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

/// Represents a browser history entry
#[derive(Debug, Clone)]
struct HistoryEntry {
    url: String,
    title: String,
    visit_time: i64, // Browser-specific time
    browser: String,
}

/// Get the path to the timestamp storage directory
fn get_timestamp_dir() -> Result<String, Box<dyn std::error::Error>> {
    let home_dir = env::var("HOME").map_err(|_| "HOME environment variable not set")?;
    let timestamp_dir = Path::new(&home_dir).join(".local/share/note_search");
    Ok(timestamp_dir.to_string_lossy().to_string())
}

/// Ensure the timestamp directory exists
fn ensure_timestamp_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let dir_str = get_timestamp_dir()?;
    let dir = Path::new(&dir_str);
    if !dir.exists() {
        fs::create_dir_all(dir)?;
    }
    Ok(PathBuf::from(dir))
}

/// Read the last timestamp for a browser
fn read_last_timestamp(browser: &str) -> Result<Option<i64>, Box<dyn std::error::Error>> {
    let dir = ensure_timestamp_dir()?;
    let filename = format!("last_timestamp.{}", browser.to_lowercase());
    let filepath = dir.join(&filename);

    if !filepath.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&filepath)?;
    let timestamp: i64 = contents.trim().parse()?;
    Ok(Some(timestamp))
}

/// Write the last timestamp for a browser
fn write_last_timestamp(browser: &str, timestamp: i64) -> Result<(), Box<dyn std::error::Error>> {
    let dir = ensure_timestamp_dir()?;
    let filename = format!("last_timestamp.{}", browser.to_lowercase());
    let filepath = dir.join(&filename);

    fs::write(&filepath, timestamp.to_string())?;
    Ok(())
}

/// Convert browser-specific time to Unix timestamp
fn browser_time_to_unix(time: i64, browser: &str) -> i64 {
    match browser {
        "Safari" => time + 978307200, // Core Data epoch (2001-01-01) to Unix
        "Vivaldi" => time / 1_000_000 - 11644473600, // Chrome epoch (1601-01-01) to Unix
        "Firefox" => time / 1_000_000, // Already microseconds since Unix epoch
        _ => time,
    }
}

/// Read Safari history for a given date range
fn read_safari_history(
    start_time: i64,
    end_time: i64,
) -> Result<Vec<HistoryEntry>, Box<dyn std::error::Error>> {
    let home_dir = env::var("HOME").map_err(|_| "HOME environment variable not set")?;
    let safari_db_path = Path::new(&home_dir).join("Library/Safari/History.db");

    if !safari_db_path.exists() {
        return Ok(vec![]);
    }

    let conn = Connection::open(safari_db_path)?;

    // Safari uses Core Data timestamps (seconds since 2001-01-01 00:00:00 UTC)
    // Unix timestamps are seconds since 1970-01-01, so we SUBTRACT the difference
    let safari_start = start_time - 978307200;
    let safari_end = end_time - 978307200;

    let query = r#"
        SELECT
            hi.url,
            hv.title,
            hv.visit_time
        FROM history_visits hv
        JOIN history_items hi ON hv.history_item = hi.id
        WHERE hv.visit_time BETWEEN CAST(? AS REAL) AND CAST(? AS REAL)
        ORDER BY hv.visit_time DESC
    "#;

    let mut stmt = conn.prepare(query)?;
    let rows = stmt.query_map([safari_start, safari_end], |row| {
        Ok(HistoryEntry {
            url: row.get::<_, String>(0)?,
            title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            visit_time: row.get::<_, f64>(2)? as i64,
            browser: "Safari".to_string(),
        })
    })?;

    let mut entries = Vec::new();
    for row in rows {
        if let Ok(entry) = row {
            entries.push(entry);
        }
    }

    Ok(entries)
}

/// Read Firefox history for a given date range
fn read_firefox_history(
    start_time: i64,
    end_time: i64,
) -> Result<Vec<HistoryEntry>, Box<dyn std::error::Error>> {
    let home_dir = env::var("HOME").map_err(|_| "HOME environment variable not set")?;
    let firefox_dir = Path::new(&home_dir).join("Library/Application Support/Firefox/Profiles");

    if !firefox_dir.exists() {
        return Ok(vec![]);
    }

    // Find the default profile (usually ends with .default or .default-release)
    let mut places_db_path = None;
    for entry in fs::read_dir(&firefox_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name.contains("default") {
                let db_path = path.join("places.sqlite");
                if db_path.exists() {
                    places_db_path = Some(db_path);
                    break;
                }
            }
        }
    }

    let places_db_path = match places_db_path {
        Some(path) => path,
        None => return Ok(vec![]),
    };

    // Create a temporary copy of the database to avoid "database is locked" error
    let temp_dir = env::temp_dir().join(format!("note_search_firefox_{}", std::process::id()));
    fs::create_dir_all(&temp_dir)?;
    let temp_db_path = temp_dir.join("places.sqlite");

    // Copy the database file
    fs::copy(&places_db_path, &temp_db_path)?;

    // Open the copied database
    let conn = match Connection::open(&temp_db_path) {
        Ok(conn) => conn,
        Err(e) => {
            let _ = fs::remove_dir_all(&temp_dir);
            return Err(e.into());
        }
    };

    // Firefox uses microseconds since Unix epoch
    let firefox_start = start_time * 1_000_000;
    let firefox_end = end_time * 1_000_000;

    let query = r#"
        SELECT
            p.url,
            p.title,
            h.visit_date
        FROM moz_historyvisits h
        JOIN moz_places p ON h.place_id = p.id
        WHERE h.visit_date BETWEEN ? AND ?
        ORDER BY h.visit_date DESC
    "#;

    let mut entries = Vec::new();

    match conn.prepare(query) {
        Ok(mut stmt) => {
            match stmt.query_map([firefox_start, firefox_end], |row| {
                Ok(HistoryEntry {
                    url: row.get::<_, String>(0)?,
                    title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    visit_time: row.get::<_, i64>(2)?,
                    browser: "Firefox".to_string(),
                })
            }) {
                Ok(rows) => {
                    for row in rows {
                        if let Ok(entry) = row {
                            entries.push(entry);
                        }
                    }
                }
                Err(e) => eprintln!("Warning: Firefox query error: {}", e),
            }
        }
        Err(e) => eprintln!("Warning: Firefox prepare error: {}", e),
    }

    // Clean up the temporary directory
    let _ = fs::remove_dir_all(&temp_dir);

    Ok(entries)
}

/// Read Vivaldi history for a given date range
fn read_vivaldi_history(
    start_time: i64,
    end_time: i64,
) -> Result<Vec<HistoryEntry>, Box<dyn std::error::Error>> {
    let home_dir = env::var("HOME").map_err(|_| "HOME environment variable not set")?;
    let vivaldi_db_path =
        Path::new(&home_dir).join("Library/Application Support/Vivaldi/Default/History");

    if !vivaldi_db_path.exists() {
        return Ok(vec![]);
    }

    // Create a temporary copy of the database to avoid "database is locked" error
    let temp_dir = env::temp_dir().join(format!("note_search_vivaldi_{}", std::process::id()));
    fs::create_dir_all(&temp_dir)?;
    let temp_db_path = temp_dir.join("History");

    // Copy the database file
    fs::copy(&vivaldi_db_path, &temp_db_path)?;

    // Open the copied database
    let conn = match Connection::open(&temp_db_path) {
        Ok(conn) => conn,
        Err(e) => {
            let _ = fs::remove_dir_all(&temp_dir);
            return Err(e.into());
        }
    };

    // Chrome-based browsers use microseconds since 1601-01-01 UTC
    let chrome_start = (start_time + 11644473600) * 1_000_000;
    let chrome_end = (end_time + 11644473600) * 1_000_000;

    let query = r#"
        SELECT
            u.url,
            u.title,
            v.visit_time
        FROM urls u
        JOIN visits v ON u.id = v.url
        WHERE v.visit_time BETWEEN ? AND ?
        ORDER BY v.visit_time DESC
    "#;

    let mut entries = Vec::new();

    match conn.prepare(query) {
        Ok(mut stmt) => {
            match stmt.query_map([chrome_start, chrome_end], |row| {
                Ok(HistoryEntry {
                    url: row.get::<_, String>(0)?,
                    title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    visit_time: row.get::<_, i64>(2)?,
                    browser: "Vivaldi".to_string(),
                })
            }) {
                Ok(rows) => {
                    for row in rows {
                        if let Ok(entry) = row {
                            entries.push(entry);
                        }
                    }
                }
                Err(e) => eprintln!("Warning: Vivaldi query error: {}", e),
            }
        }
        Err(e) => eprintln!("Warning: Vivaldi prepare error: {}", e),
    }

    // Clean up the temporary directory
    let _ = fs::remove_dir_all(&temp_dir);

    Ok(entries)
}

/// Deduplicate entries by URL, keeping the most recent visit
fn deduplicate_entries(entries: Vec<HistoryEntry>) -> Vec<HistoryEntry> {
    let mut seen = HashMap::new();

    for entry in entries {
        let normalized_time = browser_time_to_unix(entry.visit_time, &entry.browser);

        seen.entry(entry.url.clone())
            .and_modify(|e: &mut HistoryEntry| {
                let existing_normalized = browser_time_to_unix(e.visit_time, &e.browser);
                if normalized_time > existing_normalized {
                    *e = entry.clone();
                }
            })
            .or_insert(entry);
    }

    let mut result: Vec<HistoryEntry> = seen.into_values().collect();

    // Sort by normalized time (most recent first)
    result.sort_by(|a, b| {
        let a_time = browser_time_to_unix(a.visit_time, &a.browser);
        let b_time = browser_time_to_unix(b.visit_time, &b.browser);
        b_time.cmp(&a_time)
    });

    result
}

/// Get the latest timestamp from entries for a specific browser
fn get_latest_timestamp(entries: &[HistoryEntry], browser: &str) -> Option<i64> {
    entries
        .iter()
        .filter(|e| e.browser == browser)
        .map(|e| browser_time_to_unix(e.visit_time, &e.browser))
        .max()
}

/// Generate markdown content for the history entries
fn generate_markdown(entries: &[HistoryEntry], creation_date: &str) -> String {
    let mut content = String::new();

    content.push_str(&format!("---\n"));
    content.push_str(&format!("title: \"Browser History\"\n"));
    content.push_str(&format!("date: {}\n", creation_date));
    content.push_str(&format!("type: note\n"));
    content.push_str(&format!("tags: [browser-history, daily]\n"));
    content.push_str(&format!("---\n\n"));
    content.push_str(&format!("# Browser History\n\n"));
    content.push_str(&format!("Created on [[{}]]\n\n", creation_date));

    if entries.is_empty() {
        content.push_str("No browsing history found.\n");
        return content;
    }

    content.push_str(&format!("## Summary\n\n"));
    content.push_str(&format!("Total unique URLs visited: {}\n\n", entries.len()));

    // Group by browser
    let mut safari_count = 0;
    let mut vivaldi_count = 0;
    let mut firefox_count = 0;
    for entry in entries {
        match entry.browser.as_str() {
            "Safari" => safari_count += 1,
            "Vivaldi" => vivaldi_count += 1,
            "Firefox" => firefox_count += 1,
            _ => {}
        }
    }

    if safari_count > 0 {
        content.push_str(&format!("- Safari: {} URLs\n", safari_count));
    }
    if vivaldi_count > 0 {
        content.push_str(&format!("- Vivaldi: {} URLs\n", vivaldi_count));
    }
    if firefox_count > 0 {
        content.push_str(&format!("- Firefox: {} URLs\n", firefox_count));
    }
    content.push('\n');

    content.push_str("## History Entries\n\n");

    for entry in entries {
        let unix_time = browser_time_to_unix(entry.visit_time, &entry.browser);
        let dt = chrono::DateTime::from_timestamp(unix_time, 0)
            .unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH);
        let time_str = dt.format("%H:%M").to_string();
        let date_str = dt.format("%Y-%m-%d").to_string();

        let display_title = if entry.title.is_empty() {
            entry.url.clone()
        } else {
            entry.title.clone()
        };

        content.push_str(&format!(
            "- [{}]({}) [{} - {}] [[{}]]\n",
            display_title, entry.url, entry.browser, time_str, date_str
        ));
    }

    content
}

/// Write the markdown content to a file with the new path structure
fn write_history_note(
    content: &str,
    creation_time: &DateTime<Local>,
    note_dir: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    // Create the directory structure: web/YYYY/MONTH/
    let year = creation_time.format("%Y").to_string();
    let month = creation_time.format("%b").to_string(); // Short month name, first letter capitalized
    let file_date = creation_time.format("%Y-%m-%d-%H:%M").to_string();

    let web_dir = note_dir.join("web").join(&year).join(&month);
    fs::create_dir_all(&web_dir)?;

    let filename = format!("browser-{}.md", file_date);
    let filepath = web_dir.join(&filename);

    fs::write(&filepath, content)?;

    Ok(filepath.to_string_lossy().to_string())
}

/// Handle the browser-history command
pub fn handle_browser_history(
    date: Option<&String>,
    days: i64,
    note_dir: Option<&String>,
    use_timestamp: bool,
) {
    let now_utc = Utc::now();
    let now_local = Local::now();
    let creation_date = now_local.format("%Y-%m-%d").to_string();

    // Determine time range for each browser individually when using timestamp mode
    let browser_time_ranges = if use_timestamp && days <= 1 {
        let mut ranges = HashMap::new();
        let end_time = now_utc.timestamp();

        // Check each browser's last timestamp individually
        for browser in ["Safari", "Vivaldi", "Firefox"] {
            let start_time = match read_last_timestamp(browser) {
                Ok(Some(ts)) => {
                    println!("Found last {} timestamp: {}", browser, ts);
                    // Add 1 second to exclude already-seen entries
                    ts + 1
                }
                Ok(None) => {
                    println!("No previous {} timestamp found, using 1 day ago", browser);
                    now_utc.timestamp() - 86400
                }
                Err(e) => {
                    eprintln!("Warning: Could not read {} timestamp: {}", browser, e);
                    now_utc.timestamp() - 86400
                }
            };
            ranges.insert(browser, (start_time, end_time));
        }
        ranges
    } else {
        // Use days parameter - same range for all browsers
        let target_date = match date {
            Some(d) => NaiveDate::parse_from_str(d, "%Y-%m-%d")
                .unwrap_or_else(|_| now_utc.naive_utc().date()),
            None => now_utc.naive_utc().date(),
        };

        let end_of_day = target_date
            .and_hms_opt(23, 59, 59)
            .unwrap_or_else(|| target_date.and_hms_opt(0, 0, 0).unwrap());
        let start_of_period = target_date
            .and_hms_opt(0, 0, 0)
            .unwrap_or_else(|| target_date.and_hms_opt(0, 0, 0).unwrap())
            - chrono::Duration::days(days - 1);

        let start_time = start_of_period.and_utc().timestamp();
        let end_time = end_of_day.and_utc().timestamp();

        println!(
            "Reading browser history from {} to {}...",
            target_date - chrono::Duration::days(days - 1),
            target_date
        );

        let mut ranges = HashMap::new();
        for browser in ["Safari", "Vivaldi", "Firefox"] {
            ranges.insert(browser, (start_time, end_time));
        }
        ranges
    };

    // Read history from all browsers using their individual time ranges
    let mut all_entries = Vec::new();
    let mut browser_entries_found: HashMap<&str, Vec<HistoryEntry>> = HashMap::new();

    // Safari
    if let Some((start, end)) = browser_time_ranges.get("Safari") {
        match read_safari_history(*start, *end) {
            Ok(entries) => {
                println!("Found {} Safari history entries", entries.len());
                if !entries.is_empty() {
                    browser_entries_found.insert("Safari", entries.clone());
                }
                all_entries.extend(entries);
            }
            Err(e) => {
                eprintln!("Warning: Could not read Safari history: {}", e);
            }
        }
    }

    // Vivaldi
    if let Some((start, end)) = browser_time_ranges.get("Vivaldi") {
        match read_vivaldi_history(*start, *end) {
            Ok(entries) => {
                println!("Found {} Vivaldi history entries", entries.len());
                if !entries.is_empty() {
                    browser_entries_found.insert("Vivaldi", entries.clone());
                }
                all_entries.extend(entries);
            }
            Err(e) => {
                eprintln!("Warning: Could not read Vivaldi history: {}", e);
            }
        }
    }

    // Firefox
    if let Some((start, end)) = browser_time_ranges.get("Firefox") {
        match read_firefox_history(*start, *end) {
            Ok(entries) => {
                println!("Found {} Firefox history entries", entries.len());
                if !entries.is_empty() {
                    browser_entries_found.insert("Firefox", entries.clone());
                }
                all_entries.extend(entries);
            }
            Err(e) => {
                eprintln!("Warning: Could not read Firefox history: {}", e);
            }
        }
    }

    // Deduplicate entries
    let unique_entries = deduplicate_entries(all_entries);
    println!(
        "Total unique URLs after deduplication: {}",
        unique_entries.len()
    );

    // If no entries found, don't write a file
    if unique_entries.is_empty() {
        println!("No browser history entries found. No file will be created.");
        return;
    }

    // Only update timestamp files for browsers that actually had entries
    for (browser, entries) in &browser_entries_found {
        if let Some(latest_ts) = get_latest_timestamp(entries, browser) {
            if let Err(e) = write_last_timestamp(browser, latest_ts) {
                eprintln!("Warning: Could not write {} timestamp: {}", browser, e);
            } else {
                println!("Updated {} timestamp to {}", browser, latest_ts);
            }
        }
    }

    // Generate markdown
    let markdown = generate_markdown(&unique_entries, &creation_date);

    // Determine output directory
    let output_dir = match note_dir {
        Some(dir) => Path::new(dir).to_path_buf(),
        None => match env::var("NOTE_SEARCH_DIR") {
            Ok(dir) => Path::new(&dir).to_path_buf(),
            Err(_) => {
                eprintln!("Error: No output directory specified.");
                eprintln!("Use --note-dir <DIR> or set NOTE_SEARCH_DIR environment variable.");
                process::exit(1);
            }
        },
    };

    // Write to file with new path structure using LOCAL time
    match write_history_note(&markdown, &now_local, &output_dir) {
        Ok(filepath) => {
            println!("Successfully created browser history note: {}", filepath);
        }
        Err(e) => {
            eprintln!("Error writing history note: {}", e);
            process::exit(1);
        }
    }
}
