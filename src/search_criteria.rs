use crate::attribute_pair::AttributePair;

#[derive(Debug, Clone, PartialEq)]
pub enum DateComparison {
    Equal,
    LessThan,
    GreaterThan,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DueDateCriteria {
    pub date: String,
    pub comparison: DateComparison,
}

/// Represents different types of date ranges for searching
#[derive(Debug, Clone, PartialEq)]
pub enum DateRange {
    Today,
    Yesterday,
    ThisWeek,
    LastWeek,
    ThisMonth,
    LastMonth,
    ThisYear,
    LastYear,
    /// Custom date range with start and end dates (YYYYMMDD format)
    Custom {
        start: String,
        end: String,
    },
}

impl DateRange {
    /// Parse a string into a DateRange
    pub fn parse(input: &str) -> Option<Self> {
        let input = input.trim().to_lowercase();
        match input.as_str() {
            "today" => Some(DateRange::Today),
            "yesterday" => Some(DateRange::Yesterday),
            "thisweek" | "this_week" | "this week" => Some(DateRange::ThisWeek),
            "lastweek" | "last_week" | "last week" => Some(DateRange::LastWeek),
            "thismonth" | "this_month" | "this month" => Some(DateRange::ThisMonth),
            "lastmonth" | "last_month" | "last month" => Some(DateRange::LastMonth),
            "thisyear" | "this_year" | "this year" => Some(DateRange::ThisYear),
            "lastyear" | "last_year" | "last year" => Some(DateRange::LastYear),
            _ => None,
        }
    }

