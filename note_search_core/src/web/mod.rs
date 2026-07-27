use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Html,
    routing::get,
    Json, Router,
};
use crate::query_parser::parse_query;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use crate::database_service::{DatabaseService, NoteResult, TodoResult};
use crate::search_criteria::SearchCriteria;
use crate::commands::backlinks::get_backlinks;
use std::path::Path;

#[derive(Deserialize)]
struct SearchParams {
    /// Legacy plain-text search (exact match on t.text / m.header_fields).
    text: Option<String>,
    /// Obsidian-like query string (e.g. `@orchard`, `#tag`, `[[link]]`, `word`,
    /// `[attr:val]`, `(A OR B)`). When present, takes precedence over `text`.
    q: Option<String>,
    attributes: Option<String>,
    /// Which results to return: `all` (default), `notes`, or `todos`.
    kind: Option<String>,
}
#[derive(Deserialize)]
struct NoteParams {
    filename: String,
}
#[derive(Deserialize)]
struct AttributeValuesParams {
    key: String,
}

#[derive(Serialize)]
struct SearchResponse {
    notes: Vec<NoteResult>,
    todos: Vec<TodoResult>,
}

#[derive(Serialize)]
struct NoteViewResponse {
    filename: String,
    title: String,
    content: String,
    backlinks: Vec<String>,
}

#[derive(Serialize)]
struct GraphNode {
    id: String,
    title: String,
    kind: Option<String>,
}

#[derive(Serialize)]
struct GraphEdge {
    source: String,
    target: String,
}

#[derive(Serialize)]
struct GraphResponse {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
}

pub async fn start_server(port: u16, database: String, watch: bool) {
    let note_dir = std::env::var("NOTE_SEARCH_DIR").unwrap_or_else(|_| ".".to_string());
    let db_service = Arc::new(DatabaseService::new(&database));

    // One-line startup banner so the user can confirm the binary they're
    // running is the one that's serving and which DB it is reading.
    eprintln!("note_search web: serving on http://0.0.0.0:{port}");
    eprintln!("  database: {}", db_service.database_path);
    eprintln!("  note_dir: {note_dir}");

    if watch {
        let watch_dir = note_dir.clone();
        let watch_db_path = database.clone();
        eprintln!("  watch: re-importing '{}' every 60s", watch_dir);
        std::thread::spawn(move || watch_and_reimport(&watch_dir, &watch_db_path));
    }

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/search", get(search_handler))
        .route("/api/tags", get(tags_handler))
        .route("/api/links", get(links_handler))
        .route("/api/attributes", get(attributes_handler))
        .route("/api/attribute-values", get(attribute_values_handler))
        .route("/api/graph", get(graph_handler))
        .route("/api/note", get(move |state, query| note_handler(state, query, note_dir)))
        .with_state(db_service);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("Web server running on http://{}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

/// Periodically re-imports `note_dir` into `db_path`, mirroring `import --watch`.
/// Runs on its own OS thread for the lifetime of the web server.
fn watch_and_reimport(note_dir: &str, db_path: &str) {
    let input_path = std::path::Path::new(note_dir);
    let db_path = std::path::Path::new(db_path);
    let mut file_mtimes = std::collections::HashMap::new();

    if let Err(e) =
        crate::commands::import::do_import_with_tracking(input_path, db_path, &mut file_mtimes)
    {
        eprintln!("web watch: initial import failed: {}", e);
    }

    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
        if let Err(e) =
            crate::commands::import::do_import_with_tracking(input_path, db_path, &mut file_mtimes)
        {
            eprintln!("web watch: import failed: {}", e);
        }
    }
}

async fn index_handler() -> Html<&'static str> {
    Html(include_str!("static/index.html"))
}

