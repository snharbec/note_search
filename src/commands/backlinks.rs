use std::path::Path;
use std::process;

pub fn handle_backlinks(database: &str, filename: &str) {
    let db_path = Path::new(database);

    if !db_path.exists() {
        eprintln!("Error: Database '{}' does not exist", database);
        process::exit(1);
    }

    match get_backlinks(db_path, filename) {
        Ok(backlinks) => {
            for doc in backlinks {
                println!("{}", doc);
            }
        }
        Err(e) => {
            eprintln!("Error getting backlinks: {}", e);
            process::exit(1);
        }
    }
}

pub fn get_backlinks(
    db_path: &Path,
    target_filename: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    use rusqlite::Connection;
    use std::collections::HashSet;

    let conn = Connection::open(db_path)?;
    let mut backlinks = HashSet::new();

    let target_base = Path::new(target_filename)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| target_filename.to_string());

    // Normalized forms for matching: lowercase, underscores→spaces
    let normalized_target_file = target_filename.to_lowercase().replace('_', " ");
    let normalized_target_base = target_base.to_lowercase().replace('_', " ");

    // Fuzzy match threshold (0.0-1.0, higher = stricter)
    // Using 0.98 to require near-exact matches and avoid false positives
    // with names that share common prefixes like "Jürgen_"
    const FUZZY_THRESHOLD: f64 = 0.98;

    fn is_match(link: &str, normalized_target_file: &str, normalized_target_base: &str) -> bool {
        let normalized_link = link.to_lowercase().replace('_', " ");

        // Exact match
        if normalized_link == *normalized_target_file || normalized_link == *normalized_target_base
        {
            return true;
        }

        // Also try matching against the raw link (without lowering, for Unicode chars)
        let space_link = link.replace('_', " ");
        let space_target_file = normalized_target_file.replace('_', " ");
        let space_target_base = normalized_target_base.replace('_', " ");
        if space_link == space_target_file || space_link == space_target_base {
            return true;
        }

        // Fuzzy match using Jaro-Winkler similarity
        let score = strsim::jaro_winkler(&normalized_link, normalized_target_file);
        if score >= FUZZY_THRESHOLD {
            return true;
        }
        let score_base = strsim::jaro_winkler(&normalized_link, normalized_target_base);
        score_base >= FUZZY_THRESHOLD
    }

    // Search in markdown_data.links (document-level links)
    let mut stmt =
        conn.prepare("SELECT filename, links FROM markdown_data WHERE links IS NOT NULL")?;
    let rows = stmt.query_map([], |row| {
        let filename: String = row.get(0)?;
        let links_json: String = row.get(1)?;
        Ok((filename, links_json))
    })?;

    for row in rows {
        let (doc_filename, links_json) = row?;
        if let Ok(links_array) = serde_json::from_str::<Vec<String>>(&links_json) {
            for link in links_array {
                if is_match(&link, &normalized_target_file, &normalized_target_base) {
                    backlinks.insert(doc_filename.clone());
                    break;
                }
            }
        }
    }

    // Also search in todo_entries.links (todo-level links)
    let mut stmt =
        conn.prepare("SELECT DISTINCT filename, links FROM todo_entries WHERE links IS NOT NULL")?;
    let rows = stmt.query_map([], |row| {
        let filename: String = row.get(0)?;
        let links_json: String = row.get(1)?;
        Ok((filename, links_json))
    })?;

    for row in rows {
        let (doc_filename, links_json) = row?;
        if let Ok(links_array) = serde_json::from_str::<Vec<String>>(&links_json) {
            for link in links_array {
                if is_match(&link, &normalized_target_file, &normalized_target_base) {
                    backlinks.insert(doc_filename);
                    break;
                }
            }
        }
    }

    // Fetch updated timestamps and sort by modified time (newest first)
    let mut result: Vec<(i64, String)> = Vec::new();
    if !backlinks.is_empty() {
        let placeholders: Vec<String> = backlinks.iter().map(|_| "?".to_string()).collect();
        let query = format!(
            "SELECT filename, updated FROM markdown_data WHERE filename IN ({})",
            placeholders.join(",")
        );
        let mut stmt = conn.prepare(&query)?;
        let params: Vec<&dyn rusqlite::ToSql> = backlinks
            .iter()
            .map(|s| s as &dyn rusqlite::ToSql)
            .collect();

        let rows = stmt.query_map(params.as_slice(), |row| {
            let filename: String = row.get(0)?;
            let updated: i64 = row.get(1)?;
            Ok((updated, filename))
        })?;
        for row in rows {
            result.push(row?);
        }
    }

    result.sort_by(|a, b| b.0.cmp(&a.0));

    Ok(result.into_iter().map(|(_, f)| f).collect())
}
