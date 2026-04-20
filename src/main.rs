use clap::{Parser, Subcommand};
use note_search::attribute_pair::AttributePair;
use note_search::database_service::DatabaseService;
use note_search::markdown_parser;
use note_search::search_criteria::{
    DateComparison, DateRange, DueDateCriteria, SearchCriteria, SortOrder,
};
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::Path;
use std::process;
use std::time::{Duration, SystemTime};

#[derive(Parser)]
#[command(name = "note_search")]
#[command(version = "1.0.0")]
#[command(about = "A tool to search for and import todo entries from markdown files")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Specify the database file to use (overrides NOTE_SEARCH_DATABASE env var)
    #[arg(
        short = 'd',
        long = "database",
        default_value = "./note.sqlite",
        global = true
    )]
    database: String,
}

/// Common search arguments for both todos and notes
#[derive(Parser)]
struct CommonSearchArgs {
    /// Search with specified tags (all must match)
    #[arg(long = "tags")]
    tags: Option<String>,

    /// Search with specified links (all must match)
    #[arg(long = "links")]
    links: Option<String>,

    /// Search with specific attribute values in the header fields
    #[arg(long = "attributes")]
    attributes: Option<String>,

    /// Search containing the specified text
    #[arg(long = "text")]
    text: Option<String>,

    /// Search for text in the note body (case-insensitive)
    #[arg(long = "search-body")]
    search_body: Option<String>,

    /// Search in a date range (today, yesterday, this_week, last_week, this_month, last_month, this_year, last_year)
    #[arg(long = "date-range")]
    date_range: Option<String>,

    /// Search on or after this date (YYYYMMDD)
    #[arg(long = "start-date")]
    start_date: Option<String>,

    /// Search on or before this date (YYYYMMDD)
    #[arg(long = "end-date")]
    end_date: Option<String>,

    /// Configure output format string
    #[arg(long = "format")]
    format: Option<String>,

    /// Sort results by field (filename, modified, attr:ATTRIBUTE, text)
    #[arg(long = "sort")]
    sort: Option<String>,

    /// List only file locations without text
    #[arg(long = "list")]
    list: bool,

    /// Output absolute paths instead of relative paths
    #[arg(long = "absolute-path")]
    absolute_path: bool,
}

/// Todo-specific search arguments (extends CommonSearchArgs)
#[derive(Parser)]
struct TodoSearchArgs {
    #[command(flatten)]
    common: CommonSearchArgs,

    /// Search for todos with the specified priority
    #[arg(long = "priority")]
    priority: Option<String>,

    /// Search for todos due on or before the specified date (YYYYMMDD)
    #[arg(long = "due-date")]
    due_date: Option<String>,

    /// Search for todos due on the specified date (YYYYMMDD)
    #[arg(long = "due-date-eq")]
    due_date_eq: Option<String>,

    /// Search for todos due on or after the specified date (YYYYMMDD)
    #[arg(long = "due-date-gt")]
    due_date_gt: Option<String>,

    /// Search for open todos only
    #[arg(long = "open")]
    open: bool,

    /// Search for closed todos only
    #[arg(long = "closed")]
    closed: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Search for todo entries in the database
    Todos(TodoSearchArgs),

    /// Search for notes (documents) in the database
    Notes(CommonSearchArgs),

    /// Import markdown files into the database
    Import {
        /// Input directory containing markdown files (overrides NOTE_SEARCH_DIR env var)
        #[arg(short, long)]
        input: Option<String>,

        /// Output database path (defaults to the database specified with -d)
        #[arg(short, long)]
        output: Option<String>,

        /// Watch mode: continuously monitor directory for changes
        #[arg(long = "watch")]
        watch: bool,

        /// Watch interval in seconds (default: 60)
        #[arg(long = "interval", default_value = "60")]
        interval: u64,
    },

    /// Clear all data from the database
    Clear {
        /// Confirm clearing without interactive prompt
        #[arg(long = "yes")]
        yes: bool,
    },

    /// List unique values for a specific field from the database
    Values {
        /// Field to list values for (priority, due_date, link, tag, attr:ATTRIBUTE)
        #[arg(value_name = "FIELD")]
        field: String,
    },

    /// List all known attribute names from the database
    Attributes,

    /// List documents that link to a given markdown file
    Backlinks {
        /// Filename to find backlinks for
        #[arg(value_name = "FILENAME")]
        filename: String,
    },

    /// List all note names (filenames without path and extension)
    ListNames,

    /// Generate an agenda view of projects and their open todos
    Agenda {
        #[command(flatten)]
        common: CommonSearchArgs,

        /// Search for todos with the specified priority
        #[arg(long = "priority")]
        priority: Option<String>,

        /// Search for todos due on or before the specified date (YYYYMMDD)
        #[arg(long = "due-date")]
        due_date: Option<String>,

        /// Search for todos due on the specified date (YYYYMMDD)
        #[arg(long = "due-date-eq")]
        due_date_eq: Option<String>,

        /// Search for todos due on or after the specified date (YYYYMMDD)
        #[arg(long = "due-date-gt")]
        due_date_gt: Option<String>,

        /// Search for open todos only
        #[arg(long = "open")]
        open: bool,

        /// Search for closed todos only
        #[arg(long = "closed")]
        closed: bool,

        /// Specific note to generate agenda for (if project, shows only that project; if references projects, shows those projects)
        #[arg(value_name = "NOTE")]
        note: Option<String>,

        /// Show todos related to projects (type = project) [default]
        #[arg(short = 'P', long = "projects")]
        projects: bool,

        /// Show todos related to departments (type = department)
        #[arg(short = 'D', long = "departments")]
        departments: bool,

        /// Show todos related to persons (type = person)
        #[arg(short = 'E', long = "persons")]
        persons: bool,

        /// Show todos related to companies (type = company)
        #[arg(short = 'C', long = "companies")]
        companies: bool,
    },

