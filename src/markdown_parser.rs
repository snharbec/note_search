use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::LazyLock;
use std::time::SystemTime;

static TODO_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?m)^- \[([ xX])\] (.*)$").unwrap());
static PRIORITY_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"priority:\s*([A-Z])").unwrap());
static DUE_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"due:\s*(\d{4}-\d{2}-\d{2}|\d{8})").unwrap());
static TAG_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"#([a-zA-Z0-9_]+)").unwrap());
static TAG_ATTR_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"tag:\s*([a-zA-Z0-9_]+)").unwrap());
static LINK_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\[([^\]]+)\]\(([^)]+)\)").unwrap());
static WIKI_LINK_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\[\[([^\]]+)\]\]").unwrap());
static WIKI_LINK_FIELD_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^\[\[([^\]]+)\]\]$").unwrap());
static DATAVIEW_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?s)```dataview\n.*?```").unwrap());
static TASKS_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?s)```tasks\n.*?```").unwrap());

#[derive(Debug, Serialize, Deserialize)]
pub struct TodoEntry {
    pub closed: bool,
    pub priority: Option<String>,
    pub due: Option<String>,
    pub tags: Vec<String>,
    pub links: Vec<String>,
    pub line_number: usize,
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Header {
    #[serde(flatten)]
    pub fields: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MarkdownData {
    pub filename: String,
    pub created: u64,
    pub updated: u64,
    pub title: String,
    pub header: Header,
    pub todo: Vec<TodoEntry>,
    pub link: Vec<String>,
    pub body: String,
}

pub fn remove_dataview_sections(content: &str) -> String {
    let without_dataview = DATAVIEW_REGEX.replace_all(content, "");
    TASKS_REGEX.replace_all(&without_dataview, "").to_string()
}

pub fn remove_hash_prefixes(content: &str) -> String {
    // Remove # prefixes from all values in the frontmatter content
    // This handles patterns like "key: #value" or "key: #value1 #value2 #value3"
    let mut result = String::new();

    for line in content.lines() {
        if let Some(colon_pos) = line.find(':') {
            // Get the key including the colon and any whitespace after it
            let before_value = &line[..colon_pos + 1];
            let rest = &line[colon_pos + 1..];

            // Find where the actual value starts (skip whitespace after colon)
            let value_start = rest.len() - rest.trim_start().len();
            let whitespace = &rest[..value_start];
            let value_part = &rest[value_start..];

            // Remove all # prefixes from the value part
            let cleaned_value = value_part
                .split_whitespace()
                .map(|word| {
                    if word.starts_with('#') {
                        &word[1..]
                    } else {
                        word
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");

            result.push_str(before_value);
            result.push_str(whitespace);
            result.push_str(&cleaned_value);
        } else {
            result.push_str(line);
        }
        result.push('\n');
    }

    // Remove trailing newline if original didn't have one
    if !content.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }

    result
}

/// Parse a date string from frontmatter into a Unix timestamp (seconds since epoch)
/// Supports multiple formats:
/// - "[[yyyy-MM-dd]]" (e.g., "[[2024-08-05]]")
/// - "yyyy-MM-dd" (e.g., "2024-08-05")
/// - "[[yyyy-MM-dd]] hh:mm" (e.g., "[[2024-08-05]] 17:08")
/// - "yyyy-MM-dd hh:mm" (e.g., "2024-08-05 17:08")
/// - Unix timestamp (e.g., "1704067200")
pub fn parse_date_string(date_str: &str) -> Option<u64> {
    let trimmed = date_str.trim();

    // Try parsing as Unix timestamp (all digits)
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return trimmed.parse().ok();
    }

    // Extract date from [[yyyy-MM-dd]] format if present
    let mut date_part = trimmed;
    let mut time_part = "";

    // Check for [[date]] format and extract it
    if trimmed.starts_with("[[") && trimmed.contains("]]") {
        if let Some(start) = trimmed.find("[[") {
            if let Some(end) = trimmed.find("]]") {
                date_part = &trimmed[start + 2..end];
                // Check if there's a time part after ]]
                let after_brackets = &trimmed[end + 2..];
                if !after_brackets.is_empty() {
                    time_part = after_brackets.trim();
                }
            }
        }
    } else if trimmed.len() >= 10 {
        // Check for yyyy-MM-dd format with optional time
        let potential_date = &trimmed[..10];
        if potential_date.chars().nth(4) == Some('-') && potential_date.chars().nth(7) == Some('-')
        {
            date_part = potential_date;
            if trimmed.len() > 10 {
                time_part = trimmed[10..].trim();
            }
        }
    }

    // Parse the date part
    let date = chrono::NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()?;

    // Parse optional time part
    let (hour, minute) = if time_part.is_empty() {
        (0, 0)
    } else {
        // Try to parse hh:mm format
        let time_trimmed = time_part.trim();
        if time_trimmed.len() >= 5 && time_trimmed.chars().nth(2) == Some(':') {
            let hour_str = &time_trimmed[..2];
            let minute_str = &time_trimmed[3..5];
            let hour = hour_str.parse::<u32>().ok()?;
            let minute = minute_str.parse::<u32>().ok()?;
            (hour, minute)
        } else {
            (0, 0)
        }
    };

    let datetime = date.and_hms_opt(hour, minute, 0)?;
    Some(datetime.and_utc().timestamp() as u64)
}

pub fn extract_frontmatter(content: &str) -> Option<(String, String, usize)> {
    let lines: Vec<&str> = content.lines().collect();

    // Check if first line is "---"
    if lines.is_empty() || lines[0].trim() != "---" {
        return None;
    }

    // Count lines until we find the closing "---"
    let mut frontmatter_line_count = 1; // Start with 1 for the opening "---"
    let mut frontmatter_end = 0;

    for (i, line) in lines.iter().enumerate().skip(1) {
        frontmatter_line_count += 1;
        if line.trim() == "---" {
            frontmatter_end = i;
            break;
        }
    }

    // If we didn't find a closing "---", there's no valid frontmatter
    if frontmatter_end == 0 {
        return None;
    }

    // Extract frontmatter and body
    let frontmatter = lines[1..frontmatter_end].join("\n");
    let body = lines[frontmatter_end + 1..].join("\n");

    Some((frontmatter, body, frontmatter_line_count))
}

pub fn extract_title_from_filename(filename: &str) -> String {
    filename.trim_end_matches(".md").to_string()
}

pub fn extract_title_from_frontmatter(frontmatter_content: &str) -> Option<String> {
    let yaml = yaml_rust2::YamlLoader::load_from_str(frontmatter_content);
    match yaml {
        Ok(yamls) => {
            if let Some(yaml) = yamls.first() {
                if let Some(title) = yaml["title"].as_str() {
                    return Some(title.to_string());
                }
            }
        }
        Err(_) => {}
    }
    None
}

pub fn extract_todo_entries(markdown_content: &str) -> Vec<TodoEntry> {
    let mut todos = Vec::new();
    let mut line_number = 0;

    for line in markdown_content.lines() {
        line_number += 1;
        if let Some(captures) = TODO_REGEX.captures(line) {
            let closed = captures[1].trim() == "x" || captures[1].trim() == "X";
            let content = captures[2].trim();

            let mut priority = None;
            let mut due = None;
            let mut tags = Vec::new();
            let mut links = Vec::new();

            if let Some(priority_match) = PRIORITY_REGEX.captures(content) {
                priority = Some(priority_match[1].to_string());
            }

            if let Some(due_match) = DUE_REGEX.captures(content) {
                let due_str = &due_match[1];
                // Normalize to YYYYMMDD format (remove dashes if present)
                let normalized_due = due_str.replace("-", "");
                due = Some(normalized_due);
            }

            for tag_capture in TAG_REGEX.captures_iter(content) {
                tags.push(tag_capture[1].to_string());
            }

            for tag_capture in TAG_ATTR_REGEX.captures_iter(content) {
                tags.push(tag_capture[1].to_string());
            }

            for link_capture in LINK_REGEX.captures_iter(content) {
                links.push(link_capture[2].to_string());
            }

            for link_capture in WIKI_LINK_REGEX.captures_iter(content) {
                links.push(link_capture[1].to_string());
            }

            todos.push(TodoEntry {
                closed,
                priority,
                due,
                tags,
                links,
                line_number,
                text: content.to_string(),
            });
        }
    }

    todos
}

/// Convert dates in YYYY-MM-DD format to wiki links [[YYYY-MM-DD]]
pub fn convert_dates_to_wiki_links(content: &str) -> String {
    // Pattern to match dates in YYYY-MM-DD format
    // Matches: word boundary + 4 digits + hyphen + 2 digits + hyphen + 2 digits + word boundary
    let date_regex = regex::Regex::new(r"\b(\d{4})-(\d{2})-(\d{2})\b").unwrap();

    let mut result = String::new();
    let mut last_end = 0;

    for caps in date_regex.captures_iter(content) {
        let mat = caps.get(0).unwrap();
        let start = mat.start();
        let end = mat.end();

        // Check if this date is already inside wiki links [[...]]
        // by looking for [[ before and ]] after the date
        let is_in_wiki_link = is_inside_wiki_link(content, start, end);

        // Add content before this match
        result.push_str(&content[last_end..start]);

        if is_in_wiki_link {
            // Keep the original date if it's already in a wiki link
            result.push_str(mat.as_str());
        } else {
            // Convert to wiki link
            let year = &caps[1];
            let month = &caps[2];
            let day = &caps[3];
            result.push_str(&format!("[[{}-{}-{}]]", year, month, day));
        }

        last_end = end;
    }

    // Add remaining content
    result.push_str(&content[last_end..]);

    result
}

/// Check if a position in content is inside a wiki link [[...]]
fn is_inside_wiki_link(content: &str, start: usize, end: usize) -> bool {
    // Look backwards for [[
    let before = &content[..start];
    let last_open = before.rfind("[[");
    let last_close = before.rfind("]]");

    // If we found [[ after the last ]], we might be inside a wiki link
    let inside_open_link = match (last_open, last_close) {
        (Some(open), Some(close)) => open > close,
        (Some(_), None) => true,
        _ => false,
    };

    if !inside_open_link {
        return false;
    }

    // Look forwards for ]]
    let after = &content[end..];
    let next_close = after.find("]]");
    let next_open = after.find("[[");

    // If we found ]] before the next [[, we're inside a wiki link
    match (next_close, next_open) {
        (Some(close), Some(open)) => close < open,
        (Some(_), None) => true,
        _ => false,
    }
}

pub fn extract_links(markdown_content: &str) -> Vec<String> {
    let mut links = Vec::new();
    for link_capture in LINK_REGEX.captures_iter(markdown_content) {
        links.push(link_capture[2].to_string());
    }

    for link_capture in WIKI_LINK_REGEX.captures_iter(markdown_content) {
        links.push(link_capture[1].to_string());
    }

    links
}

pub fn yaml_to_json_value(value: &yaml_rust2::Yaml) -> serde_json::Value {
    match value {
        yaml_rust2::Yaml::Real(v) => serde_json::Value::String(v.to_string()),
        yaml_rust2::Yaml::Integer(v) => serde_json::Value::Number(serde_json::Number::from(*v)),
        yaml_rust2::Yaml::String(v) => {
            let v_str = v.as_str();
            if let Some(captures) = WIKI_LINK_FIELD_REGEX.captures(v_str) {
                serde_json::Value::String(captures[1].to_string())
            } else {
                serde_json::Value::String(v.clone())
            }
        }
        yaml_rust2::Yaml::Boolean(v) => serde_json::Value::Bool(*v),
        yaml_rust2::Yaml::Array(v) => {
            let mut vec = Vec::new();
            for item in v {
                vec.push(yaml_to_json_value(item));
            }
            serde_json::Value::Array(vec)
        }
        yaml_rust2::Yaml::Hash(v) => {
            let mut map = serde_json::Map::new();
            for (key, val) in v {
                if let Some(key_str) = key.as_str() {
                    map.insert(key_str.to_string(), yaml_to_json_value(val));
                }
            }
            serde_json::Value::Object(map)
        }
        yaml_rust2::Yaml::Null => serde_json::Value::Null,
        yaml_rust2::Yaml::Alias(_) | yaml_rust2::Yaml::BadValue => {
            serde_json::Value::String("".to_string())
        }
    }
}

pub fn process_markdown_file(
    file_path: &Path,
    input_dir: &Path,
) -> Result<MarkdownData, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(file_path)?;
    let relative_path = file_path
        .strip_prefix(input_dir)?
        .to_str()
        .unwrap_or("")
        .to_string();

    let (frontmatter_content, markdown_body, frontmatter_line_count) =
        match extract_frontmatter(&content) {
            Some((front, body, line_count)) => (front, body, line_count),
            None => (String::new(), content, 0),
        };

    let mut header_fields = HashMap::new();
    let mut frontmatter_links = Vec::new();
    if !frontmatter_content.is_empty() {
        for capture in WIKI_LINK_REGEX.captures_iter(&frontmatter_content) {
            let link = capture[1].to_string();
            if !frontmatter_links.contains(&link) {
                frontmatter_links.push(link);
            }
        }

        let frontmatter_for_fields =
            remove_hash_prefixes(&frontmatter_content.replace("[[", "").replace("]]", ""));
        let yaml = yaml_rust2::YamlLoader::load_from_str(&frontmatter_for_fields);
        match yaml {
            Ok(yamls) => {
                if let Some(yaml) = yamls.first() {
                    if let Some(hash) = yaml.as_hash() {
                        for (key, value) in hash {
                            if let Some(key_str) = key.as_str() {
                                header_fields
                                    .insert(key_str.to_string(), yaml_to_json_value(value));
                            }
                        }
                    }
                }
            }
            Err(_) => {}
        }
    }

    let title =
        extract_title_from_frontmatter(&frontmatter_content.replace("[[", "").replace("]]", ""))
            .unwrap_or_else(|| extract_title_from_filename(&relative_path));

    // Remove Dataview sections before parsing content
    let body_without_dataview = remove_dataview_sections(&markdown_body);

    // Convert dates to wiki links before extracting links
    let body_with_date_links = convert_dates_to_wiki_links(&body_without_dataview);

    let mut todos = extract_todo_entries(&body_without_dataview);

    // Adjust line numbers to account for frontmatter
    for todo in &mut todos {
        todo.line_number += frontmatter_line_count;
    }
    let mut body_links = extract_links(&body_with_date_links);

    body_links.extend(frontmatter_links);
    let mut seen = HashSet::new();
    let unique_links: Vec<String> = body_links
        .into_iter()
        .filter(|link| seen.insert(link.clone()))
        .collect();

    let updated = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_secs();

    // Get created timestamp from frontmatter or file metadata
    let created = header_fields
        .get("created")
        .and_then(|v| v.as_str())
        .and_then(|s| parse_date_string(s))
        .unwrap_or_else(|| {
            // Fall back to file creation time
            fs::metadata(file_path)
                .ok()
                .and_then(|m| m.created().ok())
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(updated) // Fall back to updated time if all else fails
        });

    Ok(MarkdownData {
        filename: relative_path,
        created,
        updated,
        title,
        header: Header {
            fields: header_fields,
        },
        todo: todos,
        link: unique_links,
        body: body_without_dataview,
    })
}

pub fn init_database_schema(conn: &rusqlite::Connection) -> Result<(), Box<dyn std::error::Error>> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS markdown_data (
            filename TEXT PRIMARY KEY,
            created INTEGER,
            updated INTEGER,
            title TEXT,
            todo_count INTEGER,
            link_count INTEGER,
            header_fields TEXT,
            links TEXT,
            body TEXT,
            tags TEXT
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS todo_entries (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            filename TEXT,
            closed BOOLEAN,
            priority TEXT,
            due TEXT,
            text TEXT,
            tags TEXT,
            links TEXT,
            line_number INTEGER,
            FOREIGN KEY (filename) REFERENCES markdown_data(filename)
        )",
        [],
    )?;

    let _ = conn.execute("ALTER TABLE markdown_data ADD COLUMN created INTEGER", []);
    let _ = conn.execute("ALTER TABLE markdown_data ADD COLUMN tags TEXT", []);

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_todo_entries_filename ON todo_entries(filename)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_todo_entries_closed ON todo_entries(closed)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_todo_entries_priority ON todo_entries(priority)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_todo_entries_due ON todo_entries(due)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_markdown_data_filename ON markdown_data(filename)",
        [],
    )?;