    /// Get the start and end dates for this range (inclusive)
    /// Returns (start_date, end_date) as YYYYMMDD strings
    pub fn to_date_range(&self) -> (String, String) {
        use chrono::{Datelike, Local, NaiveDate};

        let today = Local::now().date_naive();

        match self {
            DateRange::Today => {
                let date_str = today.format("%Y%m%d").to_string();
                (date_str.clone(), date_str)
            }
            DateRange::Yesterday => {
                let yesterday = today - chrono::Duration::days(1);
                let date_str = yesterday.format("%Y%m%d").to_string();
                (date_str.clone(), date_str)
            }
            DateRange::ThisWeek => {
                // Week starts on Monday (ISO 8601)
                let days_from_monday = today.weekday().num_days_from_monday();
                let start = today - chrono::Duration::days(days_from_monday as i64);
                let end = start + chrono::Duration::days(6);
                (
                    start.format("%Y%m%d").to_string(),
                    end.format("%Y%m%d").to_string(),
                )
            }
            DateRange::LastWeek => {
                let days_from_monday = today.weekday().num_days_from_monday();
                let this_week_start = today - chrono::Duration::days(days_from_monday as i64);
                let start = this_week_start - chrono::Duration::days(7);
                let end = start + chrono::Duration::days(6);
                (
                    start.format("%Y%m%d").to_string(),
                    end.format("%Y%m%d").to_string(),
                )
            }
            DateRange::ThisMonth => {
                let start = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
                let days_in_month = if today.month() == 12 {
                    NaiveDate::from_ymd_opt(today.year() + 1, 1, 1).unwrap()
                } else {
                    NaiveDate::from_ymd_opt(today.year(), today.month() + 1, 1).unwrap()
                } - chrono::Duration::days(1);
                (
                    start.format("%Y%m%d").to_string(),
                    days_in_month.format("%Y%m%d").to_string(),
                )
            }
            DateRange::LastMonth => {
                let (year, month) = if today.month() == 1 {
                    (today.year() - 1, 12)
                } else {
                    (today.year(), today.month() - 1)
                };
                let start = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
                let end = if month == 12 {
                    NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap()
                } else {
                    NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap()
                } - chrono::Duration::days(1);
                (
                    start.format("%Y%m%d").to_string(),
                    end.format("%Y%m%d").to_string(),
                )
            }
            DateRange::ThisYear => {
                let start = NaiveDate::from_ymd_opt(today.year(), 1, 1).unwrap();
                let end = NaiveDate::from_ymd_opt(today.year(), 12, 31).unwrap();
                (
                    start.format("%Y%m%d").to_string(),
                    end.format("%Y%m%d").to_string(),
                )
            }
            DateRange::LastYear => {
                let year = today.year() - 1;
                let start = NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
                let end = NaiveDate::from_ymd_opt(year, 12, 31).unwrap();
                (
                    start.format("%Y%m%d").to_string(),
                    end.format("%Y%m%d").to_string(),
                )
            }
            DateRange::Custom { start, end } => (start.clone(), end.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SortOrder {
    DueDate,
    Priority,
    Filename,
    Modified,
    Created,
    Attr(String),
    Text,
}

pub struct SearchCriteria {
    pub tags: Vec<String>,
    pub links: Vec<String>,
    pub attributes: Vec<AttributePair>,
    pub text: Option<String>,
    pub priority: Option<String>,
    pub due_date: Option<DueDateCriteria>,
    pub date_range: Option<DateRange>,
    pub created_start: Option<String>,
    pub created_end: Option<String>,
    pub open: Option<bool>,
    pub search_body: Option<String>,
    pub database_path: String,
    pub output_format: Option<String>,
    pub list_only: bool,
    pub sort_order: Option<SortOrder>,
    pub absolute_path: bool,
    pub base_path: String,
    pub note_dir: String,
}

impl Default for SearchCriteria {
    fn default() -> Self {
        SearchCriteria {
            tags: Vec::new(),
            links: Vec::new(),
            attributes: Vec::new(),
            text: None,
            priority: None,
            due_date: None,
            date_range: None,
            created_start: None,
            created_end: None,
            open: None,
            search_body: None,
            database_path: "./note.sqlite".to_string(),
            output_format: None,
            list_only: false,
            sort_order: None,
            absolute_path: false,
            base_path: ".".to_string(),
            note_dir: ".".to_string(),
        }
    }
}

impl SearchCriteria {
    pub fn has_any_criteria(&self) -> bool {
        !self.tags.is_empty()
            || !self.links.is_empty()
            || !self.attributes.is_empty()
            || self.text.is_some()
            || self.priority.is_some()
            || self.due_date.is_some()
            || self.date_range.is_some()
            || self.created_start.is_some()
            || self.created_end.is_some()
            || self.open.is_some()
            || self.search_body.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_values() {
        let criteria = SearchCriteria::default();
        assert!(criteria.tags.is_empty());
        assert!(criteria.links.is_empty());
        assert!(criteria.attributes.is_empty());
        assert_eq!(criteria.text, None);
        assert_eq!(criteria.priority, None);
        assert_eq!(criteria.due_date, None);
        assert_eq!(criteria.date_range, None);
        assert_eq!(criteria.created_start, None);
        assert_eq!(criteria.created_end, None);
        assert_eq!(criteria.open, None);
        assert_eq!(criteria.search_body, None);
        assert_eq!(criteria.database_path, "./note.sqlite");
        assert_eq!(criteria.output_format, None);
        assert!(!criteria.list_only);
        assert_eq!(criteria.sort_order, None);
    }

    #[test]
    fn test_has_any_criteria_with_no_criteria() {
        let criteria = SearchCriteria::default();
        assert!(!criteria.has_any_criteria());
    }

    #[test]
    fn test_has_any_criteria_with_tags() {
        let mut criteria = SearchCriteria::default();
        criteria.tags = vec!["feature".to_string()];
        assert!(criteria.has_any_criteria());
    }

    #[test]
    fn test_has_any_criteria_with_links() {
        let mut criteria = SearchCriteria::default();
        criteria.links = vec!["doc1".to_string()];
        assert!(criteria.has_any_criteria());
    }

    #[test]
    fn test_has_any_criteria_with_attributes() {
        let mut criteria = SearchCriteria::default();
        criteria.attributes = vec![AttributePair::new("type", "meeting")];
        assert!(criteria.has_any_criteria());
    }

    #[test]
    fn test_has_any_criteria_with_text() {
        let mut criteria = SearchCriteria::default();
        criteria.text = Some("search".to_string());
        assert!(criteria.has_any_criteria());
    }

    #[test]
    fn test_has_any_criteria_with_priority() {
        let mut criteria = SearchCriteria::default();
        criteria.priority = Some("high".to_string());
        assert!(criteria.has_any_criteria());
    }

    #[test]
    fn test_has_any_criteria_with_due_date() {
        let mut criteria = SearchCriteria::default();
        criteria.due_date = Some(DueDateCriteria {
            date: "20260315".to_string(),
            comparison: DateComparison::Equal,
        });
        assert!(criteria.has_any_criteria());
    }

    #[test]
    fn test_has_any_criteria_with_open() {
        let mut criteria = SearchCriteria::default();
        criteria.open = Some(true);
        assert!(criteria.has_any_criteria());
    }

    #[test]
    fn test_has_any_criteria_with_closed() {
        let mut criteria = SearchCriteria::default();
        criteria.open = Some(false);
        assert!(criteria.has_any_criteria());
    }

    #[test]
    fn test_has_any_criteria_with_database_path() {
        // Database path alone should not count as a search criterion
        let mut criteria = SearchCriteria::default();
        criteria.database_path = "/path/to/db.sqlite".to_string();
        assert!(!criteria.has_any_criteria());
    }

    #[test]
    fn test_has_any_criteria_with_output_format() {
        // Output format alone should not count as a search criterion
        let mut criteria = SearchCriteria::default();
        criteria.output_format = Some("{filename}:{line_number}".to_string());
        assert!(!criteria.has_any_criteria());
    }

    #[test]
    fn test_has_any_criteria_with_list_only() {
        // List only flag alone should not count as a search criterion
        let mut criteria = SearchCriteria::default();
        criteria.list_only = true;
        assert!(!criteria.has_any_criteria());
    }

    #[test]
    fn test_has_any_criteria_with_sort_order() {
        // Sort order alone should not count as a search criterion
        let mut criteria = SearchCriteria::default();
        criteria.sort_order = Some(SortOrder::DueDate);
        assert!(!criteria.has_any_criteria());
    }

    #[test]
    fn test_has_any_criteria_with_multiple() {
        let mut criteria = SearchCriteria::default();
        criteria.tags = vec!["feature".to_string(), "bug".to_string()];
        criteria.priority = Some("high".to_string());
        criteria.open = Some(true);
        assert!(criteria.has_any_criteria());
    }

    #[test]
    fn test_has_any_criteria_with_search_body() {
        let mut criteria = SearchCriteria::default();
        criteria.search_body = Some("architecture".to_string());
        assert!(criteria.has_any_criteria());
    }

    #[test]
    fn test_sort_order_variants() {
        assert_eq!(SortOrder::DueDate, SortOrder::DueDate);
        assert_eq!(SortOrder::Priority, SortOrder::Priority);
        assert_eq!(SortOrder::Filename, SortOrder::Filename);
        assert_eq!(SortOrder::Text, SortOrder::Text);
        assert_eq!(
            SortOrder::Attr("author".to_string()),
            SortOrder::Attr("author".to_string())
        );
        assert_ne!(
            SortOrder::Attr("author".to_string()),
            SortOrder::Attr("title".to_string())
        );
    }

    #[test]
    fn test_date_range_parse_today() {
        assert_eq!(DateRange::parse("today"), Some(DateRange::Today));
        assert_eq!(DateRange::parse("TODAY"), Some(DateRange::Today));
        assert_eq!(DateRange::parse(" Today "), Some(DateRange::Today));
    }

    #[test]
    fn test_date_range_parse_yesterday() {
        assert_eq!(DateRange::parse("yesterday"), Some(DateRange::Yesterday));
        assert_eq!(DateRange::parse("YESTERDAY"), Some(DateRange::Yesterday));
    }

    #[test]
    fn test_date_range_parse_this_week() {
        assert_eq!(DateRange::parse("thisweek"), Some(DateRange::ThisWeek));
        assert_eq!(DateRange::parse("this_week"), Some(DateRange::ThisWeek));
        assert_eq!(DateRange::parse("this week"), Some(DateRange::ThisWeek));
    }

    #[test]
    fn test_date_range_parse_last_week() {
        assert_eq!(DateRange::parse("lastweek"), Some(DateRange::LastWeek));
        assert_eq!(DateRange::parse("last_week"), Some(DateRange::LastWeek));
        assert_eq!(DateRange::parse("last week"), Some(DateRange::LastWeek));
    }

    #[test]
    fn test_date_range_parse_this_month() {
        assert_eq!(DateRange::parse("thismonth"), Some(DateRange::ThisMonth));
        assert_eq!(DateRange::parse("this_month"), Some(DateRange::ThisMonth));
        assert_eq!(DateRange::parse("this month"), Some(DateRange::ThisMonth));
    }

    #[test]
    fn test_date_range_parse_last_month() {
        assert_eq!(DateRange::parse("lastmonth"), Some(DateRange::LastMonth));
        assert_eq!(DateRange::parse("last_month"), Some(DateRange::LastMonth));
        assert_eq!(DateRange::parse("last month"), Some(DateRange::LastMonth));
    }

    #[test]
    fn test_date_range_parse_this_year() {
        assert_eq!(DateRange::parse("thisyear"), Some(DateRange::ThisYear));
        assert_eq!(DateRange::parse("this_year"), Some(DateRange::ThisYear));
        assert_eq!(DateRange::parse("this year"), Some(DateRange::ThisYear));
    }

    #[test]
    fn test_date_range_parse_last_year() {
        assert_eq!(DateRange::parse("lastyear"), Some(DateRange::LastYear));
        assert_eq!(DateRange::parse("last_year"), Some(DateRange::LastYear));
        assert_eq!(DateRange::parse("last year"), Some(DateRange::LastYear));
    }

    #[test]
    fn test_date_range_parse_invalid() {
        assert_eq!(DateRange::parse("invalid"), None);
        assert_eq!(DateRange::parse(""), None);
        assert_eq!(DateRange::parse("nextweek"), None);
    }

    #[test]
    fn test_has_any_criteria_with_date_range() {
        let mut criteria = SearchCriteria::default();
        criteria.date_range = Some(DateRange::Today);
        assert!(criteria.has_any_criteria());
    }

    #[test]
    fn test_has_any_criteria_with_created_start() {
        let mut criteria = SearchCriteria::default();
        criteria.created_start = Some("20260301".to_string());
        assert!(criteria.has_any_criteria());
    }

    #[test]
    fn test_has_any_criteria_with_created_end() {
        let mut criteria = SearchCriteria::default();
        criteria.created_end = Some("20260331".to_string());
        assert!(criteria.has_any_criteria());
    }
}
