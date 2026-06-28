use crate::query_builder::{Parameter, QueryBuilder};
use crate::query_parser::parse_query;
use crate::search_criteria::SearchCriteria;
use chrono::{DateTime, Local};
use rusqlite::{Connection, Result};
use serde::Serialize;
use std::path::Path;

pub struct DatabaseService {
    pub database_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NoteResult {
    pub filename: String,
    pub title: Option<String>,
    pub header_fields: Option<String>,
    pub links: Option<String>,
    pub todo_count: i32,
    pub link_count: i32,
    pub created: Option<i64>,
    pub updated: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TodoResult {
    pub filename: String,
    pub line_number: i32,
    pub text: String,
    pub tags: Option<String>,
    pub links: Option<String>,
    pub priority: Option<String>,
    pub due_date: Option<String>,
    pub header_fields: Option<String>,
}

impl DatabaseService {
    pub fn new(database_path: &str) -> Self {
        DatabaseService {
            database_path: database_path.to_string(),
        }
    }

    pub fn connect(&self) -> Result<Connection> {
        Connection::open(&self.database_path)
    }

    pub fn search_todos(&self, criteria: &SearchCriteria) -> Result<Vec<TodoResult>> {
        let builder = QueryBuilder::new().build_query(criteria);
        let query = builder.get_query();
        let parameters = builder.get_parameters();

        let conn = self.connect()?;

        let mut stmt = conn.prepare(query)?;

        // Convert parameters to rusqlite types
        let param_refs: Vec<Box<dyn rusqlite::ToSql>> = parameters
            .iter()
            .map(|p| -> Box<dyn rusqlite::ToSql> {
                match p {
                    Parameter::Text(s) => Box::new(s.clone()),
                    Parameter::Int(i) => Box::new(*i),
                }
            })
            .collect();

        let param_refs_slice: Vec<&dyn rusqlite::ToSql> =
            param_refs.iter().map(|p| p.as_ref()).collect();

        let rows = stmt.query_map(param_refs_slice.as_slice(), |row| {
            Ok(TodoResult {
                filename: row.get("filename")?,
                line_number: row.get("line_number")?,
                text: row.get("text")?,
                tags: row.get("tags").ok(),
                links: row.get("links").ok(),
                priority: row.get("priority").ok(),
                due_date: row.get("due").ok(),
                header_fields: row.get("header_fields").ok(),
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }

        Ok(results)
    }

    pub fn search_notes(&self, criteria: &SearchCriteria) -> Result<Vec<NoteResult>> {
        let builder = QueryBuilder::new().build_note_query(criteria);
        let query = builder.get_query();
        let parameters = builder.get_parameters();

        let conn = self.connect()?;

        let mut stmt = conn.prepare(query)?;

        // Convert parameters to rusqlite types
        let param_refs: Vec<Box<dyn rusqlite::ToSql>> = parameters
            .iter()
            .map(|p| -> Box<dyn rusqlite::ToSql> {
                match p {
                    Parameter::Text(s) => Box::new(s.clone()),
                    Parameter::Int(i) => Box::new(*i),
                }
            })
            .collect();

        let param_refs_slice: Vec<&dyn rusqlite::ToSql> =
            param_refs.iter().map(|p| p.as_ref()).collect();

        let rows = stmt.query_map(param_refs_slice.as_slice(), |row| {
            Ok(NoteResult {
                filename: row.get("filename")?,
                title: row.get("title").ok(),
                header_fields: row.get("header_fields").ok(),
                links: row.get("links").ok(),
                todo_count: row.get("todo_count")?,
                link_count: row.get("link_count")?,
                created: row.get("created").ok(),
                updated: row.get("updated").ok(),
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }

        Ok(results)
    }

    /// Parse an Obsidian-like query string and return matching notes.
    ///
    /// This is a convenience method that combines `parse_query` and `search_notes`
    /// into a single call. Each result includes the note's filename, title,
    /// created timestamp, and updated timestamp.
    ///
    /// # Arguments
    ///
    /// * `query_str` - An Obsidian-like query string supporting words, `[[links]]`,
    ///   `#tags`, `[attributes]`, and `(OR groups)`.
    ///
    /// # Returns
    ///
    /// * `Ok(Vec<NoteResult>)` - Matching notes with filename, title, created, updated
    /// * `Err(String)` - If the query is invalid or a database error occurs
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let service = DatabaseService::new("notes.db");
    /// let notes = service.search_notes_by_query("#urgent [created:2026-01-01]")?;
    /// for note in notes {
    ///     println!("{} - {}", note.filename, note.title.unwrap_or_default());
    /// }
    /// ```
    pub fn search_notes_by_query(&self, query_str: &str) -> std::result::Result<Vec<NoteResult>, String> {
        let expr = parse_query(query_str).map_err(|e| format!("Query parse error: {}", e))?;
        let criteria = SearchCriteria {
            database_path: self.database_path.clone(),
            query_expr: Some(expr),
            ..Default::default()
        };
        self.search_notes(&criteria).map_err(|e| format!("Database error: {}", e))
    }
}

impl TodoResult {
    pub fn formatted_string(
        &self,
        format: &Option<String>,
        absolute_path: bool,
        base_path: &str,
    ) -> String {
        let filename = if absolute_path {
            Path::new(base_path)
                .join(&self.filename)
                .to_string_lossy()
                .to_string()
        } else {
            self.filename.clone()
        };

        match format {
            Some(f) if !f.is_empty() => self.apply_format(f, &filename),
            _ => format!("\"{}\":{} {}", filename, self.line_number, self.text),
        }
    }

    fn apply_format(&self, format: &str, filename: &str) -> String {
        let mut result = String::new();
        let mut i = 0;
        let chars: Vec<char> = format.chars().collect();

        while i < chars.len() {
            if chars[i] == '{' {
                if let Some(end) = format[i + 1..].find('}') {
                    let placeholder = &format[i + 1..i + 1 + end];
                    let value = self.resolve_placeholder(placeholder, filename);
                    result.push_str(&value);
                    i = i + 1 + end + 1;
                } else {
                    result.push(chars[i]);
                    i += 1;
                }
            } else {
                result.push(chars[i]);
                i += 1;
            }
        }

        result
    }

    fn resolve_placeholder(&self, placeholder: &str, filename: &str) -> String {
        match placeholder.to_lowercase().as_str() {
            "filename" => filename.to_string(),
            "line_number" => self.line_number.to_string(),
            "text" => self.text.clone(),
            "priority" => self.priority.clone().unwrap_or_default(),
            "due_date" => self.due_date.clone().unwrap_or_default(),
            "tags" => self.tags.clone().unwrap_or_default(),
            "links" => self.links.clone().unwrap_or_default(),
            _ => {
                if placeholder.to_lowercase().starts_with("attr:") {
                    let attr_name = &placeholder[5..];
                    self.extract_attribute_from_header(attr_name)
                } else {
                    format!("{{{}}}", placeholder)
                }
            }
        }
    }

    fn extract_attribute_from_header(&self, attr_name: &str) -> String {
        let header_fields = match &self.header_fields {
            Some(h) => h,
            None => return String::new(),
        };

        if let Ok(map) =
            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(header_fields)
        {
            if let Some(value) = map.get(attr_name) {
                match value {
                    serde_json::Value::String(s) => return s.clone(),
                    serde_json::Value::Array(arr) => {
                        let items: Vec<String> = arr
                            .iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect();
                        return format!("[{}]", items.join(", "));
                    }
                    _ => return value.to_string(),
                }
            }
        }
        String::new()
    }
}

fn format_timestamp(unix_secs: i64) -> String {
    match DateTime::from_timestamp(unix_secs, 0) {
        Some(dt) => dt.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string(),
        None => unix_secs.to_string(),
    }
}

impl NoteResult {
    pub fn formatted_string(
        &self,
        format: &Option<String>,
        absolute_path: bool,
        base_path: &str,
    ) -> String {
        let filename = if absolute_path {
            Path::new(base_path)
                .join(&self.filename)
                .to_string_lossy()
                .to_string()
        } else {
            self.filename.clone()
        };

        match format {
            Some(f) if !f.is_empty() => self.apply_format(f, &filename),
            _ => format!(
                "{} [{} todos, {} links]",
                filename, self.todo_count, self.link_count
            ),
        }
    }

    fn apply_format(&self, format: &str, filename: &str) -> String {
        let mut result = String::new();
        let mut i = 0;
        let chars: Vec<char> = format.chars().collect();

        while i < chars.len() {
            if chars[i] == '{' {
                if let Some(end) = format[i + 1..].find('}') {
                    let placeholder = &format[i + 1..i + 1 + end];
                    let value = self.resolve_placeholder(placeholder, filename);
                    result.push_str(&value);
                    i = i + 1 + end + 1;
                } else {
                    result.push(chars[i]);
                    i += 1;
                }
            } else {
                result.push(chars[i]);
                i += 1;
            }
        }

        result
    }

    fn resolve_placeholder(&self, placeholder: &str, filename: &str) -> String {
        match placeholder.to_lowercase().as_str() {
            "filename" => filename.to_string(),
            "title" => self.title.clone().unwrap_or_default(),
            "todo_count" => self.todo_count.to_string(),
            "link_count" => self.link_count.to_string(),
            "links" => self.links.clone().unwrap_or_default(),
            "created" => self
                .created
                .map(format_timestamp)
                .unwrap_or_default(),
            "updated" => self
                .updated
                .map(format_timestamp)
                .unwrap_or_default(),
            _ => {
                if placeholder.to_lowercase().starts_with("attr:") {
                    let attr_name = &placeholder[5..];
                    self.extract_attribute_from_header(attr_name)
                } else {
                    format!("{{{}}}", placeholder)
                }
            }
        }
    }

    fn extract_attribute_from_header(&self, attr_name: &str) -> String {
        let header_fields = match &self.header_fields {
            Some(h) => h,
            None => return String::new(),
        };

        if let Ok(map) =
            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(header_fields)
        {
            if let Some(value) = map.get(attr_name) {
                match value {
                    serde_json::Value::String(s) => return s.clone(),
                    serde_json::Value::Array(arr) => {
                        let items: Vec<String> = arr
                            .iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect();
                        return format!("[{}]", items.join(", "));
                    }
                    _ => return value.to_string(),
                }
            }
        }
        String::new()
    }
}