    /// Convert a web page or document to a markdown note
    Convert {
        /// URL or file path to convert
        #[arg(value_name = "SOURCE")]
        source: String,

        /// Output directory (defaults to NOTE_SEARCH_DIR)
        #[arg(short = 'o', long = "output")]
        output: Option<String>,
    },

    /// Link project and person names in notes to their wiki links
    Linker {
        /// Subdirectory within the note root directory to process
        #[arg(value_name = "SUBDIR")]
        subdir: String,
    },

    /// Import JIRA issues as markdown notes
    Jira {
        /// JQL query to filter issues (defaults to issues assigned to current user)
        #[arg(value_name = "JQL")]
        jql: Option<String>,

        /// Output directory (defaults to NOTE_SEARCH_DIR)
        #[arg(short = 'o', long = "output")]
        output: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();

    // Get database path from CLI or environment variable
    let database = if cli.database != "./note.sqlite" {
        // CLI argument explicitly provided
        cli.database.clone()
    } else {
        // Check environment variable, fallback to default
        env::var("NOTE_SEARCH_DATABASE").unwrap_or_else(|_| cli.database.clone())
    };

    match &cli.command {
        Commands::Todos(args) => {
            handle_todos_search(args, &database);
        }
        Commands::Notes(args) => {
            handle_notes_search(args, &database);
        }
        Commands::Import {
            input,
            output,
            watch,
            interval,
        } => {
            // Get input directory from CLI or environment variable
            let input_dir = match input {
                Some(dir) => dir.clone(),
                None => match env::var("NOTE_SEARCH_DIR") {
                    Ok(dir) => dir,
                    Err(_) => {
                        eprintln!("Error: No input directory specified.");
                        eprintln!("Use --input <DIR> or set NOTE_SEARCH_DIR environment variable.");
                        process::exit(1);
                    }
                },
            };

            if *watch {
                handle_watch_import(&database, &input_dir, output.as_deref(), *interval);
            } else {
                handle_import(&database, &input_dir, output.as_deref());
            }
        }
        Commands::Clear { yes } => {
            handle_clear(&database, *yes);
        }
        Commands::Values { field } => {
            handle_values(&database, field);
        }
        Commands::Attributes => {
            handle_attributes(&database);
        }
        Commands::Backlinks { filename } => {
            handle_backlinks(&database, filename);
        }
        Commands::ListNames => {
            handle_list_names(&database);
        }
        Commands::Agenda {
            common,
            priority,
            due_date,
            due_date_eq,
            due_date_gt,
            open,
            closed,
            note,
            projects,
            departments,
            persons,
            companies,
        } => {
            // Determine the type filter based on flags
            let type_filter = if *projects {
                "project"
            } else if *departments {
                "department"
            } else if *persons {
                "person"
            } else if *companies {
                "company"
            } else {
                // Default to project if no flag specified
                "project"
            };

            let sort = common.sort.as_deref().unwrap_or("due");
            handle_agenda(
                &database,
                sort,
                common,
                priority.as_ref(),
                due_date.as_ref(),
                due_date_eq.as_ref(),
                due_date_gt.as_ref(),
                *open,
                *closed,
                note.as_ref(),
                type_filter,
            );
        }
        Commands::Convert { source, output } => {
            // Get output directory from CLI or environment variable
            let output_dir = match output {
                Some(dir) => dir.clone(),
                None => match env::var("NOTE_SEARCH_DIR") {
                    Ok(dir) => dir,
                    Err(_) => {
                        eprintln!("Error: No output directory specified.");
                        eprintln!(
                            "Use --output <DIR> or set NOTE_SEARCH_DIR environment variable."
                        );
                        process::exit(1);
                    }
                },
            };

            handle_convert(source, &output_dir);
        }
        Commands::Linker { subdir } => {
            handle_linker(&database, subdir);
        }
        Commands::Jira { jql, output } => {
            let output_dir = match output {
                Some(dir) => dir.clone(),
                None => match env::var("NOTE_SEARCH_DIR") {
                    Ok(dir) => dir,
                    Err(_) => {
                        eprintln!("Error: No output directory specified.");
                        eprintln!(
                            "Use --output <DIR> or set NOTE_SEARCH_DIR environment variable."
                        );
                        process::exit(1);
                    }
                },
            };

            let jql_query = jql.as_deref().unwrap_or("assignee = currentUser()");
            handle_jira_import(jql_query, &output_dir);
        }
    }
}

fn handle_import(default_db: &str, input: &str, output: Option<&str>) {
    let db_path = output.unwrap_or(default_db);
    let input_path = Path::new(input);

    if !input_path.exists() {
        eprintln!("Error: Input directory '{}' does not exist", input);
        process::exit(1);
    }

    if !input_path.is_dir() {
        eprintln!("Error: Input path '{}' is not a directory", input);
        process::exit(1);
    }

    println!(
        "Importing markdown files from '{}' to database '{}'...",
        input, db_path
    );

    // Track file modification times to detect changes
    let mut file_mtimes: std::collections::HashMap<std::path::PathBuf, SystemTime> =
        std::collections::HashMap::new();

    match do_import_with_tracking(input_path, Path::new(db_path), &mut file_mtimes) {
        Ok(count) => {
            println!(
                "Successfully imported {} markdown files to database '{}'",
                count, db_path
            );
        }
        Err(e) => {
            eprintln!("Error importing markdown files: {}", e);
            process::exit(1);
        }
    }
}

fn handle_watch_import(default_db: &str, input: &str, output: Option<&str>, interval: u64) {
    let db_path = output.unwrap_or(default_db);
    let input_path = Path::new(input);

    if !input_path.exists() {
        eprintln!("Error: Input directory '{}' does not exist", input);
        process::exit(1);
    }

    if !input_path.is_dir() {
        eprintln!("Error: Input path '{}' is not a directory", input);
        process::exit(1);
    }

    let interval_duration = Duration::from_secs(interval);
    let mut file_mtimes: std::collections::HashMap<std::path::PathBuf, SystemTime> =
        std::collections::HashMap::new();

    println!(
        "Starting watch mode for directory '{}' (checking every {} seconds)",
        input, interval
    );
    println!("Press Ctrl+C to stop watching...\n");

    // Do initial import - imports all files and replaces existing content
    match do_import_with_tracking(input_path, Path::new(db_path), &mut file_mtimes) {
        Ok(count) => {
            if count > 0 {
                println!("Initial import: {} files imported/updated", count);
            }
        }
        Err(e) => {
            eprintln!("Error during initial import: {}", e);
        }
    }

    // Watch loop
    loop {
        std::thread::sleep(interval_duration);

        match do_import_with_tracking(input_path, Path::new(db_path), &mut file_mtimes) {
            Ok(count) => {
                if count > 0 {
                    println!(
                        "Watch update: {} files imported/updated at {}",
                        count,
                        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
                    );
                }
            }
            Err(e) => {
                eprintln!("Error during watch import: {}", e);
            }
        }
    }
}

/// Imports markdown files and tracks modification times
/// Returns count of files that were newly imported or modified
fn do_import_with_tracking(
    input_dir: &Path,
    db_path: &Path,
    file_mtimes: &mut std::collections::HashMap<std::path::PathBuf, SystemTime>,
) -> Result<usize, Box<dyn std::error::Error>> {
    use rusqlite::Connection;

    let mut conn = Connection::open(db_path)?;
    markdown_parser::init_database_schema(&conn)?;

    let mut files_to_import: Vec<(std::path::PathBuf, SystemTime)> = Vec::new();

    for entry in walkdir::WalkDir::new(input_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file() && e.path().extension().map_or(false, |ext| ext == "md")
        })
    {
        let path = entry.path().to_path_buf();
        let current_mtime = fs::metadata(&path)?.modified()?;

        let should_import = match file_mtimes.get(&path) {
            Some(&last_mtime) => current_mtime != last_mtime,
            None => true,
        };

        if should_import {
            files_to_import.push((path, current_mtime));
        }
    }

