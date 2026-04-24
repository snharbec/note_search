use crate::commands::args::CommonSearchArgs;
use crate::search_criteria::normalize_date;
use std::collections::HashMap;
use std::env;
use std::path::Path;
use std::process;

/// Generate agenda view filtered by due date (no note specified)
/// Shows all open todos with due date <= specified date, grouped by project
fn generate_due_date_agenda(
    conn: &rusqlite::Connection,
    note_dir: &str,
    common: &CommonSearchArgs,
    priority: Option<&String>,
    due_date: Option<&String>,
    open: bool,
    closed: bool,
    type_filter: &str,
    today: &str,
    no_summary: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut output = String::new();

    // Add Agenda header with date
    output.push_str(&format!("# Agenda {}\n\n", today));

    // Get all projects of the specified type
    let mut stmt = conn.prepare(
        "SELECT filename, title FROM markdown_data
         WHERE json_extract(header_fields, '$.type') = ?
         ORDER BY filename",
    )?;

    let project_rows = stmt.query_map([type_filter], |row| {
        Ok((
            row.get::<_, String>(0)?,         // filename
            row.get::<_, Option<String>>(1)?, // title
        ))
    })?;

    let mut projects_map: HashMap<String, String> = HashMap::new(); // project_base -> filename
    for row in project_rows {
        let (filename, _title) = row?;
        let project_base = Path::new(&filename)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| filename.clone());
        projects_map.insert(
            project_base.to_lowercase().replace('_', " "),
            filename.clone(),
        );
        projects_map.insert(project_base.to_lowercase(), filename.clone());
        projects_map.insert(filename.to_lowercase().replace('_', " "), filename.clone());
    }

    // Build the WHERE clause for todos
    let mut where_clauses = vec!["te.closed = 0".to_string()];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    // Handle open/closed filters
    if open {
        where_clauses.push("te.closed = 0".to_string());
    } else if closed {
        where_clauses.push("te.closed = 1".to_string());
    }

    // Handle priority filter
    if let Some(p) = priority {
        where_clauses.push("te.priority = ?".to_string());
        params.push(Box::new(p.to_string()));
    }

    // Handle due date filter - due <= specified date OR no due date
    if let Some(date) = due_date {
        where_clauses.push("(te.due IS NULL OR te.due <= ?)".to_string());
        params.push(Box::new(normalize_date(date)));
    }

    // Handle text search
    if let Some(text) = &common.text {
        where_clauses.push("te.text LIKE ?".to_string());
        params.push(Box::new(format!("%{}%", text)));
    }

    // Handle tags filter
    if let Some(tags_str) = &common.tags {
        let tags: Vec<String> = tags_str.split(',').map(|s| s.trim().to_string()).collect();
        for tag in tags {
            where_clauses.push("te.tags LIKE ?".to_string());
            params.push(Box::new(format!("%\"{}\"%", tag)));
        }
    }

    // Query all matching todos
    let todo_query = format!(
        "SELECT te.text, te.priority, te.due, te.line_number, te.filename, te.links
         FROM todo_entries te
         WHERE {}
         ORDER BY te.due, te.priority",
        where_clauses.join(" AND ")
    );

    let mut todo_stmt = conn.prepare(&todo_query)?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let todos = todo_stmt.query_map(&*param_refs.as_slice(), |row| {
        Ok((
            row.get::<_, String>(0)?,         // text
            row.get::<_, Option<String>>(1)?, // priority
            row.get::<_, Option<String>>(2)?, // due
            row.get::<_, i64>(3)?,            // line_number
            row.get::<_, String>(4)?,         // filename
            row.get::<_, Option<String>>(5)?, // links JSON
        ))
    })?;

    // Collect todos and group by project
    let mut todos_by_project: HashMap<
        String,
        Vec<(String, Option<String>, Option<String>, i64, String)>,
    > = HashMap::new();
    let mut unmatched_todos: Vec<(String, Option<String>, Option<String>, i64, String)> =
        Vec::new();

    for row in todos {
        let (text, priority_val, due, line_number, source_file, links_json) = row?;

        // Try to find which project this todo belongs to
        let mut matched_project: Option<String> = None;

        // Check todo-level links
        if let Some(ref links_json_str) = links_json {
            if let Ok(links) = serde_json::from_str::<Vec<String>>(links_json_str) {
                for link in links {
                    let normalized_link = link.to_lowercase().replace('_', " ");
                    if let Some(project_filename) = projects_map.get(&normalized_link) {
                        let project_base = Path::new(project_filename)
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| project_filename.clone());
                        matched_project = Some(project_base);
                        break;
                    }
                }
            }
        }

        // If no match from todo links, check document-level links
        if matched_project.is_none() {
            let mut doc_stmt =
                conn.prepare("SELECT links FROM markdown_data WHERE filename = ?")?;
            if let Ok(doc_links_json) =
                doc_stmt.query_row([&source_file], |row| row.get::<_, Option<String>>(0))
            {
                if let Some(doc_links) = doc_links_json {
                    if let Ok(links) = serde_json::from_str::<Vec<String>>(&doc_links) {
                        for link in links {
                            let normalized_link = link.to_lowercase().replace('_', " ");
                            if let Some(project_filename) = projects_map.get(&normalized_link) {
                                let project_base = Path::new(project_filename)
                                    .file_stem()
                                    .map(|s| s.to_string_lossy().to_string())
                                    .unwrap_or_else(|| project_filename.clone());
                                matched_project = Some(project_base);
                                break;
                            }
                        }
                    }
                }
            }
        }

        if let Some(project) = matched_project {
            todos_by_project.entry(project).or_default().push((
                text,
                priority_val,
                due,
                line_number,
                source_file,
            ));
        } else {
            unmatched_todos.push((text, priority_val, due, line_number, source_file));
        }
    }

    // Add Summary section (unless --no-summary flag is set)
    if !no_summary {
        output.push_str("## Summary\n\n");

        let total_todos: usize = todos_by_project.values().map(|v| v.len()).sum();
        if total_todos == 0 && unmatched_todos.is_empty() {
            output.push_str("No open todos found matching the criteria.\n\n");
        } else {
            output.push_str(&format!(
                "Found {} open todo(s) due on or before {}.\n\n",
                total_todos + unmatched_todos.len(),
                due_date.map_or("", |d| d.as_str())
            ));
        }
    }

    // Add Projects section
    let type_name = match type_filter {
        "department" => "Departments",
        "person" => "Persons",
        "company" => "Companies",
        _ => "Projects",
    };
    output.push_str(&format!("## {}\n\n", type_name));

    if todos_by_project.is_empty() {
        output.push_str(&format!(
            "No {} with matching todos found.\n\n",
            type_name.to_lowercase()
        ));
    } else {
        // Sort projects by name for consistent output
        let mut sorted_projects: Vec<&String> = todos_by_project.keys().collect();
        sorted_projects.sort();

        for project_base in sorted_projects {
            let project_todos = todos_by_project.get(project_base).unwrap();

            // Add project heading as level 3 with wiki link
            output.push_str(&format!("### [[{}]]\n\n", project_base));

            // Sort todos by due date, then priority
            let mut sorted_todos = project_todos.clone();
            sorted_todos.sort_by(|a, b| {
                let due_cmp = match (&a.2, &b.2) {
                    (Some(due_a), Some(due_b)) => due_a.cmp(due_b),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                };
                if due_cmp != std::cmp::Ordering::Equal {
                    return due_cmp;
                }
                match (&a.1, &b.1) {
                    (Some(pri_a), Some(pri_b)) => pri_a.cmp(pri_b),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                }
            });

            for (text, priority_val, due, line_number, source_file) in sorted_todos {
                let abs_path = Path::new(note_dir)
                    .join(&source_file)
                    .to_string_lossy()
                    .to_string();

                let note_name = Path::new(&source_file)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| source_file.clone());

                let mut todo_line = format!("- [ ] {}", text);

                if let Some(p) = priority_val {
                    todo_line.push_str(&format!(" priority:{}", p));
                }

                if let Some(d) = due {
                    todo_line.push_str(&format!(" due:{}", d));
                }

                todo_line.push_str(&format!(
                    " ([{}](<{}:{} >))",
                    note_name, abs_path, line_number
                ));

                output.push_str(&todo_line);
                output.push('\n');
            }

            output.push('\n');
        }
    }

    // Add unmatched todos section if any exist
    if !unmatched_todos.is_empty() {
        output.push_str("## Unmatched Todos\n\n");
        output.push_str(
            "The following todos match the due date filter but don't link to any project:\n\n",
        );

        for (text, priority_val, due, line_number, source_file) in unmatched_todos {
            let abs_path = Path::new(note_dir)
                .join(&source_file)
                .to_string_lossy()
                .to_string();

            let note_name = Path::new(&source_file)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| source_file.clone());

            let mut todo_line = format!("- [ ] {}", text);

            if let Some(p) = priority_val {
                todo_line.push_str(&format!(" priority:{}", p));
            }

            if let Some(d) = due {
                todo_line.push_str(&format!(" due:{}", d));
            }

            todo_line.push_str(&format!(
                " ([{}](<{}:{} >))",
                note_name, abs_path, line_number
            ));

            output.push_str(&todo_line);
            output.push('\n');
        }
        output.push('\n');
    }

    Ok(output)
}