    Ok(())
}

pub fn write_markdown_data_to_sqlite_with_conn(
    data: &MarkdownData,
    conn: &rusqlite::Connection,
) -> Result<(), Box<dyn std::error::Error>> {
    let header_json = serde_json::to_string(&data.header.fields)?;
    let links_json = serde_json::to_string(&data.link)?;

    let mut all_tags: HashSet<&String> = HashSet::new();
    for todo in &data.todo {
        for tag in &todo.tags {
            all_tags.insert(tag);
        }
    }
    let tags_json = serde_json::to_string(&all_tags.iter().cloned().collect::<Vec<_>>())?;

    conn.execute(
        "DELETE FROM todo_entries WHERE filename = ?1",
        rusqlite::params![data.filename],
    )?;

    conn.execute(
        "INSERT OR REPLACE INTO markdown_data
         (filename, created, updated, title, todo_count, link_count, header_fields, links, body, tags)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            data.filename,
            data.created as i64,
            data.updated as i64,
            data.title,
            data.todo.len() as i64,
            data.link.len() as i64,
            header_json,
            links_json,
            data.body,
            tags_json
        ],
    )?;

    for todo in &data.todo {
        let tags_json = serde_json::to_string(&todo.tags)?;
        let links_json = serde_json::to_string(&todo.links)?;

        conn.execute(
            "INSERT INTO todo_entries
             (filename, closed, priority, due, text, tags, links, line_number)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                data.filename,
                todo.closed,
                todo.priority.as_ref().map(|s| s.as_str()),
                todo.due.as_ref().map(|s| s.as_str()),
                todo.text,
                tags_json,
                links_json,
                todo.line_number as i64
            ],
        )?;
    }

    Ok(())
}

