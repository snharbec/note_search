use clap::Parser;

/// Common search arguments for both todos and notes
#[derive(Parser)]
pub struct CommonSearchArgs {
    /// Search with specified tags (all must match)
    #[arg(long = "tags")]
    pub tags: Option<String>,

    /// Search with specified links (all must match)
    #[arg(long = "links")]
    pub links: Option<String>,

    /// Search with specific attribute values in the header fields
    #[arg(long = "attributes")]
    pub attributes: Option<String>,

    /// Search containing the specified text
    #[arg(long = "text")]
    pub text: Option<String>,

    /// Search for text in the note body (case-insensitive)
    #[arg(long = "search-body")]
    pub search_body: Option<String>,

    /// Search in a date range (today, yesterday, this_week, last_week, this_month, last_month, this_year, last_year)
    #[arg(long = "date-range")]
    pub date_range: Option<String>,

    /// Search on or after this date (YYYYMMDD)
    #[arg(long = "start-date")]
    pub start_date: Option<String>,

    /// Search on or before this date (YYYYMMDD)
    #[arg(long = "end-date")]
    pub end_date: Option<String>,

    /// Configure output format string
    #[arg(long = "format")]
    pub format: Option<String>,

    /// Sort results by field (filename, modified, attr:ATTRIBUTE, text)
    #[arg(long = "sort")]
    pub sort: Option<String>,

    /// List only file locations without text
    #[arg(long = "list")]
    pub list: bool,

    /// Output absolute paths instead of relative paths
    #[arg(long = "absolute-path")]
    pub absolute_path: bool,
}

/// Todo-specific search arguments (extends CommonSearchArgs)
#[derive(Parser)]
pub struct TodoSearchArgs {
    #[command(flatten)]
    pub common: CommonSearchArgs,

    /// Search for todos with the specified priority
    #[arg(long = "priority")]
    pub priority: Option<String>,

    /// Search for todos due on or before the specified date (YYYYMMDD or YYYY-MM-DD)
    #[arg(long = "due-date")]
    pub due_date: Option<String>,

    /// Search for todos due on the specified date (YYYYMMDD or YYYY-MM-DD)
    #[arg(long = "due-date-eq")]
    pub due_date_eq: Option<String>,

    /// Search for todos due on or after the specified date (YYYYMMDD or YYYY-MM-DD)
    #[arg(long = "due-date-gt")]
    pub due_date_gt: Option<String>,

    /// Search for open todos only
    #[arg(long = "open")]
    pub open: bool,

    /// Search for closed todos only
    #[arg(long = "closed")]
    pub closed: bool,
}