pub fn handle_agenda(
    database: &str,
    sort: &str,
    common: &CommonSearchArgs,
    priority: Option<&String>,
    due_date: Option<&String>,
    due_date_eq: Option<&String>,
    due_date_gt: Option<&String>,
    open: bool,
    closed: bool,
    note: Option<&String>,
    type_filter: &str,
    no_summary: bool,
) {
    let db_path = Path::new(database);

    if !db_path.exists() {
        eprintln!("Error: Database '{}' does not exist", database);
        process::exit(1);
    }

    match generate_agenda(
        db_path,
        sort,
        common,
        priority,
        due_date,
        due_date_eq,
        due_date_gt,
        open,
        closed,
        note,
        type_filter,
        no_summary,
    ) {
        Ok(output) => {
            if output.is_empty() {
                println!("No projects with open todos found.");
            } else {
                println!("{}", output);
            }
        }
        Err(e) => {
            eprintln!("Error generating agenda: {}", e);
            process::exit(1);
        }
    }
}

pub fn generate_agenda(
    db_path: &Path,
    _sort: &str,
    common: &CommonSearchArgs,
    priority: Option<&String>,
    due_date: Option<&String>,
    due_date_eq: Option<&String>,
    due_date_gt: Option<&String>,
    open: bool,
    closed: bool,
    note: Option<&String>,
    type_filter: &str,
    no_summary: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    use rusqlite::Connection;
    use std::collections::HashSet;

    let conn = Connection::open(db_path)?;
    let note_dir = env::var("NOTE_SEARCH_DIR").unwrap_or_else(|_| ".".to_string());

    // Get current date for the agenda header
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    // Special case: no note specified and due-date is provided
    // Show all open todos with due date <= specified date (across all projects)
    if note.is_none() && due_date.is_some() {
        return generate_due_date_agenda(
            &conn,
            &note_dir,
            common,
            priority,
            due_date,
            open,
            closed,
            type_filter,
            &today,
            no_summary,
        );
    }

    // Handle specific note filter if provided
    let mut target_projects: Option<Vec<String>> = None;
    if let Some(note_name) = note {
        // Check if this note matches the target type
        let mut stmt = conn.prepare(
            "SELECT filename FROM markdown_data 
             WHERE json_extract(header_fields, '$.type') = ?4 
             AND (filename = ?1 OR filename LIKE ?2 OR title = ?3)",
        )?;

        let note_file = format!("{}.md", note_name);
        let note_pattern = format!("%/{0}.md", note_name);

        let project_files: Vec<String> = stmt
            .query_map(
                rusqlite::params![&note_file, &note_pattern, note_name, type_filter],
                |row| row.get::<_, String>(0),
            )?
            .filter_map(Result::ok)
            .collect();

        if !project_files.is_empty() {
            // The note is the target type, use it directly
            target_projects = Some(project_files);
        } else {
            // The note is not the target type, find target types it references
            let mut links_stmt = conn.prepare(
                "SELECT links FROM markdown_data 
                 WHERE filename = ?1 OR filename LIKE ?2",
            )?;

            let note_links: Vec<String> = links_stmt
                .query_map([&note_file, &note_pattern], |row| row.get::<_, String>(0))?
                .filter_map(Result::ok)
                .collect();

            let mut referenced_projects = Vec::new();
            for links_json in note_links {
                if let Ok(links) = serde_json::from_str::<Vec<String>>(&links_json) {
                    for link in links {
                        // Check if this link is the target type
                        let mut check_stmt = conn.prepare(
                            "SELECT filename FROM markdown_data 
                             WHERE json_extract(header_fields, '$.type') = ?3 
                             AND (filename = ?1 OR filename LIKE ?2)",
                        )?;

                        let link_file = format!("{}.md", link);
                        let link_pattern = format!("%/{0}.md", link);

                        let project_file: Option<String> = check_stmt
                            .query_row(
                                rusqlite::params![&link_file, &link_pattern, type_filter],
                                |row| row.get(0),
                            )
                            .ok();

                        if let Some(proj) = project_file {
                            referenced_projects.push(proj);
                        }
                    }
                }
            }

            // When a note is specified but no projects are found, return empty agenda
            target_projects = Some(referenced_projects);
        }
    }

    // If a specific note was requested but no projects were found, return empty agenda
    if let Some(ref targets) = target_projects {
        if targets.is_empty() {
            return Ok(String::new());
        }
    }

    // Get current date for the agenda header
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    // Calculate date 7 days from now for summary filtering
    let today_date = chrono::Local::now().date_naive();
    let seven_days_later = today_date + chrono::Duration::days(7);
    let seven_days_str = seven_days_later.format("%Y%m%d").to_string();

    // Query projects (filtered by target_projects if specified)
    let projects_query = if let Some(ref targets) = target_projects {
        // Build IN clause for target projects
        let placeholders: Vec<String> = targets.iter().map(|_| "?".to_string()).collect();
        let in_clause = placeholders.join(",");
        format!(
            "SELECT md.filename, md.title, md.created, md.header_fields 
             FROM markdown_data md
             WHERE json_extract(md.header_fields, '$.type') = ?
             AND md.filename IN ({})
             ORDER BY md.created DESC",
            in_clause
        )
    } else {
        "SELECT md.filename, md.title, md.created, md.header_fields 
         FROM markdown_data md
         WHERE json_extract(md.header_fields, '$.type') = ?
         ORDER BY md.created DESC"
            .to_string()
    };

    let mut stmt = conn.prepare(&projects_query)?;

    let projects: Vec<Result<(String, Option<String>, i64, Option<String>), rusqlite::Error>> =
        if let Some(ref targets) = target_projects {
            // Combine type_filter with targets
            let mut params: Vec<&dyn rusqlite::ToSql> = Vec::new();
            params.push(&type_filter);
            for target in targets {
                params.push(target);
            }
            stmt.query_map(&*params, |row| {
                Ok((
                    row.get::<_, String>(0)?,         // filename
                    row.get::<_, Option<String>>(1)?, // title
                    row.get::<_, i64>(2)?,            // created
                    row.get::<_, Option<String>>(3)?, // header_fields
                ))
            })?
            .collect()
        } else {
            stmt.query_map([type_filter], |row| {
                Ok((
                    row.get::<_, String>(0)?,         // filename
                    row.get::<_, Option<String>>(1)?, // title
                    row.get::<_, i64>(2)?,            // created
                    row.get::<_, Option<String>>(3)?, // header_fields
                ))
            })?
            .collect()
        };

    // Collect all projects and their todos
    let mut project_data: Vec<(
        String,
        i64,
        String,
        Vec<(String, Option<String>, Option<String>, i64, String)>,
    )> = Vec::new();
    let mut summary_todos: Vec<(String, Option<String>, Option<String>, i64, String, String)> =
        Vec::new();

    for project in projects {
        let (filename, _title, created, _header_fields): (
            String,
            Option<String>,
            i64,
            Option<String>,
        ) = project?;

        // Get project identifier for matching (basename without .md)
        let project_base = Path::new(&filename)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| filename.clone());

        // Normalize project name for matching (lowercase, replace underscores with spaces)
        let normalized_project_file = filename.to_lowercase().replace('_', " ");
        let normalized_project_base = project_base.to_lowercase().replace('_', " ");

        // Find all notes that link to this project
        let mut linking_files = HashSet::new();

        // Search in markdown_data.links (document-level links)
        let mut link_stmt =
            conn.prepare("SELECT filename, links FROM markdown_data WHERE links IS NOT NULL")?;
        let link_rows = link_stmt.query_map([], |row| {
            let fname: String = row.get(0)?;
            let links_json: String = row.get(1)?;
            Ok((fname, links_json))
        })?;

        for row in link_rows {
            let (doc_filename, links_json) = row?;
            if doc_filename == filename {
                continue;
            }
            if let Ok(links_array) = serde_json::from_str::<Vec<String>>(&links_json) {
                for link in links_array {
                    let normalized_link = link.to_lowercase().replace('_', " ");
                    if normalized_link == normalized_project_file
                        || normalized_link == normalized_project_base
                    {
                        linking_files.insert(doc_filename.clone());
                        break;
                    }
                }
            }
        }

        // Also search in todo_entries.links (todo-level links)
        let mut todo_link_stmt = conn
            .prepare("SELECT DISTINCT filename, links FROM todo_entries WHERE links IS NOT NULL")?;
        let todo_link_rows = todo_link_stmt.query_map([], |row| {
            let fname: String = row.get(0)?;
            let links_json: String = row.get(1)?;
            Ok((fname, links_json))
        })?;

        for row in todo_link_rows {
            let (doc_filename, links_json) = row?;
            if doc_filename == filename {
                continue;
            }
            if let Ok(links_array) = serde_json::from_str::<Vec<String>>(&links_json) {
                for link in links_array {
                    let normalized_link = link.to_lowercase().replace('_', " ");
                    if normalized_link == normalized_project_file
                        || normalized_link == normalized_project_base
                    {
                        linking_files.insert(doc_filename);
                        break;
                    }
                }
            }
        }

        if linking_files.is_empty() {
            continue;
        }

        // Build the WHERE clause based on filters
        let mut where_clauses = vec!["te.closed = 0".to_string()];
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        // Handle open/closed filters
        if open {
            where_clauses.push("te.closed = 0".to_string());
        } else if closed {
            where_clauses.push("te.closed = 1".to_string());
        }

        // Handle priority filter
        if let Some(p) = priority {
            where_clauses.push("te.priority = ?".to_string());
            params.push(Box::new(p.to_string()));
        }

        // Handle due date filters (command line args take precedence)
        if let Some(date) = due_date_eq {
            where_clauses.push("te.due = ?".to_string());
            params.push(Box::new(normalize_date(date)));
        } else if let Some(date) = due_date_gt {
            where_clauses.push("te.due >= ?".to_string());
            params.push(Box::new(normalize_date(date)));
        } else if let Some(date) = due_date {
            where_clauses.push("te.due <= ?".to_string());
            params.push(Box::new(normalize_date(date)));
        }

        // Handle text search
        if let Some(text) = &common.text {
            where_clauses.push("te.text LIKE ?".to_string());
            params.push(Box::new(format!("%{}%", text)));
        }

        // Handle tags filter
        if let Some(tags_str) = &common.tags {
            let tags: Vec<String> = tags_str.split(',').map(|s| s.trim().to_string()).collect();
            for tag in tags {
                where_clauses.push("te.tags LIKE ?".to_string());
                params.push(Box::new(format!("%\"{}\"%", tag)));
            }
        }

        // Create placeholders for linking files
        let file_placeholders: Vec<String> =
            linking_files.iter().map(|_| "?".to_string()).collect();
        let files_str = file_placeholders.join(",");

        // Build the complete query
        let todo_query = format!(
            "SELECT te.text, te.priority, te.due, te.line_number, te.filename 
             FROM todo_entries te
             WHERE te.filename IN ({}) AND {}",
            files_str,
            where_clauses.join(" AND "),
        );

        let mut todo_stmt = conn.prepare(&todo_query)?;

        // Add linking files to params
        for file in &linking_files {
            params.push(Box::new(file.clone()));
        }

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let todos = todo_stmt.query_map(&*param_refs.as_slice(), |row| {
            Ok((
                row.get::<_, String>(0)?,         // text
                row.get::<_, Option<String>>(1)?, // priority
                row.get::<_, Option<String>>(2)?, // due
                row.get::<_, i64>(3)?,            // line_number
                row.get::<_, String>(4)?,         // filename
            ))
        })?;

        let all_todos: Vec<_> = todos.filter_map(Result::ok).collect();

        if all_todos.is_empty() {
            continue;
        }

        // Collect todos for this project
        let project_todos: Vec<_> = all_todos.clone();

        // Add to summary if due within 7 days and has a due date
        for (text, priority, due, line_number, source_file) in &all_todos {
            if let Some(due_date_str) = due {
                // Check if due date is <= 7 days from now
                if due_date_str <= &seven_days_str {
                    summary_todos.push((
                        text.clone(),
                        priority.clone(),
                        due.clone(),
                        *line_number,
                        source_file.clone(),
                        project_base.clone(),
                    ));
                }
            }
        }

        project_data.push((project_base, created, filename, project_todos));
    }

    // Build output
    let mut output = String::new();

    // Add Agenda header with date
    output.push_str(&format!("# Agenda {}\n\n", today));

    // Add Summary section (unless --no-summary flag is set)
    if !no_summary {
        output.push_str("## Summary\n\n");

        if summary_todos.is_empty() {
            output.push_str("No todos due within the next 7 days.\n\n");
        } else {
            // Sort summary todos by due date (earliest first)
            summary_todos.sort_by(|a, b| match (&a.2, &b.2) {
                (Some(due_a), Some(due_b)) => due_a.cmp(due_b),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            });

            for (text, priority, due, line_number, source_file, project_name) in summary_todos {
                let abs_path = Path::new(&note_dir)
                    .join(&source_file)
                    .to_string_lossy()
                    .to_string();

                let note_name = Path::new(&source_file)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| source_file.clone());

                let mut todo_line = format!("- [ ] {}", text);

                if let Some(p) = priority {
                    todo_line.push_str(&format!(" priority:{}", p));
                }

                if let Some(d) = due {
                    todo_line.push_str(&format!(" due:{}", d));
                }

                todo_line.push_str(&format!(
                    " ([{}](<{}:{} >)) - Project: {}",
                    note_name, abs_path, line_number, project_name
                ));

                output.push_str(&todo_line);
                output.push('\n');
            }
            output.push('\n');
        }
    }

    // Add Projects section with appropriate type name
    let type_name = match type_filter {
        "department" => "Departments",
        "person" => "Persons",
        "company" => "Companies",
        _ => "Projects",
    };
    output.push_str(&format!("## {}\n\n", type_name));

    if project_data.is_empty() {
        output.push_str(&format!(
            "No {} with open todos found.\n\n",
            type_name.to_lowercase()
        ));
    } else {
        for (project_base, _created, _project_filename, todos) in project_data {
            // Add project heading as level 3 with wiki link
            output.push_str(&format!("### [[{}]]\n\n", project_base));

            // Sort todos by due date, then priority
            let mut sorted_todos = todos;
            sorted_todos.sort_by(|a, b| {
                let due_cmp = match (&a.2, &b.2) {
                    (Some(due_a), Some(due_b)) => due_a.cmp(due_b),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                };
                if due_cmp != std::cmp::Ordering::Equal {
                    return due_cmp;
                }
                match (&a.1, &b.1) {
                    (Some(pri_a), Some(pri_b)) => pri_a.cmp(pri_b),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                }
            });

            for (text, priority, due, line_number, source_file) in sorted_todos {
                let abs_path = Path::new(&note_dir)
                    .join(&source_file)
                    .to_string_lossy()
                    .to_string();

                let note_name = Path::new(&source_file)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| source_file.clone());

                let mut todo_line = format!("- [ ] {}", text);

                if let Some(p) = priority {
                    todo_line.push_str(&format!(" priority:{}", p));
                }

                if let Some(d) = due {
                    todo_line.push_str(&format!(" due:{}", d));
                }

                todo_line.push_str(&format!(
                    " ([{}](<{}:{} >))",
                    note_name, abs_path, line_number
                ));

                output.push_str(&todo_line);
                output.push('\n');
            }

            output.push('\n');
        }
    }

    Ok(output)
}