async fn search_handler(
    State(db_service): State<Arc<DatabaseService>>,
    Query(params): Query<SearchParams>,
) -> Result<Json<SearchResponse>, (StatusCode, String)> {
    let mut criteria = SearchCriteria::default();

    // Prefer the Obsidian-like `q` query string (it supports links, tags, attrs,
    // OR-groups). Fall back to legacy `text` for plain-word search.
    if let Some(q) = params.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        match parse_query(q) {
            Ok(expr) => criteria.query_expr = Some(expr),
            Err(e) => {
                eprintln!("web: failed to parse query {:?}: {}", q, e);
            }
        }
    } else {
        criteria.text = params.text.clone();
        criteria.search_body = params.text;
    }

    // Simple attribute parsing: assume "key=value"
    if let Some(attr_str) = params.attributes {
        let parts: Vec<&str> = attr_str.split('=').collect();
        if parts.len() == 2 {
            criteria.attributes.push(crate::attribute_pair::AttributePair::new(parts[0], parts[1]));
        }
    }

    let db_error = |e: rusqlite::Error| (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {e}"));

    let kind = params.kind.as_deref().unwrap_or("all");
    let (notes, todos) = match kind {
        "notes" => (db_service.search_notes(&criteria).map_err(db_error)?, Vec::new()),
        "todos" => (Vec::new(), db_service.search_todos(&criteria).map_err(db_error)?),
        _ => (
            db_service.search_notes(&criteria).map_err(db_error)?,
            db_service.search_todos(&criteria).map_err(db_error)?,
        ),
    };

    Ok(Json(SearchResponse { notes, todos }))
}

/// Normalize a note name/link for cross-matching: lowercase, underscores as
/// spaces. Same scheme `agenda.rs`'s `projects_map` and `backlinks.rs`'s
/// `is_match` use, so graph edges resolve link targets the same way
/// `--links` search and `agenda` already do.
fn normalize_graph_name(s: &str) -> String {
    s.to_lowercase().replace('_', " ")
}

/// Build the node list plus a normalized-name -> filename lookup table
/// (covering each note's full filename and its basename) used to resolve
/// link targets to actual notes.
fn build_graph_nodes(
    notes: &[(String, Option<String>, Option<String>)],
) -> (Vec<GraphNode>, std::collections::HashMap<String, String>) {
    let mut nodes = Vec::with_capacity(notes.len());
    let mut lookup = std::collections::HashMap::new();

    for (filename, title, header_fields) in notes {
        let kind = header_fields.as_deref().and_then(|h| {
            serde_json::from_str::<serde_json::Value>(h).ok().and_then(|v| {
                v.get("type")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string())
            })
        });

        let basename = Path::new(filename)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| filename.clone());

        lookup.insert(normalize_graph_name(filename), filename.clone());
        lookup.insert(normalize_graph_name(&basename), filename.clone());

        nodes.push(GraphNode {
            id: filename.clone(),
            title: title.clone().unwrap_or_else(|| filename.clone()),
            kind,
        });
    }

    (nodes, lookup)
}

/// Resolve `(filename, link)` pairs into deduplicated edges between notes
/// that actually exist in the vault. A link that doesn't resolve to a known
/// note is skipped - no placeholder nodes for nonexistent targets.
fn resolve_graph_edges(
    links: &[(String, String)],
    lookup: &std::collections::HashMap<String, String>,
) -> Vec<GraphEdge> {
    let mut seen = std::collections::HashSet::new();
    let mut edges = Vec::new();

    for (filename, link) in links {
        let Some(target) = lookup.get(&normalize_graph_name(link)) else {
            continue;
        };
        if target == filename {
            continue;
        }
        let pair = if filename < target {
            (filename.clone(), target.clone())
        } else {
            (target.clone(), filename.clone())
        };
        if seen.insert(pair.clone()) {
            edges.push(GraphEdge {
                source: pair.0,
                target: pair.1,
            });
        }
    }

    edges
}