pub fn write_markdown_data_to_sqlite(
    data: &MarkdownData,
    db_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use rusqlite::Connection;

    let conn = Connection::open(db_path)?;
    init_database_schema(&conn)?;
    write_markdown_data_to_sqlite_with_conn(data, &conn)?;
    Ok(())
}

/// Remove notes from the database that no longer exist on the filesystem
pub fn remove_orphaned_notes(
    input_dir: &Path,
    conn: &rusqlite::Connection,
) -> Result<usize, Box<dyn std::error::Error>> {
    // Get all filenames currently in the database
    let mut stmt = conn.prepare("SELECT filename FROM markdown_data")?;
    let db_filenames: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(Result::ok)
        .collect();

    let mut removed_count = 0;
    for filename in db_filenames {
        let file_path = input_dir.join(&filename);
        if !file_path.exists() {
            conn.execute(
                "DELETE FROM todo_entries WHERE filename = ?1",
                rusqlite::params![filename],
            )?;
            conn.execute(
                "DELETE FROM markdown_data WHERE filename = ?1",
                rusqlite::params![filename],
            )?;
            removed_count += 1;
        }
    }

    Ok(removed_count)
}

pub fn parse_markdown_directory_batch(
    input_dir: &Path,
    db_path: &Path,
) -> Result<usize, Box<dyn std::error::Error>> {
    use rusqlite::Connection;

    let mut conn = Connection::open(db_path)?;
    init_database_schema(&conn)?;

    let tx = conn.transaction()?;
    let mut count = 0;

    for entry in walkdir::WalkDir::new(input_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file() && e.path().extension().map_or(false, |ext| ext == "md")
        })
    {
        let data = process_markdown_file(entry.path(), input_dir)?;
        write_markdown_data_to_sqlite_with_conn(&data, &tx)?;
        count += 1;
    }

    tx.commit()?;

    // Remove notes that no longer exist on the filesystem
    let removed = remove_orphaned_notes(input_dir, &conn)?;
    if removed > 0 {
        println!("Removed {} orphaned notes from database", removed);
    }

    Ok(count)
}

