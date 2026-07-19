use std::path::Path;
use std::process;

pub fn handle_backlinks(database: &str, filename: &str, markdown: bool) {
    let db_path = Path::new(database);

    if !db_path.exists() {
        eprintln!("Error: Database '{}' does not exist", database);
        process::exit(1);
    }

    match get_backlinks(db_path, filename) {
        Ok(backlinks) => {
            for doc in backlinks {
                if markdown {
                    println!("[{}]({})", doc, doc);
                } else {
                    println!("{}", doc);
                }
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
    // Ensures note_links exists (and is backfilled) on a database that
    // predates the tag/link junction tables.
    crate::markdown_parser::init_database_schema(&conn)?;
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

    // note_links aggregates every link found in a note's body (which includes
    // todo lines, since todos are just regular body text), so a single scan
    // over it covers what previously required separately scanning
    // markdown_data.links and todo_entries.links.
    let mut stmt = conn.prepare("SELECT filename, link FROM note_links")?;
    let rows = stmt.query_map([], |row| {
        let filename: String = row.get(0)?;
        let link: String = row.get(1)?;
        Ok((filename, link))
    })?;

    for row in rows {
        let (doc_filename, link) = row?;
        if is_match(&link, &normalized_target_file, &normalized_target_base) {
            backlinks.insert(doc_filename);
        }
    }

    // The target document should never appear in its own backlink list
    backlinks.remove(target_filename);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown_parser::{write_markdown_data_to_sqlite, Header, MarkdownData};
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn test_get_backlinks() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");

        // doc1 links to target
        let doc1 = MarkdownData {
            filename: "doc1.md".to_string(),
            created: 1234567890,
            updated: 1234567890,
            title: "Doc 1".to_string(),
            header: Header {
                fields: HashMap::new(),
            },
            todo: vec![],
            link: vec!["target".to_string()],
            body: "".to_string(),
            elements: vec![],
        };

        // doc2 does not link to target
        let doc2 = MarkdownData {
            filename: "doc2.md".to_string(),
            created: 1234567890,
            updated: 1234567890,
            title: "Doc 2".to_string(),
            header: Header {
                fields: HashMap::new(),
            },
            todo: vec![],
            link: vec!["other".to_string()],
            body: "".to_string(),
            elements: vec![],
        };

        // target note
        let target = MarkdownData {
            filename: "target.md".to_string(),
            created: 1234567890,
            updated: 1234567890,
            title: "Target".to_string(),
            header: Header {
                fields: HashMap::new(),
            },
            todo: vec![],
            link: vec![],
            body: "".to_string(),
            elements: vec![],
        };

        write_markdown_data_to_sqlite(&doc1, &db_path)?;
        write_markdown_data_to_sqlite(&doc2, &db_path)?;
        write_markdown_data_to_sqlite(&target, &db_path)?;

        let backlinks = get_backlinks(&db_path, "target.md")?;
        assert_eq!(backlinks.len(), 1);
        assert_eq!(backlinks[0], "doc1.md");

        Ok(())
    }
}
