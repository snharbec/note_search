use crate::query_builder::{Parameter, QueryBuilder};
use crate::search_criteria::SearchCriteria;
use rusqlite::{Connection, Result};
use std::path::Path;

pub struct DatabaseService {
    database_path: String,
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct NoteResult {
    pub filename: String,
    pub title: Option<String>,
    pub header_fields: Option<String>,
    pub links: Option<String>,
    pub todo_count: i32,
    pub link_count: i32,
}

impl DatabaseService {
    pub fn new(database_path: &str) -> Self {
        DatabaseService {
            database_path: database_path.to_string(),
        }
    }

    pub fn search_todos(&self, criteria: &SearchCriteria) -> Result<Vec<TodoResult>> {
        let builder = QueryBuilder::new().build_query(criteria);
        let query = builder.get_query();
        let parameters = builder.get_parameters();

        let conn = Connection::open(&self.database_path)?;

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

        let rows = stmt.query_map(&*param_refs_slice.as_slice(), |row| {
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

        let conn = Connection::open(&self.database_path)?;

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

        let rows = stmt.query_map(&*param_refs_slice.as_slice(), |row| {
            Ok(NoteResult {
                filename: row.get("filename")?,
                title: row.get("title").ok(),
                header_fields: row.get("header_fields").ok(),
                links: row.get("links").ok(),
                todo_count: row.get("todo_count")?,
                link_count: row.get("link_count")?,
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }

        Ok(results)
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
