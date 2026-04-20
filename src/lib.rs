pub mod attribute_pair;
pub mod converter;
pub mod database_service;
pub mod jira;
pub mod markdown_parser;
pub mod query_builder;
pub mod search_criteria;

// Re-export commonly used types
pub use converter::{
    convert_document, convert_email, convert_msg, convert_reddit_discussion, convert_web_page,
    create_note, is_reddit_url, is_url,
};
pub use markdown_parser::{
    init_database_schema, remove_orphaned_notes, write_markdown_data_to_sqlite_with_conn,
};
pub use search_criteria::{DateComparison, DateRange, DueDateCriteria, SearchCriteria, SortOrder};
