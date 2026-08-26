use pulldown_cmark::{html, Options, Parser as MdParser};
use rusqlite::Connection;
use std::fs;
use std::path::Path;
use std::process;

const DEFAULT_CSS: &str = include_str!("../templates/export_default.css");

pub fn handle_export(
    database: &str,
    filename: &str,
    output: Option<&str>,
    stylesheet: Option<&str>,
) {
    let db_path = Path::new(database);
    crate::commands::require_db_exists(db_path, database);

    let resolved = match resolve_filename(db_path, filename) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    };

    let note = match get_note(db_path, &resolved) {
        Ok(note) => note,
        Err(e) => {
            eprintln!("Error loading note: {}", e);
            process::exit(1);
        }
    };

    let css = match stylesheet {
        Some(path) => match fs::read_to_string(path) {
            Ok(css) => css,
            Err(e) => {
                eprintln!("Error reading stylesheet '{}': {}", path, e);
                process::exit(1);
            }
        },
        None => DEFAULT_CSS.to_string(),
    };

    let html_doc = render_html(&note, &css);

    let output_path = output
        .map(|o| o.to_string())
        .unwrap_or_else(|| default_output_path(&resolved));

    if let Err(e) = fs::write(&output_path, html_doc) {
        eprintln!("Error writing '{}': {}", output_path, e);
        process::exit(1);
    }

    println!("Exported '{}' to '{}'", resolved, output_path);
}

struct Note {
    filename: String,
    title: Option<String>,
    body: String,
}

fn resolve_filename(db_path: &Path, filename: &str) -> Result<String, Box<dyn std::error::Error>> {
    let conn = Connection::open(db_path)?;

    let exact: Option<String> = conn
        .query_row(
            "SELECT filename FROM markdown_data WHERE filename = ?",
            [filename],
            |row| row.get(0),
        )
        .ok();
    if let Some(f) = exact {
        return Ok(f);
    }

    let mut stmt = conn.prepare("SELECT filename FROM markdown_data WHERE filename LIKE ?")?;
    let pattern = format!("%/{}", filename);
    let matches: Vec<String> = stmt
        .query_map([&pattern], |row| row.get(0))?
        .collect::<Result<_, _>>()?;

    match matches.len() {
        0 => Err(format!("Document '{}' not found", filename).into()),
        1 => Ok(matches[0].clone()),
        _ => Err(format!(
            "Multiple documents found for '{}': {}. Please specify the full path.",
            filename,
            matches.join(", ")
        )
        .into()),
    }
}

fn get_note(db_path: &Path, filename: &str) -> Result<Note, Box<dyn std::error::Error>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare("SELECT filename, title, body FROM markdown_data WHERE filename = ?")?;
    let mut rows = stmt.query([filename])?;
    if let Some(row) = rows.next()? {
        Ok(Note {
            filename: row.get(0)?,
            title: row.get(1)?,
            body: row.get(2)?,
        })
    } else {
        Err(format!("Document '{}' not found", filename).into())
    }
}

fn render_html(note: &Note, css: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);

    let parser = MdParser::new_ext(&note.body, options);
    let mut body_html = String::new();
    html::push_html(&mut body_html, parser);

    let title = note.title.clone().unwrap_or_else(|| note.filename.clone());

    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>{title}</title>\n<style>\n{css}\n</style>\n</head>\n<body>\n<h1>{title}</h1>\n<div class=\"note-meta\">{filename}</div>\n{body}\n</body>\n</html>\n",
        title = html_escape(&title),
        css = css,
        filename = html_escape(&note.filename),
        body = body_html,
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn default_output_path(filename: &str) -> String {
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("note");
    format!("{}.html", stem)
}
