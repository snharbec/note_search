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
        .route("/api/projects", get(projects_handler))
        .route("/api/persons", get(persons_handler))
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

/// Build the set of attribute keys whose (post-mapping) value should be treated
/// as a member of the given concept. The mapping config may rename source
/// attributes to a canonical target key, so we include the target as well as
/// every source key that maps to it.
fn concept_keys(
    mapping: &crate::commands::mapping::MappingConfig,
    target: &str,
    extras: &[&str],
) -> std::collections::HashSet<String> {
    let mut keys = std::collections::HashSet::new();
    keys.insert(target.to_string());
    for extra in extras {
        keys.insert((*extra).to_string());
    }
    for (src, dst) in &mapping.mappings {
        if dst == target {
            keys.insert(src.clone());
        }
    }
    keys
}

/// Extract distinct string values for the given attribute keys from a
/// `header_fields` JSON string, collecting both string and array shapes.
fn extract_values_from_headers(
    header_fields_json: &str,
    keys: &std::collections::HashSet<String>,
) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(map) =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(header_fields_json)
    {
        for (k, v) in &map {
            if !keys.contains(k) {
                continue;
            }
            match v {
                serde_json::Value::String(s) => out.push(s.clone()),
                serde_json::Value::Array(arr) => {
                    for item in arr {
                        if let Some(s) = item.as_str() {
                            out.push(s.to_string());
                        }
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// Read all `header_fields` rows from the database and collect distinct values
/// found under any of the given attribute keys (post-mapping-aware via `keys`).
/// Also includes titles of notes whose `type` matches `fallback_type`, if given.
/// Returns the set of values AND diagnostic info (total rows, per-key match
/// counts, mapping sources) so callers can see *why* a result is empty.
fn collect_attribute_values(
    conn: &rusqlite::Connection,
    keys: &std::collections::HashSet<String>,
    fallback_type: Option<&str>,
) -> Result<(std::collections::HashSet<String>, CollectDebug), Box<dyn std::error::Error>> {
    let mut values: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut per_key: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut rows_scanned = 0usize;

    let mut stmt = conn.prepare(
        "SELECT header_fields FROM markdown_data WHERE header_fields IS NOT NULL",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    for row in rows {
        let row = row?;
        rows_scanned += 1;
        if let Ok(map) =
            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&row)
        {
            for (k, v) in &map {
                if !keys.contains(k) {
                    continue;
                }
                match v {
                    serde_json::Value::String(s) => {
                        if !s.is_empty() {
                            values.insert(s.clone());
                            *per_key.entry(k.clone()).or_insert(0) += 1;
                        }
                    }
                    serde_json::Value::Array(arr) => {
                        let mut n = 0;
                        for item in arr {
                            if let Some(s) = item.as_str() {
                                if !s.is_empty() {
                                    values.insert(s.to_string());
                                    n += 1;
                                }
                            }
                        }
                        if n > 0 {
                            *per_key.entry(k.clone()).or_insert(0) += n;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    let mut fallback_count = 0usize;
    if let Some(ftype) = fallback_type {
        let q = format!(
            "SELECT title FROM markdown_data WHERE json_extract(header_fields, '$.type') = '{}'",
            ftype.replace('\'', "''")
        );
        let mut stmt_t = conn.prepare(&q)?;
        let rows_t = stmt_t.query_map([], |row| row.get::<_, String>(0))?;
        for row in rows_t.flatten() {
            if !row.is_empty() {
                values.insert(row);
                fallback_count += 1;
            }
        }
    }

    let debug = CollectDebug {
        rows_scanned,
        per_key,
        fallback_count,
    };
    Ok((values, debug))
}

#[derive(Debug, serde::Serialize)]
struct CollectDebug {
    rows_scanned: usize,
    per_key: std::collections::BTreeMap<String, usize>,
    fallback_count: usize,
}

#[derive(Debug, serde::Serialize)]
struct SidebarResponse {
    values: Vec<String>,
    /// Diagnostic info so the user can see *why* the list is empty
    /// (e.g. wrong DB path, no mapped keys matching, no `type` fallback).
    #[serde(rename = "_debug")]
    debug: SidebarDebug,
}

#[derive(Debug, serde::Serialize)]
struct SidebarDebug {
    db_path: String,
    keys: Vec<String>,
    mapping: std::collections::BTreeMap<String, String>,
    rows_scanned: usize,
    per_key: std::collections::BTreeMap<String, usize>,
    fallback_count: usize,
}

async fn projects_handler(
    State(db_service): State<Arc<DatabaseService>>,
) -> Json<SidebarResponse> {
    let conn = db_service.connect().expect("Failed to connect to database");
    let mapping = crate::commands::mapping::MappingConfig::load();
    let keys = concept_keys(&mapping, "project", &[]);
    let (values, debug) = collect_attribute_values(&conn, &keys, Some("project"))
        .expect("Failed to collect project values");

    // Server-side log so the operator can see why the list is empty
    // without having to open devtools.
    eprintln!(
        "web /api/projects: db={} keys={:?} rows_scanned={} per_key={:?} fallback={} result={}",
        db_service.database_path,
        keys,
        debug.rows_scanned,
        debug.per_key,
        debug.fallback_count,
        values.len(),
    );

    let mut out: Vec<String> = values.into_iter().collect();
    out.sort();
    Json(SidebarResponse {
        values: out,
        debug: SidebarDebug {
            db_path: db_service.database_path.clone(),
            keys: {
                let mut v: Vec<String> = keys.iter().cloned().collect();
                v.sort();
                v
            },
            mapping: mapping.mappings.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            rows_scanned: debug.rows_scanned,
            per_key: debug.per_key,
            fallback_count: debug.fallback_count,
        },
    })
}

async fn persons_handler(
    State(db_service): State<Arc<DatabaseService>>,
) -> Json<SidebarResponse> {
    let conn = db_service.connect().expect("Failed to connect to database");
    let mapping = crate::commands::mapping::MappingConfig::load();
    let keys = concept_keys(
        &mapping,
        "person",
        &["participant", "people", "persons", "participants"],
    );
    let (values, debug) = collect_attribute_values(&conn, &keys, Some("person"))
        .expect("Failed to collect person values");

    eprintln!(
        "web /api/persons: db={} keys={:?} rows_scanned={} per_key={:?} fallback={} result={}",
        db_service.database_path,
        keys,
        debug.rows_scanned,
        debug.per_key,
        debug.fallback_count,
        values.len(),
    );

    let mut out: Vec<String> = values.into_iter().collect();
    out.sort();
    Json(SidebarResponse {
        values: out,
        debug: SidebarDebug {
            db_path: db_service.database_path.clone(),
            keys: {
                let mut v: Vec<String> = keys.iter().cloned().collect();
                v.sort();
                v
            },
            mapping: mapping.mappings.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            rows_scanned: debug.rows_scanned,
            per_key: debug.per_key,
            fallback_count: debug.fallback_count,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown_parser::{
        init_database_schema, write_markdown_data_to_sqlite_with_conn, Header, MarkdownData,
    };
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn insert_note(
        db_path: &Path,
        input_dir: &Path,
        filename: &str,
        fields: &[(&str, serde_json::Value)],
    ) {
        let mut map = HashMap::new();
        for (k, v) in fields {
            map.insert(k.to_string(), v.clone());
        }
        let data = MarkdownData {
            filename: filename.to_string(),
            created: 0,
            updated: 0,
            title: filename.to_string(),
            header: Header { fields: map },
            todo: vec![],
            link: vec![],
            body: String::new(),
            elements: vec![],
        };
        let conn = rusqlite::Connection::open(db_path).unwrap();
        init_database_schema(&conn).unwrap();
        write_markdown_data_to_sqlite_with_conn(&data, &conn).unwrap();
        // The above opens a fresh connection; keep `input_dir` in scope so
        // the caller can also operate on it. (No-op here.)
        let _ = input_dir;
    }

    #[test]
    fn test_concept_keys_includes_mapped_source() {
        let mut mapping = crate::commands::mapping::MappingConfig {
            mappings: HashMap::new(),
        };
        mapping
            .mappings
            .insert("projects".to_string(), "project".to_string());
        mapping
            .mappings
            .insert("participants".to_string(), "people".to_string());

        let keys = concept_keys(&mapping, "project", &[]);
        assert!(keys.contains("project"));
        assert!(keys.contains("projects"));
        assert!(!keys.contains("people"));

        // "person" target with extras=["people"]: keys are {person, people}.
        // `participants` maps to `people`, not `person`, so it is NOT added.
        let keys2 = concept_keys(&mapping, "person", &["people"]);
        assert!(keys2.contains("person"));
        assert!(keys2.contains("people"));
        assert!(!keys2.contains("participants"));
    }

    #[test]
    fn test_extract_values_string_and_array() {
        let mut keys = std::collections::HashSet::new();
        keys.insert("project".to_string());
        keys.insert("projects".to_string());

        let v1 = extract_values_from_headers(r#"{"project":"Orchard"}"#, &keys);
        assert_eq!(v1, vec!["Orchard"]);

        let v2 = extract_values_from_headers(r#"{"projects":["A","B"]}"#, &keys);
        assert_eq!(v2, vec!["A", "B"]);

        let v3 = extract_values_from_headers(r#"{"other":"X"}"#, &keys);
        assert!(v3.is_empty());
    }

    #[test]
    fn test_sidebar_end_to_end_with_mapping() -> Result<(), Box<dyn std::error::Error>> {
        // Use a temp mapping config so this test is independent of the
        // user's real `~/.config/note_search/config`.
        let tmp = TempDir::new()?;
        let cfg = tmp.path().join("cfg");
        std::fs::write(&cfg, "[Mapping]\nprojects=project\nparticipants=people\n")?;
        std::env::set_var("NOTE_SEARCH_CONFIG", cfg.to_str().unwrap());

        let tmpdb = TempDir::new()?;
        let db_path = tmpdb.path().join("test.db");
        let input_dir = tmpdb.path();

        // Note A: `projects: [A, B]` -> should surface as project "A","B"
        insert_note(
            &db_path,
            input_dir,
            "a.md",
            &[("projects", serde_json::json!(["A", "B"]))],
        );
        // Note B: `project: "Solo"` and `type: person`, name="Alice"
        insert_note(
            &db_path,
            input_dir,
            "b.md",
            &[
                ("project", serde_json::json!("Solo")),
                ("type", serde_json::json!("person")),
            ],
        );
        // Note C: `participants: [Carol]` (mapped -> people) -> person sidebar
        insert_note(
            &db_path,
            input_dir,
            "c.md",
            &[("participants", serde_json::json!(["Carol"]))],
        );

        let conn = rusqlite::Connection::open(&db_path)?;
        let mapping = crate::commands::mapping::MappingConfig::load();
        let proj_keys = concept_keys(&mapping, "project", &[]);
        let person_keys = concept_keys(
            &mapping,
            "person",
            &["people", "persons", "participant", "participants"],
        );

        let (projects, _p_debug) = collect_attribute_values(&conn, &proj_keys, Some("project"))?;
        let (persons, _pe_debug) = collect_attribute_values(&conn, &person_keys, Some("person"))?;

        // Projects: A, B (from a.md -> projects), Solo (from b.md -> project)
        assert!(projects.contains("A"));
        assert!(projects.contains("B"));
        assert!(projects.contains("Solo"));
        // b.md is type=person, so its title "b.md" must NOT be a project
        // (fallback only matches fallback_type="project").
        assert!(!projects.contains("b.md"));

        // Persons: "b.md" (type=person -> title fallback), Carol (c.md ->
        // participants -> mapped to "people" -> in person_keys via extras).
        assert!(persons.contains("b.md"));
        assert!(persons.contains("Carol"));

        std::env::remove_var("NOTE_SEARCH_CONFIG");
        Ok(())
    }

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