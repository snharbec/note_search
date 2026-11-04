use crate::commands::backlinks::get_backlinks;
use crate::database_service::{format_timestamp, DatabaseService};
use crate::query_builder::normalize_name;
use rusqlite::Connection;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::process;

pub fn handle_info(database: &str, filename: &str) {
    let db_service = DatabaseService::new(database);
    let db_path = Path::new(database);

    crate::commands::require_db_exists(db_path, database);

    // First try exact match
    match get_note_info(db_path, filename) {
        Ok(info) => print_note_info(&db_service, db_path, info),
        Err(_) => {
            // If not found, try searching for any filename that ends with /filename or equals filename
            match search_filename_matches(db_path, filename) {
                Ok(matches) => {
                    if matches.is_empty() {
                        eprintln!("Error: Document '{}' not found", filename);
                        process::exit(1);
                    } else if matches.len() == 1 {
                        match get_note_info(db_path, &matches[0]) {
                            Ok(info) => print_note_info(&db_service, db_path, info),
                            Err(e) => eprintln!("Error getting note info: {}", e),
                        }
                    } else {
                        println!("Multiple documents found for '{}':", filename);
                        for m in matches {
                            println!("  - {}", m);
                        }
                        println!("\nPlease specify the full path.");
                    }
                }
                Err(e) => {
                    eprintln!("Error searching for document: {}", e);
                    process::exit(1);
                }
            }
        }
    }
}

fn print_note_info(db: &DatabaseService, db_path: &Path, info: NoteInfo) {
    println!("Filename: {}", info.filename);
    println!("Title: {}", info.title.unwrap_or_else(|| "N/A".to_string()));
    println!("Created: {}", format_timestamp(info.created));
    println!("Updated: {}", format_timestamp(info.updated));

    println!("\nAttributes:");
    let fields = info
        .header_fields
        .and_then(|f| serde_json::from_str::<BTreeMap<String, Value>>(&f).ok())
        .unwrap_or_default();
    let fields: Vec<(String, String)> = fields
        .into_iter()
        .filter_map(|(k, v)| attribute_text(&v).filter(|v| !v.is_empty()).map(|v| (k, v)))
        .collect();
    print_list(fields.iter().map(|(k, v)| format!("{}: {}", k, v)));

    println!("\nTags:");
    let tags: Vec<String> = info
        .tags
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    print_list(tags.iter().map(|t| format!("#{}", t)));

    // "name -> file" when the link resolves to a note, so callers can open it.
    println!("\nLinks:");
    let links: Vec<String> = info
        .links
        .and_then(|l| serde_json::from_str(&l).ok())
        .unwrap_or_default();
    let conn = db.connect().ok();
    print_list(links.iter().map(|link| {
        match conn.as_ref().and_then(|c| resolve_link(c, link)) {
            Some(file) => format!("- {} -> {}", link, file),
            None => format!("- {}", link),
        }
    }));

    println!("\nOpen todos:");
    let todos = conn
        .as_ref()
        .map(|c| open_todos(c, &info.filename))
        .unwrap_or_default();
    print_list(todos.into_iter());

    println!("\nBacklinks:");
    match get_backlinks(db_path, &info.filename) {
        Ok(backlinks) => print_list(backlinks.iter().map(|b| format!("- {}", b))),
        Err(e) => eprintln!("  Error: {}", e),
    }
}

/// Print each line indented, or "None" when there are none.
fn print_list(lines: impl Iterator<Item = String>) {
    let mut any = false;
    for line in lines {
        any = true;
        println!("  {}", line);
    }
    if !any {
        println!("  None");
    }
}

/// A frontmatter value as plain text: strings unquoted, lists joined; None for null.
fn attribute_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Array(items) => Some(
            items
                .iter()
                .filter_map(attribute_text)
                .collect::<Vec<_>>()
                .join(", "),
        ),
        other => Some(other.to_string()),
    }
}

/// The note a link name points to: file name or alias, ignoring case and
/// `_` vs space; the shortest path wins.
fn resolve_link(conn: &Connection, link: &str) -> Option<String> {
    let name = normalize_name(link.trim_end_matches(".md"));
    conn.query_row(
        "SELECT filename FROM markdown_data \
         WHERE LOWER(REPLACE(filename, '_', ' ')) = ?1 || '.md' \
            OR substr(LOWER(REPLACE(filename, '_', ' ')), -(length(?1) + 4)) = '/' || ?1 || '.md' \
            OR filename IN (SELECT filename FROM note_aliases WHERE LOWER(REPLACE(alias, '_', ' ')) = ?1) \
         ORDER BY length(filename) LIMIT 1",
        [name],
        |row| row.get(0),
    )
    .ok()
}