    if files_to_import.is_empty() {
        return Ok(0);
    }

    let tx = conn.transaction()?;
    let mut updated_count = 0;

    for (path, current_mtime) in files_to_import {
        let data = markdown_parser::process_markdown_file(&path, input_dir)?;
        markdown_parser::write_markdown_data_to_sqlite_with_conn(&data, &tx)?;
        file_mtimes.insert(path, current_mtime);
        updated_count += 1;
    }

    tx.commit()?;

    // Remove notes that no longer exist on the filesystem
    let removed = markdown_parser::remove_orphaned_notes(input_dir, &conn)?;
    if removed > 0 {
        println!("Removed {} orphaned notes from database", removed);
    }

    Ok(updated_count)
}

fn handle_clear(database: &str, yes: bool) {
    let db_path = Path::new(database);

    if !db_path.exists() {
        eprintln!("Error: Database '{}' does not exist", database);
        process::exit(1);
    }

    // Confirm unless --yes flag is provided
    if !yes {
        print!(
            "Are you sure you want to clear all data from '{}'? [y/N]: ",
            database
        );
        use std::io::{self, Write};
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        let input = input.trim().to_lowercase();
        if input != "y" && input != "yes" {
            println!("Clear operation cancelled.");
            return;
        }
    }

    match clear_database(db_path) {
        Ok(()) => {
            println!("Successfully cleared all data from database '{}'", database);
        }
        Err(e) => {
            eprintln!("Error clearing database: {}", e);
            process::exit(1);
        }
    }
}

fn clear_database(db_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use rusqlite::Connection;

    let conn = Connection::open(db_path)?;

    // Delete all data from both tables
    conn.execute("DELETE FROM todo_entries", [])?;
    conn.execute("DELETE FROM markdown_data", [])?;

    // Vacuum to reclaim space
    conn.execute("VACUUM", [])?;

    Ok(())
}

