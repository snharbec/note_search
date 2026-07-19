use std::collections::HashSet;
use std::path::Path;
use std::process;

pub fn handle_values(database: &str, field: &str) {
    let db_path = Path::new(database);

    if !db_path.exists() {
        eprintln!("Error: Database '{}' does not exist", database);
        process::exit(1);
    }

    match get_unique_values(db_path, field) {
        Ok(values) => {
            if values.is_empty() {
                println!("No values found for field '{}'", field);
            } else {
                println!("Unique values for '{}':", field);
                for value in values {
                    println!("  {}", value);
                }
            }
        }
        Err(e) => {
            eprintln!("Error getting values: {}", e);
            process::exit(1);
        }
    }
}

pub fn get_unique_values(
    db_path: &Path,
    field: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    use rusqlite::Connection;

    let conn = Connection::open(db_path)?;
    // Ensures note_tags/note_links exist (and are backfilled) on a database
    // that predates the tag/link junction tables.
    crate::markdown_parser::init_database_schema(&conn)?;
    let mut values = HashSet::new();

    let field_lower = field.trim().to_lowercase();

    match field_lower.as_str() {
        "priority" => {
            let mut stmt = conn
                .prepare("SELECT DISTINCT priority FROM todo_entries WHERE priority IS NOT NULL")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            for row in rows {
                values.insert(row?);
            }
        }
        "due_date" | "duedate" | "due" => {
            let mut stmt =
                conn.prepare("SELECT DISTINCT due FROM todo_entries WHERE due IS NOT NULL")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            for row in rows {
                values.insert(row?);
            }
        }
        "tag" | "tags" => {
            // note_tags is the full per-note tag aggregate (todo tags, body
            // hashtags, and frontmatter-declared tags all folded in at write
            // time), so it alone covers everything the old todo_entries +
            // markdown_data JSON scan did.
            let mut stmt = conn.prepare("SELECT DISTINCT tag FROM note_tags")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            for row in rows {
                values.insert(row?);
            }
        }
        "link" | "links" => {
            let mut stmt = conn.prepare("SELECT DISTINCT link FROM note_links")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            for row in rows {
                values.insert(row?);
            }
        }
        _ if field_lower.starts_with("attr:") => {
            let attr_name = field_lower[5..].trim().to_string();
            if !attr_name.is_empty() {
                let mut stmt = conn.prepare("SELECT DISTINCT header_fields FROM markdown_data WHERE header_fields IS NOT NULL")?;
                let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
                for row in rows {
                    let header_json: String = row?;
                    if let Ok(header_map) = serde_json::from_str::<
                        serde_json::Map<String, serde_json::Value>,
                    >(&header_json)
                    {
                        if let Some(value) = header_map.get(&attr_name) {
                            // Handle both single values and arrays
                            match value {
                                serde_json::Value::String(s) => {
                                    values.insert(s.clone());
                                }
                                serde_json::Value::Array(arr) => {
                                    for item in arr {
                                        if let Some(s) = item.as_str() {
                                            values.insert(s.to_string());
                                        }
                                    }
                                }
                                _ => {
                                    values.insert(value.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
        _ => {
            return Err(format!(
                "Unknown field: {}. Use: priority, due_date, tag, link, or attr:ATTRIBUTE",
                field
            )
            .into());
        }
    }

    // Convert HashSet to sorted Vec
    let mut result: Vec<String> = values.into_iter().collect();
    result.sort();

    Ok(result)
}

pub fn handle_attributes(database: &str) {
    let db_path = Path::new(database);

    if !db_path.exists() {
        eprintln!("Error: Database '{}' does not exist", database);
        process::exit(1);
    }

    match get_all_attributes(db_path) {
        Ok(attrs) => {
            if attrs.is_empty() {
                println!("No attributes found in database.");
            } else {
                println!("Known attributes:");
                for attr in attrs {
                    println!("  {}", attr);
                }
            }
        }
        Err(e) => {
            eprintln!("Error getting attributes: {}", e);
            process::exit(1);
        }
    }
}

pub fn get_all_attributes(db_path: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    use rusqlite::Connection;
    use std::collections::HashSet;

    let conn = Connection::open(db_path)?;
    let mut attributes = HashSet::new();

    let mut stmt = conn.prepare(
        "SELECT DISTINCT header_fields FROM markdown_data WHERE header_fields IS NOT NULL",
    )?;

    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;

    for row in rows {
        let header_json: String = row?;
        if let Ok(header_map) =
            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&header_json)
        {
            for key in header_map.keys() {
                attributes.insert(key.clone());
            }
        }
    }

    let mut result: Vec<String> = attributes.into_iter().collect();
    result.sort();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown_parser::{write_markdown_data_to_sqlite, Header, MarkdownData};
    use serde_json::json;
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn test_get_unique_values() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");

        let mut fields = HashMap::new();
        fields.insert("type".to_string(), json!("meeting"));
        fields.insert("author".to_string(), json!(["john", "jane"]));

        let data = MarkdownData {
            filename: "test.md".to_string(),
            created: 1234567890,
            updated: 1234567890,
            title: "Test".to_string(),
            header: Header { fields },
            todo: vec![],
            link: vec!["link1".to_string()],
            body: "".to_string(),
            elements: vec![],
        };

        write_markdown_data_to_sqlite(&data, &db_path)?;

        // Test attr:type
        let values = get_unique_values(&db_path, "attr:type")?;
        assert_eq!(values, vec!["meeting"]);

        // Test attr:author
        let values = get_unique_values(&db_path, "attr:author")?;
        assert_eq!(values, vec!["jane", "john"]);

        Ok(())
    }
}