pub fn parse_markdown_directory(
    input_dir: &Path,
    db_path: &Path,
) -> Result<usize, Box<dyn std::error::Error>> {
    parse_markdown_directory_batch(input_dir, db_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_extract_frontmatter_with_valid_frontmatter() {
        let content = "---\ntitle: Test\n---\n# Body\nSome text";
        let result = extract_frontmatter(content);
        assert!(result.is_some());
        let (front, body, line_count) = result.unwrap();
        // Frontmatter content without the delimiters
        assert_eq!(front, "title: Test");
        // Body should be everything after the frontmatter
        assert_eq!(body, "# Body\nSome text");
        // line count: opening --- (1) + title line (1) + closing --- (1) = 3
        assert_eq!(line_count, 3);
    }

    #[test]
    fn test_extract_frontmatter_without_frontmatter() {
        let content = "# Body\nSome text";
        let result = extract_frontmatter(content);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_frontmatter_empty() {
        let content = "";
        let result = extract_frontmatter(content);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_frontmatter_no_closing() {
        let content = "---\ntitle: Test\n# Body without closing";
        let result = extract_frontmatter(content);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_title_from_filename() {
        assert_eq!(extract_title_from_filename("test.md"), "test");
        assert_eq!(extract_title_from_filename("my-file.md"), "my-file");
        assert_eq!(
            extract_title_from_filename("path/to/file.md"),
            "path/to/file"
        );
    }

    #[test]
    fn test_extract_title_from_filename_without_extension() {
        assert_eq!(extract_title_from_filename("test"), "test");
    }

    #[test]
    fn test_extract_title_from_frontmatter() {
        let frontmatter = "title: My Document\nauthor: John";
        let result = extract_title_from_frontmatter(frontmatter);
        assert_eq!(result, Some("My Document".to_string()));
    }

    #[test]
    fn test_extract_title_from_frontmatter_no_title() {
        let frontmatter = "author: John\ndate: 2024-01-01";
        let result = extract_title_from_frontmatter(frontmatter);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_title_from_frontmatter_empty() {
        let frontmatter = "";
        let result = extract_title_from_frontmatter(frontmatter);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_todo_entries_open() {
        let content = "- [ ] First todo\n- [ ] Second todo";
        let todos = extract_todo_entries(content);
        assert_eq!(todos.len(), 2);
        assert!(!todos[0].closed);
        assert!(!todos[1].closed);
        assert_eq!(todos[0].text, "First todo");
        assert_eq!(todos[1].text, "Second todo");
    }

    #[test]
    fn test_extract_todo_entries_closed() {
        let content = "- [x] Completed todo\n- [X] Also completed";
        let todos = extract_todo_entries(content);
        assert_eq!(todos.len(), 2);
        assert!(todos[0].closed);
        assert!(todos[1].closed);
    }

    #[test]
    fn test_extract_todo_entries_with_priority() {
        let content = "- [ ] High priority priority: A\n- [ ] Low priority priority: C";
        let todos = extract_todo_entries(content);
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].priority, Some("A".to_string()));
        assert_eq!(todos[1].priority, Some("C".to_string()));
    }

    #[test]
    fn test_extract_todo_entries_with_due_date() {
        let content = "- [ ] Due soon due: 20241231";
        let todos = extract_todo_entries(content);
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].due, Some("20241231".to_string()));
    }

    #[test]
    fn test_extract_todo_entries_with_tags() {
        let content = "- [ ] Feature todo #feature #important";
        let todos = extract_todo_entries(content);
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].tags, vec!["feature", "important"]);
    }

    #[test]
    fn test_extract_todo_entries_with_tag_attr() {
        let content = "- [ ] Tagged todo tag: review tag: urgent";
        let todos = extract_todo_entries(content);
        assert_eq!(todos.len(), 1);
        assert!(todos[0].tags.contains(&"review".to_string()));
        assert!(todos[0].tags.contains(&"urgent".to_string()));
    }

    #[test]
    fn test_extract_todo_entries_with_markdown_links() {
        let content = "- [ ] Check [documentation](https://example.com)";
        let todos = extract_todo_entries(content);
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].links, vec!["https://example.com"]);
    }

    #[test]
    fn test_extract_todo_entries_with_wiki_links() {
        let content = "- [ ] Read [[Related Page]] and [[Another Page]]";
        let todos = extract_todo_entries(content);
        assert_eq!(todos.len(), 1);
        assert!(todos[0].links.contains(&"Related Page".to_string()));
        assert!(todos[0].links.contains(&"Another Page".to_string()));
    }

    #[test]
    fn test_extract_todo_entries_line_numbers() {
        let content = "Line 1\nLine 2\n- [ ] Todo on line 3\nLine 4\n- [ ] Todo on line 5";
        let todos = extract_todo_entries(content);
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].line_number, 3);
        assert_eq!(todos[1].line_number, 5);
    }

    #[test]
    fn test_extract_todo_entries_empty() {
        let content = "No todos here\nJust regular text";
        let todos = extract_todo_entries(content);
        assert!(todos.is_empty());
    }

    #[test]
    fn test_extract_links_markdown() {
        let content = "Check [link1](https://a.com) and [link2](https://b.com)";
        let links = extract_links(content);
        assert_eq!(links, vec!["https://a.com", "https://b.com"]);
    }

    #[test]
    fn test_extract_links_wiki() {
        let content = "See [[Page One]] and [[Page Two]]";
        let links = extract_links(content);
        assert!(links.contains(&"Page One".to_string()));
        assert!(links.contains(&"Page Two".to_string()));
    }

    #[test]
    fn test_extract_links_mixed() {
        let content = "[Web link](https://example.com) and [[Wiki link]]";
        let links = extract_links(content);
        assert!(links.contains(&"https://example.com".to_string()));
        assert!(links.contains(&"Wiki link".to_string()));
    }

    #[test]
    fn test_extract_links_empty() {
        let content = "No links here";
        let links = extract_links(content);
        assert!(links.is_empty());
    }

    #[test]
    fn test_convert_dates_to_wiki_links() {
        // Case 1: Plain dates should be converted
        let content = "Meeting scheduled for 2026-04-12 and follow-up on 2026-04-15";
        let result = convert_dates_to_wiki_links(content);
        assert_eq!(
            result,
            "Meeting scheduled for [[2026-04-12]] and follow-up on [[2026-04-15]]"
        );
    }

    #[test]
    fn test_convert_dates_preserves_existing_simple_links() {
        // Case 2: Dates already in [[YYYY-MM-DD]] should NOT be touched
        let content = "See [[2026-04-14]] and also 2026-04-15";
        let result = convert_dates_to_wiki_links(content);
        assert!(
            result.contains("[[2026-04-14]]"),
            "Simple wiki link should be preserved"
        );
        assert!(
            result.contains("[[2026-04-15]]"),
            "Plain date should be converted"
        );
        // Make sure we don't get double brackets
        assert!(
            !result.contains("[[[[2026-04-14]]]]"),
            "Should not create double brackets"
        );
    }

    #[test]
    fn test_convert_dates_preserves_complex_wiki_links() {
        // Case 3: Dates inside complex wiki links like [[Tasks-2026-04-14-DOIT]] should NOT be touched
        let content = "Task [[Tasks-2026-04-14-DOIT]] and date 2026-04-12";
        let result = convert_dates_to_wiki_links(content);
        // The date inside the complex wiki link should remain unchanged
        assert!(
            result.contains("[[Tasks-2026-04-14-DOIT]]"),
            "Complex wiki link should be preserved"
        );
        // The plain date should be converted
        assert!(
            result.contains("[[2026-04-12]]"),
            "Plain date should be converted"
        );
    }

    #[test]
    fn test_convert_dates_no_dates() {
        let content = "No dates in this content";
        let result = convert_dates_to_wiki_links(content);
        assert_eq!(result, content);
    }

    #[test]
    fn test_convert_dates_multiple_in_complex_link() {
        // Multiple dates inside a complex wiki link
        let content = "[[Project-2026-04-12-to-2026-04-15]] and 2026-04-20";
        let result = convert_dates_to_wiki_links(content);
        // Both dates in the complex link should be preserved
        assert!(
            result.contains("[[Project-2026-04-12-to-2026-04-15]]"),
            "Complex link with multiple dates should be preserved"
        );
        // The plain date should be converted
        assert!(
            result.contains("[[2026-04-20]]"),
            "Plain date should be converted"
        );
    }

    #[test]
    fn test_yaml_to_json_value_string() {
        let yaml = yaml_rust2::Yaml::String("test".to_string());
        let json = yaml_to_json_value(&yaml);
        assert_eq!(json, serde_json::json!("test"));
    }

    #[test]
    fn test_yaml_to_json_value_integer() {
        let yaml = yaml_rust2::Yaml::Integer(42);
        let json = yaml_to_json_value(&yaml);
        assert_eq!(json, serde_json::json!(42));
    }

    #[test]
    fn test_yaml_to_json_value_boolean() {
        let yaml = yaml_rust2::Yaml::Boolean(true);
        let json = yaml_to_json_value(&yaml);
        assert_eq!(json, serde_json::json!(true));
    }

    #[test]
    fn test_yaml_to_json_value_null() {
        let yaml = yaml_rust2::Yaml::Null;
        let json = yaml_to_json_value(&yaml);
        assert_eq!(json, serde_json::json!(null));
    }

    #[test]
    fn test_yaml_to_json_value_array() {
        let yaml = yaml_rust2::Yaml::Array(vec![
            yaml_rust2::Yaml::String("a".to_string()),
            yaml_rust2::Yaml::String("b".to_string()),
        ]);
        let json = yaml_to_json_value(&yaml);
        assert_eq!(json, serde_json::json!(["a", "b"]));
    }

    #[test]
    fn test_yaml_to_json_value_wiki_link() {
        let yaml = yaml_rust2::Yaml::String("[[Page Name]]".to_string());
        let json = yaml_to_json_value(&yaml);
        assert_eq!(json, serde_json::json!("Page Name"));
    }

    #[test]
    fn test_process_markdown_file_no_frontmatter() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let file_path = input_dir.join("test.md");

        let mut file = fs::File::create(&file_path)?;
        writeln!(file, "# Title\n\n- [ ] Todo item")?;

        let data = process_markdown_file(&file_path, input_dir)?;
        assert_eq!(data.filename, "test.md");
        assert_eq!(data.title, "test");
        assert_eq!(data.todo.len(), 1);
        assert_eq!(data.header.fields.len(), 0);

        Ok(())
    }

    #[test]
    fn test_process_markdown_file_with_frontmatter() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let file_path = input_dir.join("test.md");

        let mut file = fs::File::create(&file_path)?;
        writeln!(
            file,
            "---\ntitle: My Document\nauthor: John\n---\n\n# Body\n\n- [ ] Todo"
        )?;

        let data = process_markdown_file(&file_path, input_dir)?;
        assert_eq!(data.filename, "test.md");
        assert_eq!(data.title, "My Document");
        assert_eq!(data.todo.len(), 1);
        assert!(data.header.fields.contains_key("author"));

        Ok(())
    }

    #[test]
    fn test_process_markdown_file_subdir() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let subdir = input_dir.join("subdir");
        fs::create_dir(&subdir)?;
        let file_path = subdir.join("test.md");

        let mut file = fs::File::create(&file_path)?;
        writeln!(file, "- [ ] Todo in subdir")?;

        let data = process_markdown_file(&file_path, input_dir)?;
        assert_eq!(data.filename, "subdir/test.md");

        Ok(())
    }

    #[test]
    fn test_write_markdown_data_to_sqlite() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");

        let data = MarkdownData {
            filename: "test.md".to_string(),
            created: 1234567890,
            updated: 1234567890,
            title: "Test".to_string(),
            header: Header {
                fields: HashMap::new(),
            },
            todo: vec![TodoEntry {
                closed: false,
                priority: Some("A".to_string()),
                due: Some("20241231".to_string()),
                tags: vec!["feature".to_string()],
                links: vec!["https://example.com".to_string()],
                line_number: 5,
                text: "Test todo".to_string(),
            }],
            link: vec!["https://example.com".to_string()],
            body: "This is the test note body content.".to_string(),
        };

        write_markdown_data_to_sqlite(&data, &db_path)?;

        // Verify database was created and has data
        let conn = rusqlite::Connection::open(&db_path)?;
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM markdown_data", [], |row| row.get(0))?;
        assert_eq!(count, 1);

        let todo_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM todo_entries", [], |row| row.get(0))?;
        assert_eq!(todo_count, 1);

        Ok(())
    }

    #[test]
    fn test_parse_markdown_directory() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let db_path = temp_dir.path().join("test.db");

        // Create test files
        fs::write(input_dir.join("file1.md"), "- [ ] Todo 1")?;
        fs::write(input_dir.join("file2.md"), "- [ ] Todo 2")?;

        // Create subdirectory with file
        let subdir = input_dir.join("subdir");
        fs::create_dir(&subdir)?;
        fs::write(subdir.join("file3.md"), "- [ ] Todo 3")?;

        let count = parse_markdown_directory(input_dir, &db_path)?;
        assert_eq!(count, 3);

        // Verify database contents
        let conn = rusqlite::Connection::open(&db_path)?;
        let file_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM markdown_data", [], |row| row.get(0))?;
        assert_eq!(file_count, 3);

        Ok(())
    }

    #[test]
    fn test_parse_markdown_directory_empty() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let db_path = temp_dir.path().join("test.db");

        let count = parse_markdown_directory(input_dir, &db_path)?;
        assert_eq!(count, 0);

        Ok(())
    }

    #[test]
    fn test_remove_dataview_sections() {
        let content = r#"# My Note

Some content here

```dataview
LIST
FROM "Projects"
WHERE completed = false
```

More content after dataview

- [ ] A real todo

```dataview
TABLE file.name, file.size
FROM "Documents"
```

Final content
"#;

        let filtered = remove_dataview_sections(content);

        // Should contain the todos and regular content
        assert!(filtered.contains("Some content here"));
        assert!(filtered.contains("More content after dataview"));
        assert!(filtered.contains("A real todo"));
        assert!(filtered.contains("Final content"));

        // Should NOT contain dataview content
        assert!(!filtered.contains("FROM \"Projects\""));
        assert!(!filtered.contains("file.size"));
        assert!(!filtered.contains("```dataview"));
    }

    #[test]
    fn test_extract_todo_entries_ignores_dataview() {
        let content = r#"- [ ] Real todo
```dataview
- [ ] This is dataview syntax not a todo
```
- [ ] Another real todo"#;

        // Must filter dataview sections before extracting, as done in process_markdown_file
        let filtered = remove_dataview_sections(content);
        let todos = extract_todo_entries(&filtered);
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].text, "Real todo");
        assert_eq!(todos[1].text, "Another real todo");
    }

    #[test]
    fn test_extract_links_ignores_dataview() {
        let content = r#"Check [this link](https://example.com)
```dataview
[[Page in dataview]]
```
See [[Wiki Link]]"#;

        // Must filter dataview sections before extracting, as done in process_markdown_file
        let filtered = remove_dataview_sections(content);
        let links = extract_links(&filtered);
        assert!(links.contains(&"https://example.com".to_string()));
        assert!(links.contains(&"Wiki Link".to_string()));
        assert!(!links.contains(&"Page in dataview".to_string()));
    }

    #[test]
    fn test_remove_tasks_sections() {
        let content = r#"# My Note

Some content here

```tasks
not done
path includes Projects
```

More content after tasks

- [ ] A real todo

```tasks
done
sort by due date
```

Final content
"#;

        let filtered = remove_dataview_sections(content);

        // Should contain the todos and regular content
        assert!(filtered.contains("Some content here"));
        assert!(filtered.contains("More content after tasks"));
        assert!(filtered.contains("A real todo"));
        assert!(filtered.contains("Final content"));

        // Should NOT contain tasks content
        assert!(!filtered.contains("not done"));
        assert!(!filtered.contains("sort by due date"));
        assert!(!filtered.contains("```tasks"));
    }

    #[test]
    fn test_extract_todo_entries_ignores_tasks() {
        let content = r#"- [ ] Real todo
```tasks
- [ ] This is tasks syntax not a real todo
not done
```
- [ ] Another real todo"#;

        // Must filter dataview sections before extracting, as done in process_markdown_file
        let filtered = remove_dataview_sections(content);
        let todos = extract_todo_entries(&filtered);
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].text, "Real todo");
        assert_eq!(todos[1].text, "Another real todo");
    }

    #[test]
    fn test_extract_links_ignores_tasks() {
        let content = r#"Check [this link](https://example.com)
```tasks
[[Page in tasks block]]
```
See [[Wiki Link]]"#;

        // Must filter dataview sections before extracting, as done in process_markdown_file
        let filtered = remove_dataview_sections(content);
        let links = extract_links(&filtered);
        assert!(links.contains(&"https://example.com".to_string()));
        assert!(links.contains(&"Wiki Link".to_string()));
        assert!(!links.contains(&"Page in tasks block".to_string()));
    }

    #[test]
    fn test_remove_mixed_code_sections() {
        let content = r#"# My Note

Some content here

```dataview
LIST
FROM "Projects"
```

Middle content

```tasks
not done
```

- [ ] A real todo

```dataview
TABLE file.name
```

Final content
"#;

        let filtered = remove_dataview_sections(content);

        // Should contain the todos and regular content
        assert!(filtered.contains("Some content here"));
        assert!(filtered.contains("Middle content"));
        assert!(filtered.contains("A real todo"));
        assert!(filtered.contains("Final content"));

        // Should NOT contain any code block content
        assert!(!filtered.contains("FROM \"Projects\""));
        assert!(!filtered.contains("not done"));
        assert!(!filtered.contains("file.name"));
        assert!(!filtered.contains("```dataview"));
        assert!(!filtered.contains("```tasks"));
    }

    #[test]
    fn test_file_update_replaces_old_content() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");

        // First import - file with 2 todos
        let data1 = MarkdownData {
            filename: "test.md".to_string(),
            created: 1234567880,
            updated: 1234567890,
            title: "Test".to_string(),
            header: Header {
                fields: HashMap::new(),
            },
            todo: vec![
                TodoEntry {
                    closed: false,
                    priority: Some("A".to_string()),
                    due: Some("20241231".to_string()),
                    tags: vec!["old".to_string()],
                    links: vec![],
                    line_number: 1,
                    text: "First todo".to_string(),
                },
                TodoEntry {
                    closed: true,
                    priority: None,
                    due: None,
                    tags: vec![],
                    links: vec![],
                    line_number: 2,
                    text: "Second todo".to_string(),
                },
            ],
            link: vec![],
            body: "Old body content".to_string(),
        };

        write_markdown_data_to_sqlite(&data1, &db_path)?;

        // Verify first import
        let conn = rusqlite::Connection::open(&db_path)?;
        let todo_count1: i64 = conn.query_row(
            "SELECT COUNT(*) FROM todo_entries WHERE filename = ?1",
            ["test.md"],
            |row| row.get(0),
        )?;
        assert_eq!(todo_count1, 2);

        let body1: String = conn.query_row(
            "SELECT body FROM markdown_data WHERE filename = ?1",
            ["test.md"],
            |row| row.get(0),
        )?;
        assert_eq!(body1, "Old body content");

        // Second import - same file with different content (1 todo, different body)
        let data2 = MarkdownData {
            filename: "test.md".to_string(),
            created: 1234567880,
            updated: 1234567891,
            title: "Updated Test".to_string(),
            header: Header {
                fields: HashMap::new(),
            },
            todo: vec![TodoEntry {
                closed: false,
                priority: Some("B".to_string()),
                due: Some("20250101".to_string()),
                tags: vec!["new".to_string()],
                links: vec!["https://example.com".to_string()],
                line_number: 5,
                text: "New todo".to_string(),
            }],
            link: vec!["https://example.com".to_string()],
            body: "New body content".to_string(),
        };

        write_markdown_data_to_sqlite(&data2, &db_path)?;

        // Verify second import - old todos should be deleted, new ones added
        let todo_count2: i64 = conn.query_row(
            "SELECT COUNT(*) FROM todo_entries WHERE filename = ?1",
            ["test.md"],
            |row| row.get(0),
        )?;
        assert_eq!(todo_count2, 1); // Should have only 1 todo now

        let todo_text: String = conn.query_row(
            "SELECT text FROM todo_entries WHERE filename = ?1",
            ["test.md"],
            |row| row.get(0),
        )?;
        assert_eq!(todo_text, "New todo"); // Should be the new todo, not old ones

        // Verify body was updated
        let body2: String = conn.query_row(
            "SELECT body FROM markdown_data WHERE filename = ?1",
            ["test.md"],
            |row| row.get(0),
        )?;
        assert_eq!(body2, "New body content");

        // Verify title was updated
        let title: String = conn.query_row(
            "SELECT title FROM markdown_data WHERE filename = ?1",
            ["test.md"],
            |row| row.get(0),
        )?;
        assert_eq!(title, "Updated Test");

        Ok(())
    }

    #[test]
    fn test_yaml_to_json_value_regular_string() {
        let yaml = yaml_rust2::Yaml::String("normal value".to_string());
        let json = yaml_to_json_value(&yaml);
        assert_eq!(json, serde_json::json!("normal value"));
    }

    #[test]
    fn test_process_markdown_file_with_tag_attribute() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let file_path = input_dir.join("test.md");

        let mut file = fs::File::create(&file_path)?;
        writeln!(
            file,
            "---\ntitle: My Document\ntype: #meeting\n---\n\n# Body\n\n- [ ] Todo"
        )?;

        let data = process_markdown_file(&file_path, input_dir)?;
        assert_eq!(data.filename, "test.md");
        assert_eq!(data.title, "My Document");

        // Check that the type field has the # stripped
        assert!(data.header.fields.contains_key("type"));
        assert_eq!(
            data.header.fields.get("type"),
            Some(&serde_json::json!("meeting"))
        );

        Ok(())
    }

    #[test]
    fn test_remove_hash_prefixes() {
        let content = "type: #meeting";
        let cleaned = remove_hash_prefixes(content);
        assert_eq!(cleaned, "type: meeting");
    }

    #[test]
    fn test_remove_hash_prefixes_multiple() {
        let content = "type: #meeting\ncategory: #work\nstatus: active";
        let cleaned = remove_hash_prefixes(content);
        assert_eq!(cleaned, "type: meeting\ncategory: work\nstatus: active");
    }

    #[test]
    fn test_remove_hash_prefixes_no_tags() {
        let content = "title: My Document\nstatus: active";
        let cleaned = remove_hash_prefixes(content);
        assert_eq!(cleaned, "title: My Document\nstatus: active");
    }

    #[test]
    fn test_remove_hash_prefixes_with_spaces() {
        let content = "type:   #meeting";
        let cleaned = remove_hash_prefixes(content);
        assert_eq!(cleaned, "type:   meeting");
    }

    #[test]
    fn test_remove_hash_prefixes_in_values() {
        let content = "tags: #feature #bug #urgent";
        let cleaned = remove_hash_prefixes(content);
        // Should remove all # from the value
        assert_eq!(cleaned, "tags: feature bug urgent");
    }

    #[test]
    fn test_process_markdown_file_with_mixed_attributes() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let file_path = input_dir.join("test.md");

        let mut file = fs::File::create(&file_path)?;
        writeln!(
            file,
            "---\ntitle: My Document\ntype: #meeting\ncategory: #work\nstatus: active\n---\n\n# Body"
        )?;

        let data = process_markdown_file(&file_path, input_dir)?;

        // Tag-like values should have # stripped
        assert_eq!(
            data.header.fields.get("type"),
            Some(&serde_json::json!("meeting"))
        );
        assert_eq!(
            data.header.fields.get("category"),
            Some(&serde_json::json!("work"))
        );
        // Regular strings stay as-is
        assert_eq!(
            data.header.fields.get("status"),
            Some(&serde_json::json!("active"))
        );

        Ok(())
    }

    #[test]
    fn test_parse_date_string_unix_timestamp() {
        let result = parse_date_string("1704067200");
        assert_eq!(result, Some(1704067200));
    }

    #[test]
    fn test_parse_date_string_iso_date() {
        // 2024-01-01 in Unix timestamp (UTC)
        let result = parse_date_string("2024-01-01");
        assert!(result.is_some());
        assert_eq!(result, Some(1704067200));
    }

    #[test]
    fn test_parse_date_string_with_brackets() {
        // [[yyyy-MM-dd]] format
        let result = parse_date_string("[[2024-01-01]]");
        assert!(result.is_some());
        assert_eq!(result, Some(1704067200));
    }

    #[test]
    fn test_parse_date_string_with_brackets_and_time() {
        // [[yyyy-MM-dd]] hh:mm format
        let result = parse_date_string("[[2024-01-01]] 17:08");
        assert!(result.is_some());
        // 2024-01-01 17:08:00 UTC = 1704128880
        assert_eq!(result, Some(1704128880));
    }

    #[test]
    fn test_parse_date_string_with_time() {
        // yyyy-MM-dd hh:mm format
        let result = parse_date_string("2024-01-01 17:08");
        assert!(result.is_some());
        // 2024-01-01 17:08:00 UTC = 1704128880
        assert_eq!(result, Some(1704128880));
    }

    #[test]
    fn test_parse_date_string_midnight() {
        let result = parse_date_string("2024-01-01 00:00");
        assert!(result.is_some());
        assert_eq!(result, Some(1704067200));
    }

    #[test]
    fn test_parse_date_string_invalid() {
        let result = parse_date_string("not a date");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_date_string_empty() {
        let result = parse_date_string("");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_markdown_directory_no_md_files() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let db_path = temp_dir.path().join("test.db");

        // Create non-markdown files
        fs::write(input_dir.join("file.txt"), "Text file")?;
        fs::write(input_dir.join("file.json"), "{}")?;

        let count = parse_markdown_directory(input_dir, &db_path)?;
        assert_eq!(count, 0);

        Ok(())
    }

    #[test]
    fn test_remove_orphaned_notes() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let db_path = temp_dir.path().join("test.db");

        // Create initial files
        fs::write(input_dir.join("existing.md"), "- [ ] Existing todo")?;
        fs::write(input_dir.join("to_be_deleted.md"), "- [ ] Will be deleted")?;

        // First import - both files exist
        let count1 = parse_markdown_directory(input_dir, &db_path)?;
        assert_eq!(count1, 2);

        // Verify both files are in database
        let conn = rusqlite::Connection::open(&db_path)?;
        let count_before: i64 =
            conn.query_row("SELECT COUNT(*) FROM markdown_data", [], |row| row.get(0))?;
        assert_eq!(count_before, 2);

        // Delete one file from filesystem
        fs::remove_file(input_dir.join("to_be_deleted.md"))?;

        // Run import again - should remove orphaned note
        let count2 = parse_markdown_directory(input_dir, &db_path)?;
        assert_eq!(count2, 1); // Only existing.md was imported

        // Verify only one file remains in database
        let count_after: i64 =
            conn.query_row("SELECT COUNT(*) FROM markdown_data", [], |row| row.get(0))?;
        assert_eq!(count_after, 1);

        // Verify it's the correct file
        let remaining_file: String =
            conn.query_row("SELECT filename FROM markdown_data LIMIT 1", [], |row| {
                row.get(0)
            })?;
        assert_eq!(remaining_file, "existing.md");

        // Verify orphaned todos were also removed
        let todo_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM todo_entries", [], |row| row.get(0))?;
        assert_eq!(todo_count, 1); // Only one todo from existing.md

        Ok(())
    }

    #[test]
    fn test_remove_orphaned_notes_with_subdirs() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let db_path = temp_dir.path().join("test.db");

        // Create subdirectories with files
        let subdir = input_dir.join("subdir");
        fs::create_dir(&subdir)?;
        fs::write(subdir.join("keep.md"), "- [ ] Keep this")?;
        fs::write(subdir.join("remove.md"), "- [ ] Remove this")?;

        // First import
        let count1 = parse_markdown_directory(input_dir, &db_path)?;
        assert_eq!(count1, 2);

        // Delete one file
        fs::remove_file(subdir.join("remove.md"))?;

        // Re-import
        let count2 = parse_markdown_directory(input_dir, &db_path)?;
        assert_eq!(count2, 1);

        // Verify database state
        let conn = rusqlite::Connection::open(&db_path)?;
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM markdown_data", [], |row| row.get(0))?;
        assert_eq!(count, 1);

        let remaining: String =
            conn.query_row("SELECT filename FROM markdown_data LIMIT 1", [], |row| {
                row.get(0)
            })?;
        assert_eq!(remaining, "subdir/keep.md");

        Ok(())
    }
}