fn handle_values(database: &str, field: &str) {
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

fn get_unique_values(
    db_path: &Path,
    field: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    use rusqlite::Connection;
    use std::collections::HashSet;

    let conn = Connection::open(db_path)?;
    let mut values = HashSet::new();

    let field_lower = field.trim().to_lowercase();

    match field_lower.as_str() {
        "priority" => {
            let mut stmt = conn
                .prepare("SELECT DISTINCT priority FROM todo_entries WHERE priority IS NOT NULL")?;
            let rows = stmt.query_map([], |row| Ok(row.get::<_, String>(0)?))?;
            for row in rows {
                values.insert(row?);
            }
        }
        "due_date" | "duedate" | "due" => {
            let mut stmt =
                conn.prepare("SELECT DISTINCT due FROM todo_entries WHERE due IS NOT NULL")?;
            let rows = stmt.query_map([], |row| Ok(row.get::<_, String>(0)?))?;
            for row in rows {
                values.insert(row?);
            }
        }
        "tag" | "tags" => {
            // Get tags from todo_entries
            let mut stmt =
                conn.prepare("SELECT DISTINCT tags FROM todo_entries WHERE tags IS NOT NULL")?;
            let rows = stmt.query_map([], |row| Ok(row.get::<_, String>(0)?))?;
            for row in rows {
                let tags_json: String = row?;
                // Parse JSON array of tags
                if let Ok(tags_array) = serde_json::from_str::<Vec<String>>(&tags_json) {
                    for tag in tags_array {
                        values.insert(tag);
                    }
                }
            }

            // Also get tags from markdown_data table (aggregated from todos)
            let mut stmt =
                conn.prepare("SELECT DISTINCT tags FROM markdown_data WHERE tags IS NOT NULL")?;
            let rows = stmt.query_map([], |row| Ok(row.get::<_, String>(0)?))?;
            for row in rows {
                let tags_json: String = row?;
                // Parse JSON array of tags
                if let Ok(tags_array) = serde_json::from_str::<Vec<String>>(&tags_json) {
                    for tag in tags_array {
                        values.insert(tag);
                    }
                }
            }
        }
        "link" | "links" => {
            // Get links from todo_entries
            let mut stmt =
                conn.prepare("SELECT DISTINCT links FROM todo_entries WHERE links IS NOT NULL")?;
            let rows = stmt.query_map([], |row| Ok(row.get::<_, String>(0)?))?;
            for row in rows {
                let links_json: String = row?;
                if let Ok(links_array) = serde_json::from_str::<Vec<String>>(&links_json) {
                    for link in links_array {
                        values.insert(link);
                    }
                }
            }

            // Also get links from markdown_data table
            let mut stmt =
                conn.prepare("SELECT DISTINCT links FROM markdown_data WHERE links IS NOT NULL")?;
            let rows = stmt.query_map([], |row| Ok(row.get::<_, String>(0)?))?;
            for row in rows {
                let links_json: String = row?;
                if let Ok(links_array) = serde_json::from_str::<Vec<String>>(&links_json) {
                    for link in links_array {
                        values.insert(link);
                    }
                }
            }
        }
        _ if field_lower.starts_with("attr:") => {
            let attr_name = field_lower[5..].trim().to_string();
            if !attr_name.is_empty() {
                let mut stmt = conn.prepare("SELECT DISTINCT header_fields FROM markdown_data WHERE header_fields IS NOT NULL")?;
                let rows = stmt.query_map([], |row| Ok(row.get::<_, String>(0)?))?;
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

fn handle_attributes(database: &str) {
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

fn get_all_attributes(db_path: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    use rusqlite::Connection;
    use std::collections::HashSet;

    let conn = Connection::open(db_path)?;
    let mut attributes = HashSet::new();

    let mut stmt = conn.prepare(
        "SELECT DISTINCT header_fields FROM markdown_data WHERE header_fields IS NOT NULL",
    )?;

    let rows = stmt.query_map([], |row| Ok(row.get::<_, String>(0)?))?;

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

fn handle_backlinks(database: &str, filename: &str) {
    let db_path = Path::new(database);

    if !db_path.exists() {
        eprintln!("Error: Database '{}' does not exist", database);
        process::exit(1);
    }

    match get_backlinks(db_path, filename) {
        Ok(backlinks) => {
            for doc in backlinks {
                println!("{}", doc);
            }
        }
        Err(e) => {
            eprintln!("Error getting backlinks: {}", e);
            process::exit(1);
        }
    }
}

fn handle_list_names(database: &str) {
    let db_path = Path::new(database);

    if !db_path.exists() {
        eprintln!("Error: Database '{}' does not exist", database);
        process::exit(1);
    }

    match get_note_names(db_path) {
        Ok(names) => {
            if names.is_empty() {
                println!("No notes found in database.");
            } else {
                for name in names {
                    println!("{}", name);
                }
            }
        }
        Err(e) => {
            eprintln!("Error getting note names: {}", e);
            process::exit(1);
        }
    }
}

fn handle_agenda(
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

fn generate_agenda(
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
) -> Result<String, Box<dyn std::error::Error>> {
    use rusqlite::Connection;
    use std::collections::HashSet;

    let conn = Connection::open(db_path)?;
    let note_dir = env::var("NOTE_SEARCH_DIR").unwrap_or_else(|_| ".".to_string());

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
        let note_pattern = format!("%/{}.md", note_name);

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
                        let link_pattern = format!("%/{}.md", link);

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
            params.push(Box::new(date.to_string()));
        } else if let Some(date) = due_date_gt {
            where_clauses.push("te.due >= ?".to_string());
            params.push(Box::new(date.to_string()));
        } else if let Some(date) = due_date {
            where_clauses.push("te.due <= ?".to_string());
            params.push(Box::new(date.to_string()));
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

    // Add Summary section
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
                " ([{}](<{}:{}>)) - Project: {}",
                note_name, abs_path, line_number, project_name
            ));

            output.push_str(&todo_line);
            output.push('\n');
        }
        output.push('\n');
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
                    " ([{}](<{}:{}>))",
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

fn get_backlinks(
    db_path: &Path,
    target_filename: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    use rusqlite::Connection;
    use std::collections::HashSet;

    let conn = Connection::open(db_path)?;
    let mut backlinks = HashSet::new();

    let target_base = Path::new(target_filename)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| target_filename.to_string());

    // Normalized forms for matching: lowercase, underscores→spaces
    let normalized_target_file = target_filename.to_lowercase().replace('_', " ");
    let normalized_target_base = target_base.to_lowercase().replace('_', " ");

    // Fuzzy match threshold (0.0-1.0, higher = stricter)
    // Using 0.98 to require near-exact matches and avoid false positives
    // with names that share common prefixes like "Jürgen_"
    const FUZZY_THRESHOLD: f64 = 0.98;

    fn is_match(link: &str, normalized_target_file: &str, normalized_target_base: &str) -> bool {
        let normalized_link = link.to_lowercase().replace('_', " ");

        // Exact match
        if normalized_link == *normalized_target_file || normalized_link == *normalized_target_base
        {
            return true;
        }

        // Also try matching against the raw link (without lowering, for Unicode chars)
        let space_link = link.replace('_', " ");
        let space_target_file = normalized_target_file.replace('_', " ");
        let space_target_base = normalized_target_base.replace('_', " ");
        if space_link == space_target_file || space_link == space_target_base {
            return true;
        }

        // Fuzzy match using Jaro-Winkler similarity
        let score = strsim::jaro_winkler(&normalized_link, normalized_target_file);
        if score >= FUZZY_THRESHOLD {
            return true;
        }
        let score_base = strsim::jaro_winkler(&normalized_link, normalized_target_base);
        score_base >= FUZZY_THRESHOLD
    }

    // Search in markdown_data.links (document-level links)
    let mut stmt =
        conn.prepare("SELECT filename, links FROM markdown_data WHERE links IS NOT NULL")?;
    let rows = stmt.query_map([], |row| {
        let filename: String = row.get(0)?;
        let links_json: String = row.get(1)?;
        Ok((filename, links_json))
    })?;

    for row in rows {
        let (doc_filename, links_json) = row?;
        if let Ok(links_array) = serde_json::from_str::<Vec<String>>(&links_json) {
            for link in links_array {
                if is_match(&link, &normalized_target_file, &normalized_target_base) {
                    backlinks.insert(doc_filename.clone());
                    break;
                }
            }
        }
    }

    // Also search in todo_entries.links (todo-level links)
    let mut stmt =
        conn.prepare("SELECT DISTINCT filename, links FROM todo_entries WHERE links IS NOT NULL")?;
    let rows = stmt.query_map([], |row| {
        let filename: String = row.get(0)?;
        let links_json: String = row.get(1)?;
        Ok((filename, links_json))
    })?;

    for row in rows {
        let (doc_filename, links_json) = row?;
        if let Ok(links_array) = serde_json::from_str::<Vec<String>>(&links_json) {
            for link in links_array {
                if is_match(&link, &normalized_target_file, &normalized_target_base) {
                    backlinks.insert(doc_filename);
                    break;
                }
            }
        }
    }

    // Fetch updated timestamps and sort by modified time (newest first)
    let mut result: Vec<(i64, String)> = Vec::new();
    if !backlinks.is_empty() {
        let placeholders: Vec<String> = backlinks.iter().map(|_| "?".to_string()).collect();
        let query = format!(
            "SELECT filename, updated FROM markdown_data WHERE filename IN ({})",
            placeholders.join(",")
        );
        let mut stmt = conn.prepare(&query)?;
        let params: Vec<&dyn rusqlite::ToSql> = backlinks
            .iter()
            .map(|s| s as &dyn rusqlite::ToSql)
            .collect();
        let rows = stmt.query_map(params.as_slice(), |row| {
            let filename: String = row.get(0)?;
            let updated: i64 = row.get(1)?;
            Ok((updated, filename))
        })?;
        for row in rows {
            result.push(row?);
        }
    }

    result.sort_by(|a, b| b.0.cmp(&a.0));

    Ok(result.into_iter().map(|(_, f)| f).collect())
}

fn get_note_names(db_path: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    use rusqlite::Connection;
    use std::collections::HashSet;

    let conn = Connection::open(db_path)?;
    let mut names = HashSet::new();

    // Get unique filenames from markdown_data table, extract basename without path and .md extension
    let mut stmt =
        conn.prepare("SELECT DISTINCT filename FROM markdown_data WHERE filename IS NOT NULL")?;

    let rows = stmt.query_map([], |row| Ok(row.get::<_, String>(0)?))?;

    for row in rows {
        let filename = row?;
        // Extract basename without path
        let base = Path::new(&filename)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| filename.clone());
        // Remove .md extension if present
        let name = base.trim_end_matches(".md").to_string();
        names.insert(name);
    }

    // Convert HashSet to sorted Vec
    let mut result: Vec<String> = names.into_iter().collect();
    result.sort();
    Ok(result)
}

fn handle_todos_search(args: &TodoSearchArgs, database: &str) {
    let mut criteria = build_todo_criteria(args, database);

    // Set base path for absolute path resolution using NOTE_SEARCH_DIR
    if criteria.absolute_path {
        let note_dir = env::var("NOTE_SEARCH_DIR").unwrap_or_else(|_| ".".to_string());
        criteria.note_dir = note_dir.clone();
        criteria.base_path = note_dir;
    }

    let database_service = DatabaseService::new(&criteria.database_path);

    match database_service.search_todos(&criteria) {
        Ok(results) => {
            if results.is_empty() {
                println!("No matching todos found.");
            } else if criteria.list_only {
                // When listing, show each file only once
                let mut seen_files: HashSet<String> = HashSet::new();
                for result in results {
                    if seen_files.insert(result.filename.clone()) {
                        println!(
                            "{}",
                            result.formatted_string(
                                &criteria.output_format,
                                criteria.absolute_path,
                                &criteria.base_path
                            )
                        );
                    }
                }
            } else {
                for result in results {
                    println!(
                        "{}",
                        result.formatted_string(
                            &criteria.output_format,
                            criteria.absolute_path,
                            &criteria.base_path
                        )
                    );
                }
            }
        }
        Err(e) => {
            eprintln!("Database error: {}", e);
            process::exit(1);
        }
    }
}

fn handle_notes_search(args: &CommonSearchArgs, database: &str) {
    let mut criteria = build_note_criteria(args, database);

    // Set base path for absolute path resolution using NOTE_SEARCH_DIR
    if criteria.absolute_path {
        let note_dir = env::var("NOTE_SEARCH_DIR").unwrap_or_else(|_| ".".to_string());
        criteria.note_dir = note_dir.clone();
        criteria.base_path = note_dir;
    }

    let database_service = DatabaseService::new(&criteria.database_path);

    match database_service.search_notes(&criteria) {
        Ok(results) => {
            if results.is_empty() {
                println!("No matching notes found.");
            } else if criteria.list_only {
                // When listing, show each file only once
                let mut seen_files: HashSet<String> = HashSet::new();
                for result in results {
                    if seen_files.insert(result.filename.clone()) {
                        println!(
                            "{}",
                            result.formatted_string(
                                &criteria.output_format,
                                criteria.absolute_path,
                                &criteria.base_path
                            )
                        );
                    }
                }
            } else {
                for result in results {
                    println!(
                        "{}",
                        result.formatted_string(
                            &criteria.output_format,
                            criteria.absolute_path,
                            &criteria.base_path
                        )
                    );
                }
            }
        }
        Err(e) => {
            eprintln!("Database error: {}", e);
            process::exit(1);
        }
    }
}

fn build_todo_criteria(args: &TodoSearchArgs, database: &str) -> SearchCriteria {
    let mut criteria = SearchCriteria {
        database_path: database.to_string(),
        output_format: args.common.format.clone(),
        list_only: args.common.list,
        absolute_path: args.common.absolute_path,
        ..Default::default()
    };

    if let Some(tags_str) = &args.common.tags {
        criteria.tags = parse_comma_separated(tags_str);
    }

    if let Some(links_str) = &args.common.links {
        criteria.links = parse_comma_separated(links_str);
    }

    if let Some(attrs_str) = &args.common.attributes {
        criteria.attributes = parse_key_value_pairs(attrs_str);
    }

    criteria.text = args.common.text.clone();
    criteria.priority = args.priority.clone();

    // Handle due date options
    if let Some(date) = &args.due_date_eq {
        criteria.due_date = Some(DueDateCriteria {
            date: date.clone(),
            comparison: DateComparison::Equal,
        });
    } else if let Some(date) = &args.due_date_gt {
        criteria.due_date = Some(DueDateCriteria {
            date: date.clone(),
            comparison: DateComparison::GreaterThan,
        });
    } else if let Some(date) = &args.due_date {
        criteria.due_date = Some(DueDateCriteria {
            date: date.clone(),
            comparison: DateComparison::LessThan,
        });
    }

    // Handle date range options
    if let Some(date_range_str) = &args.common.date_range {
        if let Some(date_range) = DateRange::parse(date_range_str) {
            criteria.date_range = Some(date_range);
        } else {
            eprintln!(
                "Warning: Invalid date range '{}'. Expected: today, yesterday, this_week, last_week, this_month, last_month, this_year, last_year",
                date_range_str
            );
        }
    }

    // Handle custom start/end dates
    criteria.created_start = args.common.start_date.clone();
    criteria.created_end = args.common.end_date.clone();

    // Handle body search
    criteria.search_body = args.common.search_body.clone();

    if args.open {
        criteria.open = Some(true);
    } else if args.closed {
        criteria.open = Some(false);
    }

    // Handle sort order - todos can sort by due_date and priority
    if let Some(sort_str) = &args.common.sort {
        criteria.sort_order = parse_todo_sort_order(sort_str);
    }

    criteria
}

fn build_note_criteria(args: &CommonSearchArgs, database: &str) -> SearchCriteria {
    let mut criteria = SearchCriteria {
        database_path: database.to_string(),
        output_format: args.format.clone(),
        list_only: args.list,
        absolute_path: args.absolute_path,
        ..Default::default()
    };

    if let Some(tags_str) = &args.tags {
        criteria.tags = parse_comma_separated(tags_str);
    }

    if let Some(links_str) = &args.links {
        criteria.links = parse_comma_separated(links_str);
    }

    if let Some(attrs_str) = &args.attributes {
        criteria.attributes = parse_key_value_pairs(attrs_str);
    }

    criteria.text = args.text.clone();

    // Handle date range options
    if let Some(date_range_str) = &args.date_range {
        if let Some(date_range) = DateRange::parse(date_range_str) {
            criteria.date_range = Some(date_range);
        } else {
            eprintln!(
                "Warning: Invalid date range '{}'. Expected: today, yesterday, this_week, last_week, this_month, last_month, this_year, last_year",
                date_range_str
            );
        }
    }

    // Handle custom start/end dates
    criteria.created_start = args.start_date.clone();
    criteria.created_end = args.end_date.clone();

    // Handle body search
    criteria.search_body = args.search_body.clone();

    // Handle sort order - notes cannot sort by due_date or priority
    if let Some(sort_str) = &args.sort {
        criteria.sort_order = parse_note_sort_order(sort_str);
    }

    criteria
}

fn parse_todo_sort_order(input: &str) -> Option<SortOrder> {
    let input = input.trim().to_lowercase();

    if input.starts_with("attr:") {
        let attr_name = input[5..].trim().to_string();
        if !attr_name.is_empty() {
            return Some(SortOrder::Attr(attr_name));
        }
    }

    match input.as_str() {
        "due_date" => Some(SortOrder::DueDate),
        "priority" => Some(SortOrder::Priority),
        "filename" => Some(SortOrder::Filename),
        "modified" => Some(SortOrder::Modified),
        "text" => Some(SortOrder::Text),
        _ => {
            eprintln!("Warning: Unknown sort order '{}'. Using default.", input);
            None
        }
    }
}

fn parse_note_sort_order(input: &str) -> Option<SortOrder> {
    let input = input.trim().to_lowercase();

    if input.starts_with("attr:") {
        let attr_name = input[5..].trim().to_string();
        if !attr_name.is_empty() {
            return Some(SortOrder::Attr(attr_name));
        }
    }

    match input.as_str() {
        "filename" => Some(SortOrder::Filename),
        "modified" => Some(SortOrder::Modified),
        "created" => Some(SortOrder::Created),
        "text" => Some(SortOrder::Text),
        "due_date" | "priority" => {
            eprintln!("Warning: Cannot sort notes by '{}'. Notes don't have due dates or priorities. Use 'filename', 'modified', 'created', or 'attr:ATTRIBUTE' instead.", input);
            None
        }
        _ => {
            eprintln!("Warning: Unknown sort order '{}'. Using default.", input);
            None
        }
    }
}

fn parse_comma_separated(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn parse_key_value_pairs(input: &str) -> Vec<AttributePair> {
    input
        .split(',')
        .filter_map(|pair| {
            let parts: Vec<&str> = pair.splitn(2, '=').collect();
            if parts.len() == 2 {
                let key = parts[0].trim().to_string();
                let value = parts[1].trim().to_string();
                if !key.is_empty() && !value.is_empty() {
                    return Some(AttributePair::new(&key, &value));
                }
            }
            None
        })
        .collect()
}

fn handle_linker(database: &str, subdir: &str) {
    use rusqlite::Connection;
    use walkdir::WalkDir;

    let note_dir = env::var("NOTE_SEARCH_DIR").unwrap_or_else(|_| ".".to_string());
    let target_dir = Path::new(&note_dir).join(subdir);

    if !target_dir.exists() {
        eprintln!("Error: Directory '{}' does not exist", target_dir.display());
        process::exit(1);
    }

    // Query database for project and person names
    let db_path = Path::new(database);
    if !db_path.exists() {
        eprintln!(
            "Error: Database '{}' not found. Run import first.",
            database
        );
        process::exit(1);
    }

    let conn = match Connection::open(db_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error opening database: {}", e);
            process::exit(1);
        }
    };

    // Get note names for projects and persons
    let entity_names = match get_entity_names(&conn) {
        Ok(names) => names,
        Err(e) => {
            eprintln!("Error querying database: {}", e);
            process::exit(1);
        }
    };

    if entity_names.is_empty() {
        println!("No projects or persons found in database.");
        return;
    }

    eprintln!(
        "Found {} entities (projects/persons) to link",
        entity_names.len()
    );

    // Sort names by length (longest first) to avoid partial matches
    let mut sorted_names = entity_names;
    sorted_names.sort_by(|a, b| b.len().cmp(&a.len()));

    // Process all .md files in the target directory
    let mut total_replacements = 0;
    let mut files_modified = 0;

    for entry in WalkDir::new(&target_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "md") {
            match process_file_for_links(path, &sorted_names) {
                Ok(count) => {
                    if count > 0 {
                        files_modified += 1;
                        total_replacements += count;
                    }
                }
                Err(e) => {
                    eprintln!("Error processing {}: {}", path.display(), e);
                }
            }
        }
    }

    println!(
        "Linked {} replacements across {} files in {}",
        total_replacements,
        files_modified,
        target_dir.display()
    );
}

fn get_entity_names(
    conn: &rusqlite::Connection,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut names = Vec::new();

    let query = "SELECT DISTINCT filename FROM markdown_data \
                 WHERE json_extract(header_fields, '$.type') IN ('project', 'person')";

    let mut stmt = conn.prepare(query)?;
    let rows = stmt.query_map([], |row| Ok(row.get::<_, String>(0)?))?;

    for row in rows {
        let filename = row?;
        let name = Path::new(&filename)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| filename.clone());
        names.push(name);
    }

    Ok(names)
}

fn process_file_for_links(
    path: &Path,
    entity_names: &[String],
) -> Result<usize, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().collect();
    let mut new_lines = Vec::new();
    let mut total_replacements = 0;

    for line in &lines {
        let (new_line, count) = replace_entity_names_in_line(line, entity_names);
        new_lines.push(new_line);
        total_replacements += count;
    }

    if total_replacements > 0 {
        let new_content = new_lines.join("\n");
        let final_content = if content.ends_with('\n') && !new_content.ends_with('\n') {
            format!("{}\n", new_content)
        } else {
            new_content
        };
        fs::write(path, final_content)?;
    }

    Ok(total_replacements)
}

fn replace_entity_names_in_line(line: &str, entity_names: &[String]) -> (String, usize) {
    let mut result = line.to_string();
    let mut total_count = 0;

    for note_name in entity_names {
        let (new_result, count) = link_replacements(&result, note_name);
        result = new_result;
        total_count += count;
    }

    (result, total_count)
}

fn link_replacements(text: &str, note_name: &str) -> (String, usize) {
    let pattern = build_entity_pattern(note_name);
    let re = match regex::Regex::new(&pattern) {
        Ok(re) => re,
        Err(_) => return (text.to_string(), 0),
    };

    let mut result = String::new();
    let mut last_end = 0;
    let mut count = 0;

    for cap in re.find_iter(text) {
        let start = cap.start();
        let end = cap.end();

        let boundary_before = start == 0
            || (!text.as_bytes()[start - 1].is_ascii_alphanumeric()
                && text.as_bytes()[start - 1] != b'_'
                && text.as_bytes()[start - 1] != b'-');

        let boundary_after = end == text.len()
            || (!text.as_bytes()[end].is_ascii_alphanumeric()
                && text.as_bytes()[end] != b'_'
                && text.as_bytes()[end] != b'-');

        if !boundary_before || !boundary_after {
            continue;
        }

        if is_inside_wiki_link(text, start) {
            continue;
        }

        result.push_str(&text[last_end..start]);
        result.push_str(&format!("[[{}]]", note_name));
        last_end = end;
        count += 1;
    }

    if count > 0 {
        result.push_str(&text[last_end..]);
        (result, count)
    } else {
        (text.to_string(), 0)
    }
}

fn build_entity_pattern(name: &str) -> String {
    let mut pattern = String::from("(?:");

    for ch in name.chars() {
        if ch == ' ' || ch == '_' || ch == '-' {
            pattern.push_str("[_\\s-]");
        } else if ch.is_ascii_alphabetic() {
            let lower = ch.to_ascii_lowercase();
            let upper = ch.to_ascii_uppercase();
            pattern.push_str(&format!("[{}{}]", lower, upper));
        } else {
            pattern.push_str(&regex::escape(&ch.to_string()));
        }
    }

    pattern.push(')');
    pattern
}

fn is_inside_wiki_link(text: &str, pos: usize) -> bool {
    let before = &text[..pos];
    if let Some(open_pos) = before.rfind("[[") {
        let between = &text[open_pos + 2..pos];
        if !between.contains("]]") {
            return true;
        }
    }
    false
}

fn handle_convert(source: &str, output_dir: &str) {
    use note_search::converter::{
        convert_document, convert_email, convert_msg, convert_reddit_discussion, convert_web_page,
        create_note, is_reddit_url, is_url,
    };
    use std::path::Path;

    let output_path = Path::new(output_dir);

    // Ensure output directory exists
    if !output_path.exists() {
        eprintln!("Error: Output directory '{}' does not exist", output_dir);
        process::exit(1);
    }

    let is_eml = Path::new(source)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("eml"));
    let is_msg = Path::new(source)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("msg"));

    let result = if is_reddit_url(source) {
        println!("Converting Reddit discussion: {}", source);
        convert_reddit_discussion(source)
    } else if is_url(source) {
        println!("Converting web page: {}", source);
        convert_web_page(source)
    } else if is_eml {
        let source_path = Path::new(source);
        if !source_path.exists() {
            eprintln!("Error: Source file '{}' does not exist", source);
            process::exit(1);
        }
        println!("Converting email: {}", source);
        convert_email(source_path)
    } else if is_msg {
        let source_path = Path::new(source);
        if !source_path.exists() {
            eprintln!("Error: Source file '{}' does not exist", source);
            process::exit(1);
        }
        println!("Converting Outlook message: {}", source);
        convert_msg(source_path)
    } else {
        let source_path = Path::new(source);
        if !source_path.exists() {
            eprintln!("Error: Source file '{}' does not exist", source);
            process::exit(1);
        }
        println!("Converting document: {}", source);
        convert_document(source_path)
    };

    match result {
        Ok((content, metadata)) => match create_note(&content, &metadata, output_path) {
            Ok(file_path) => {
                println!(
                    "Successfully created note: {} (type: {})",
                    file_path.display(),
                    metadata.note_type
                );
                if let Some(title) = metadata.title {
                    println!("Title: {}", title);
                }
            }
            Err(e) => {
                eprintln!("Error creating note: {}", e);
                process::exit(1);
            }
        },
        Err(e) => {
            eprintln!("Error converting source: {}", e);
            process::exit(1);
        }
    }
}