/// "LINE: text (due DATE)" for each open todo of `filename`.
fn open_todos(conn: &Connection, filename: &str) -> Vec<String> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT line_number, text, due FROM todo_entries \
         WHERE filename = ?1 AND NOT closed ORDER BY line_number",
    ) else {
        return Vec::new();
    };
    stmt.query_map([filename], |row| {
        let line: i64 = row.get(0)?;
        let text: String = row.get(1)?;
        let due: Option<String> = row.get(2)?;
        Ok(match due.filter(|d| !d.is_empty()) {
            Some(d) => format!("{}: {} (due {})", line, text, d),
            None => format!("{}: {}", line, text),
        })
    })
    .map(|rows| rows.filter_map(Result::ok).collect())
    .unwrap_or_default()
}

fn search_filename_matches(
    db_path: &Path,
    filename: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let conn = Connection::open(db_path)?;
    let mut stmt =
        conn.prepare("SELECT filename FROM markdown_data WHERE filename = ? OR filename LIKE ?")?;
    let pattern = format!("%/{}", filename);
    let rows = stmt.query_map([filename, &pattern], |row| row.get(0))?;

    let mut matches = Vec::new();
    for row in rows {
        matches.push(row?);
    }
    Ok(matches)
}

pub struct NoteInfo {
    pub filename: String,
    pub title: Option<String>,
    pub created: i64,
    pub updated: i64,
    pub header_fields: Option<String>,
    pub links: Option<String>,
    pub tags: Option<String>,
}

fn get_note_info(db_path: &Path, filename: &str) -> Result<NoteInfo, Box<dyn std::error::Error>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT filename, title, created, updated, header_fields, links, tags FROM markdown_data WHERE filename = ?",
    )?;

    let mut rows = stmt.query([filename])?;
    if let Some(row) = rows.next()? {
        Ok(NoteInfo {
            filename: row.get(0)?,
            title: row.get(1)?,
            created: row.get(2)?,
            updated: row.get(3)?,
            header_fields: row.get(4)?,
            links: row.get(5)?,
            tags: row.get(6)?,
        })
    } else {
        Err(format!("Document '{}' not found", filename).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown_parser::{write_markdown_data_to_sqlite, Header, MarkdownData};
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn test_search_filename_matches() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");

        let data = MarkdownData {
            filename: "projects/test.md".to_string(),
            created: 1234567890,
            updated: 1234567890,
            title: "Test".to_string(),
            header: Header {
                fields: HashMap::new(),
            },
            todo: vec![],
            link: vec![],
            body: "".to_string(),
            segments: vec![],
            tags: vec![],
        };

        write_markdown_data_to_sqlite(&data, &db_path)?;

        let matches = search_filename_matches(&db_path, "test.md")?;
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0], "projects/test.md");

        let matches_exact = search_filename_matches(&db_path, "projects/test.md")?;
        assert_eq!(matches_exact.len(), 1);
        assert_eq!(matches_exact[0], "projects/test.md");

        let matches_none = search_filename_matches(&db_path, "nonexistent.md")?;
        assert!(matches_none.is_empty());

        Ok(())
    }

    #[test]
    fn test_resolve_link_and_attribute_text() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");
        let mut fields = HashMap::new();
        fields.insert("aliases".to_string(), serde_json::json!(["HS"]));
        let data = MarkdownData {
            filename: "person/harald_stürmer.md".to_string(),
            created: 0,
            updated: 0,
            title: "Harald".to_string(),
            header: Header { fields },
            todo: vec![],
            link: vec![],
            body: "".to_string(),
            segments: vec![],
            tags: vec![],
        };
        write_markdown_data_to_sqlite(&data, &db_path)?;
        let conn = DatabaseService::new(db_path.to_str().unwrap()).connect()?;

        for link in ["Harald Stürmer", "harald_stürmer.md", "hs"] {
            assert_eq!(resolve_link(&conn, link).as_deref(), Some("person/harald_stürmer.md"), "{}", link);
        }
        assert_eq!(resolve_link(&conn, "stürmer"), None);

        assert_eq!(attribute_text(&serde_json::json!(["a", null, "b"])).as_deref(), Some("a, b"));
        assert_eq!(attribute_text(&serde_json::json!(null)), None);
        assert_eq!(attribute_text(&serde_json::json!(3)).as_deref(), Some("3"));
        Ok(())
    }
}
