use clap::{Parser, Subcommand};
use note_search::commands::args::{CommonSearchArgs, TodoSearchArgs};
use std::env;
use std::process;

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

        /// Search for todos due on or before the specified date (YYYYMMDD or YYYY-MM-DD)
        #[arg(long = "due-date")]
        due_date: Option<String>,

        /// Search for todos due on the specified date (YYYYMMDD or YYYY-MM-DD)
        #[arg(long = "due-date-eq")]
        due_date_eq: Option<String>,

        /// Search for todos due on or after the specified date (YYYYMMDD or YYYY-MM-DD)
        #[arg(long = "due-date-gt")]
        due_date_gt: Option<String>,

        /// Search for open todos only
        #[arg(long = "open")]
        open: bool,

        /// Search for closed todos only
        #[arg(long = "closed")]
        closed: bool,

        /// Specific note to generate agenda for
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

        /// Hide the summary section in agenda output
        #[arg(long = "no-summary")]
        no_summary: bool,
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

    /// Fetch a single JIRA issue as markdown (outputs to stdout)
    JiraIssue {
        /// Issue key to fetch (e.g., PROJ-123)
        #[arg(value_name = "ISSUE_KEY")]
        issue_key: String,

        /// Output directory for saving the issue
        #[arg(short = 'o', long = "output")]
        output: Option<String>,

        /// Print to stdout instead of saving to file
        #[arg(short = 'p', long = "print")]
        print: bool,
    },

    /// Import browser history from Safari, Vivaldi, and Firefox
    BrowserHistory {
        /// Date to fetch history for (YYYY-MM-DD format, defaults to today)
        #[arg(value_name = "DATE")]
        date: Option<String>,

        /// Number of days to include in the history (defaults to 1)
        #[arg(short = 'n', long = "days", default_value = "1")]
        days: i64,

        /// Output directory for the note (defaults to NOTE_SEARCH_DIR/web/)
        #[arg(short = 'o', long = "note-dir")]
        note_dir: Option<String>,

        /// Use last timestamp from previous run (overrides --days)
        #[arg(short = 't', long = "use-timestamp")]
        use_timestamp: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    let database = if cli.database != "./note.sqlite" {
        cli.database.clone()
    } else {
        env::var("NOTE_SEARCH_DATABASE").unwrap_or_else(|_| cli.database.clone())
    };

    match &cli.command {
        Commands::Todos(args) => {
            note_search::commands::search::handle_todos_search(args, &database);
        }
        Commands::Notes(args) => {
            note_search::commands::search::handle_notes_search(args, &database);
        }
        Commands::Import {
            input,
            output,
            watch,
            interval,
        } => {
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
                note_search::commands::import::handle_watch_import(
                    &database,
                    &input_dir,
                    output.as_deref(),
                    *interval,
                );
            } else {
                note_search::commands::import::handle_import(
                    &database,
                    &input_dir,
                    output.as_deref(),
                );
            }
        }
        Commands::Clear { yes } => {
            note_search::commands::clear::handle_clear(&database, *yes);
        }
        Commands::Values { field } => {
            note_search::commands::metadata::handle_values(&database, field);
        }
        Commands::Attributes => {
            note_search::commands::metadata::handle_attributes(&database);
        }
        Commands::Backlinks { filename } => {
            note_search::commands::backlinks::handle_backlinks(&database, filename);
        }
        Commands::ListNames => {
            note_search::commands::list_names::handle_list_names(&database);
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
            no_summary,
        } => {
            let type_filter = if *projects {
                "project"
            } else if *departments {
                "department"
            } else if *persons {
                "person"
            } else if *companies {
                "company"
            } else {
                "project"
            };

            let sort = common.sort.as_deref().unwrap_or("due");
            note_search::commands::agenda::handle_agenda(
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
                *no_summary,
            );
        }
        Commands::Convert { source, output } => {
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

            note_search::commands::convert::handle_convert(source, &output_dir);
        }
        Commands::Linker { subdir } => {
            note_search::commands::linker::handle_linker(&database, subdir);
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
            note_search::commands::jira::handle_jira_import(jql_query, &output_dir);
        }
        Commands::JiraIssue {
            issue_key,
            output,
            print,
        } => {
            let output_dir = if *print {
                env::var("NOTE_SEARCH_DIR").ok()
            } else {
                match output {
                    Some(dir) => Some(dir.clone()),
                    None => env::var("NOTE_SEARCH_DIR").ok(),
                }
            };

            note_search::commands::jira::handle_jira_single_issue(
                issue_key,
                output_dir.as_deref(),
                *print,
            );
        }
        Commands::BrowserHistory {
            date,
            days,
            note_dir,
            use_timestamp,
        } => {
            note_search::commands::browser_history::handle_browser_history(
                date.as_ref(),
                *days,
                note_dir.as_ref(),
                *use_timestamp,
            );
        }
    }
}