fn handle_jira_import(jql: &str, output_dir: &str) {
    let output_path = Path::new(output_dir);

    if !output_path.exists() {
        eprintln!("Error: Output directory '{}' does not exist", output_dir);
        process::exit(1);
    }

    println!("Importing JIRA issues with JQL: {}", jql);

    match note_search::jira::import_jira_issues(jql, output_path) {
        Ok(count) => {
            println!(
                "Successfully imported {} JIRA issues to {}/jira/",
                count, output_dir
            );
        }
        Err(e) => {
            eprintln!("Error importing JIRA issues: {}", e);
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod linker_tests {
    use super::*;

    #[test]
    fn test_build_entity_pattern_simple() {
        let pattern = build_entity_pattern("Alpha");
        let re = regex::Regex::new(&pattern).unwrap();
        assert!(re.is_match("Alpha"));
        assert!(re.is_match("alpha"));
        assert!(re.is_match("ALPHA"));
    }

    #[test]
    fn test_build_entity_pattern_with_spaces() {
        let pattern = build_entity_pattern("Project Alpha");
        let re = regex::Regex::new(&pattern).unwrap();
        assert!(re.is_match("Project Alpha"));
        assert!(re.is_match("Project_Alpha"));
        assert!(re.is_match("Project-Alpha"));
        assert!(re.is_match("project alpha"));
    }

    #[test]
    fn test_link_replacements_basic() {
        let (result, count) = link_replacements("Working on Project Alpha today", "Project Alpha");
        assert_eq!(count, 1);
        assert_eq!(result, "Working on [[Project Alpha]] today");
    }

    #[test]
    fn test_link_replacements_case_insensitive() {
        let (result, count) = link_replacements("Working on project alpha today", "Project Alpha");
        assert_eq!(count, 1);
        assert_eq!(result, "Working on [[Project Alpha]] today");
    }

    #[test]
    fn test_link_replacements_underscore_match() {
        let (result, count) = link_replacements("Working on Project_Alpha today", "Project Alpha");
        assert_eq!(count, 1);
        assert_eq!(result, "Working on [[Project Alpha]] today");
    }

    #[test]
    fn test_link_replacements_skip_already_linked() {
        let (result, count) =
            link_replacements("See [[Project Alpha]] for details", "Project Alpha");
        assert_eq!(count, 0);
        assert_eq!(result, "See [[Project Alpha]] for details");
    }

    #[test]
    fn test_link_replacements_word_boundary() {
        let (result, count) = link_replacements("The Alpha release is here", "Alpha");
        assert_eq!(count, 1);
        assert_eq!(result, "The [[Alpha]] release is here");
    }

    #[test]
    fn test_link_replacements_no_partial_match() {
        let (result, count) = link_replacements("AlphaCentauri is a star", "Alpha");
        assert_eq!(count, 0);
        assert_eq!(result, "AlphaCentauri is a star");
    }

    #[test]
    fn test_link_replacements_no_prefix_match() {
        let (result, count) = link_replacements("The SubAlpha project", "Alpha");
        assert_eq!(count, 0);
        assert_eq!(result, "The SubAlpha project");
    }

    #[test]
    fn test_link_replacements_multiple_occurrences() {
        let (result, count) = link_replacements("Alpha and more Alpha here", "Alpha");
        assert_eq!(count, 2);
        assert_eq!(result, "[[Alpha]] and more [[Alpha]] here");
    }

    #[test]
    fn test_replace_entity_names_in_line_multiple_entities() {
        let names = vec!["Project Alpha".to_string(), "Jane Smith".to_string()];
        let (result, count) =
            replace_entity_names_in_line("Project Alpha assigned to Jane Smith", &names);
        assert_eq!(count, 2);
        assert_eq!(result, "[[Project Alpha]] assigned to [[Jane Smith]]");
    }

    #[test]
    fn test_is_inside_wiki_link() {
        assert!(is_inside_wiki_link("See [[Project Alpha", 10));
        assert!(!is_inside_wiki_link("See [[Project]] Alpha", 17));
        assert!(!is_inside_wiki_link("Project Alpha is cool", 0));
    }
}
