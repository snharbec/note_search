use axum::{
    extract::{Query, State},
    response::Html,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use crate::database_service::{DatabaseService, NoteResult, TodoResult};
use crate::search_criteria::SearchCriteria;
use crate::commands::backlinks::get_backlinks;
use std::path::Path;

#[derive(Deserialize)]
struct SearchParams {
    text: Option<String>,
    attributes: Option<String>,
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
    title: String,
    content: String,
    backlinks: Vec<String>,
}

pub async fn start_server(port: u16, database: String, _watch: bool) {
    let note_dir = std::env::var("NOTE_SEARCH_DIR").unwrap_or_else(|_| ".".to_string());
    let db_service = Arc::new(DatabaseService::new(&database));
    
    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/search", get(search_handler))
        .route("/api/projects", get(projects_handler))
        .route("/api/persons", get(persons_handler))
        .route("/api/note", get(move |state, query| note_handler(state, query, note_dir)))
        .with_state(db_service);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("Web server running on http://{}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn index_handler() -> Html<&'static str> {
    Html(include_str!("static/index.html"))
}

async fn search_handler(
    State(db_service): State<Arc<DatabaseService>>,
    Query(params): Query<SearchParams>,
) -> Json<SearchResponse> {
    let mut criteria = SearchCriteria::default();
    criteria.text = params.text.clone();
    criteria.search_body = params.text;
    
    // Simple attribute parsing: assume "key=value"
    if let Some(attr_str) = params.attributes {
        let parts: Vec<&str> = attr_str.split('=').collect();
        if parts.len() == 2 {
            criteria.attributes.push(crate::attribute_pair::AttributePair::new(parts[0], parts[1]));
        }
    }

    let notes = db_service.search_notes(&criteria).unwrap_or_default();
    let todos = db_service.search_todos(&criteria).unwrap_or_default();

    Json(SearchResponse { notes, todos })
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
        title: title.unwrap_or_default(),
        content,
        backlinks,
    })
}

async fn projects_handler(
    State(db_service): State<Arc<DatabaseService>>,
) -> Json<Vec<String>> {
    let conn = db_service.connect().expect("Failed to connect to database");
    
    // Assuming projects are either in a 'project' header field or type='project'
    let mut stmt = conn
        .prepare("SELECT DISTINCT value FROM markdown_data, json_each(header_fields, '$.project')")
        .unwrap_or_else(|_| conn.prepare("SELECT DISTINCT title FROM markdown_data WHERE json_extract(header_fields, '$.type') = 'project'").unwrap());
    
    let rows = stmt.query_map([], |row| row.get(0)).unwrap();
    let projects: Vec<String> = rows.flatten().collect();
    Json(projects)
}

async fn persons_handler(
    State(db_service): State<Arc<DatabaseService>>,
) -> Json<Vec<String>> {
    let conn = db_service.connect().expect("Failed to connect to database");
    
    let mut stmt = conn
        .prepare("SELECT DISTINCT value FROM markdown_data, json_each(header_fields, '$.person')")
        .unwrap_or_else(|_| conn.prepare("SELECT DISTINCT title FROM markdown_data WHERE json_extract(header_fields, '$.type') = 'person'").unwrap());
    
    let rows = stmt.query_map([], |row| row.get(0)).unwrap();
    let persons: Vec<String> = rows.flatten().collect();
    Json(persons)
}