async fn graph_handler(
    State(db_service): State<Arc<DatabaseService>>,
) -> Result<Json<GraphResponse>, (StatusCode, String)> {
    let db_error =
        |e: rusqlite::Error| (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {e}"));
    let conn = db_service.connect().map_err(db_error)?;

    let mut notes_stmt = conn
        .prepare("SELECT filename, title, header_fields FROM markdown_data")
        .map_err(db_error)?;
    let notes: Vec<(String, Option<String>, Option<String>)> = notes_stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .map_err(db_error)?
        .filter_map(Result::ok)
        .collect();
    drop(notes_stmt);

    let (nodes, lookup) = build_graph_nodes(&notes);

    let mut links_stmt = conn
        .prepare("SELECT filename, link FROM note_links")
        .map_err(db_error)?;
    let links: Vec<(String, String)> = links_stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(db_error)?
        .filter_map(Result::ok)
        .collect();
    drop(links_stmt);

    let edges = resolve_graph_edges(&links, &lookup);

    Ok(Json(GraphResponse { nodes, edges }))
}

async fn note_handler(
    State(db_service): State<Arc<DatabaseService>>,
    Query(params): Query<NoteParams>,
    note_dir: String,
) -> Json<NoteViewResponse> {
    let conn = db_service.connect().expect("Failed to connect to database");
    
    let full_path = Path::new(&note_dir).join(&params.filename);
    
    let (title, body): (Option<String>, String) = conn
        .query_row(
            "SELECT title, body FROM markdown_data WHERE filename = ?",
            [&params.filename],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap_or_else(|_| (Some("Not found".to_string()), format!("Could not find: {} (Path: {})", params.filename, full_path.display())));

    let backlinks = get_backlinks(Path::new(&db_service.database_path), &params.filename).unwrap_or_default();
    
    let content = body;

    Json(NoteViewResponse {
        filename: params.filename,
        title: title.unwrap_or_default(),
        content,
        backlinks,
    })
}

#[derive(Debug, serde::Serialize)]
struct ValuesResponse {
    values: Vec<String>,
}

async fn tags_handler(
    State(db_service): State<Arc<DatabaseService>>,
) -> Result<Json<ValuesResponse>, (StatusCode, String)> {
    let values = crate::commands::metadata::get_unique_values(
        Path::new(&db_service.database_path),
        "tag",
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {e}")))?;
    Ok(Json(ValuesResponse { values }))
}

async fn links_handler(
    State(db_service): State<Arc<DatabaseService>>,
) -> Result<Json<ValuesResponse>, (StatusCode, String)> {
    let values = crate::commands::metadata::get_unique_values(
        Path::new(&db_service.database_path),
        "link",
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {e}")))?;
    Ok(Json(ValuesResponse { values }))
}

async fn attributes_handler(
    State(db_service): State<Arc<DatabaseService>>,
) -> Result<Json<ValuesResponse>, (StatusCode, String)> {
    let values =
        crate::commands::metadata::get_all_attributes(Path::new(&db_service.database_path))
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {e}")))?;
    Ok(Json(ValuesResponse { values }))
}

async fn attribute_values_handler(
    State(db_service): State<Arc<DatabaseService>>,
    Query(params): Query<AttributeValuesParams>,
) -> Result<Json<ValuesResponse>, (StatusCode, String)> {
    let field = format!("attr:{}", params.key);
    let values =
        crate::commands::metadata::get_unique_values(Path::new(&db_service.database_path), &field)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {e}")))?;
    Ok(Json(ValuesResponse { values }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_edges_resolve_by_basename_and_filename_skip_unresolved() {
        let notes = vec![
            ("a.md".to_string(), Some("Note A".to_string()), None),
            ("sub/b.md".to_string(), Some("Note B".to_string()), None),
        ];
        let (nodes, lookup) = build_graph_nodes(&notes);
        assert_eq!(nodes.len(), 2);

        let links = vec![
            ("a.md".to_string(), "b".to_string()), // resolves by basename
            ("a.md".to_string(), "sub/b.md".to_string()), // resolves by full filename, same edge -> deduped
            ("a.md".to_string(), "Nonexistent".to_string()), // unresolved -> skipped
            ("a.md".to_string(), "a".to_string()), // self-link -> skipped
        ];
        let edges = resolve_graph_edges(&links, &lookup);

        assert_eq!(edges.len(), 1);
        let mut pair = [edges[0].source.clone(), edges[0].target.clone()];
        pair.sort();
        assert_eq!(pair, ["a.md".to_string(), "sub/b.md".to_string()]);
    }

    #[test]
    fn test_graph_edges_underscore_space_normalization() {
        let notes = vec![("project_x.md".to_string(), Some("Project X".to_string()), None)];
        let (_, lookup) = build_graph_nodes(&notes);

        let links = vec![("other.md".to_string(), "Project X".to_string())];
        let edges = resolve_graph_edges(&links, &lookup);

        assert_eq!(edges.len(), 1);
    }

    #[test]
    fn test_graph_nodes_extract_kind_from_header_fields() {
        let notes = vec![(
            "p.md".to_string(),
            Some("P".to_string()),
            Some(r#"{"type":"project"}"#.to_string()),
        )];
        let (nodes, _) = build_graph_nodes(&notes);
        assert_eq!(nodes[0].kind.as_deref(), Some("project"));
    }
}