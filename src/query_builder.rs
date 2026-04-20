use crate::attribute_pair::AttributePair;
use crate::search_criteria::{
    DateComparison, DateRange, DueDateCriteria, SearchCriteria, SortOrder,
};
use chrono::NaiveDate;

pub struct QueryBuilder {
    query: String,
    parameters: Vec<Parameter>,
    conditions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Parameter {
    Text(String),
    Int(i32),
}

impl QueryBuilder {
    pub fn new() -> Self {
        QueryBuilder {
            query: String::new(),
            parameters: Vec::new(),
            conditions: Vec::new(),
        }
    }

    pub fn build_query(mut self, criteria: &SearchCriteria) -> Self {
        self.build_base_query();
        self.add_tag_conditions(&criteria.tags);
        self.add_link_conditions(&criteria.links);
        self.add_attribute_conditions(&criteria.attributes);
        self.add_text_condition(criteria.text.as_deref());
        self.add_body_search_condition(criteria.search_body.as_deref());
        self.add_priority_condition(criteria.priority.as_deref());
        self.add_due_date_condition(criteria.due_date.as_ref());
        self.add_date_range_condition(criteria.date_range.as_ref());
        self.add_created_date_conditions(
            criteria.created_start.as_deref(),
            criteria.created_end.as_deref(),
        );
        self.add_open_condition(criteria.open);
        self.add_where_clause();
        self.add_order_by(criteria.sort_order.as_ref());
        self
    }

    fn build_base_query(&mut self) {
        self.query.push_str(
            "SELECT t.filename, t.line_number, t.text, t.tags, t.links, t.priority, t.due, m.header_fields FROM todo_entries t JOIN markdown_data m ON t.filename = m.filename "
        );
    }

    fn add_tag_conditions(&mut self, tags: &[String]) {
        for tag in tags {
            // Normalize tag: lowercase and replace underscores with spaces for matching
            let normalized_tag = tag.to_lowercase().replace('_', " ");
            // Search in both todo tags and markdown header fields (case-insensitive, space/underscore equivalent)
            self.conditions.push(
                "(LOWER(REPLACE(REPLACE(t.tags, '_', ' '), '  ', ' ')) LIKE '%\"' || LOWER(REPLACE(?, '_', ' ')) || '\"%' OR LOWER(REPLACE(m.header_fields, '_', ' ')) LIKE '%\"tags\":%' || LOWER(REPLACE(?, '_', ' ')) || '%')".to_string()
            );
            self.parameters
                .push(Parameter::Text(normalized_tag.clone()));
            self.parameters.push(Parameter::Text(normalized_tag));
        }
    }

    fn add_link_conditions(&mut self, links: &[String]) {
        for link in links {
            let normalized_link = link.to_lowercase().replace('_', " ");
            // Use LIKE with OR for case-insensitive Unicode matching:
            // match against both the original and lowercase versions to handle Unicode chars
            // that SQLite LOWER() doesn't handle (e.g. German Ü, Ö, Ä)
            self.conditions.push(
                "(LOWER(REPLACE(t.links, '_', ' ')) LIKE '%\"' || LOWER(REPLACE(?, '_', ' ')) || '\"%' OR LOWER(REPLACE(m.links, '_', ' ')) LIKE '%\"' || LOWER(REPLACE(?, '_', ' ')) || '\"%' OR LOWER(REPLACE(m.header_fields, '_', ' ')) LIKE '%\"links\":%' || LOWER(REPLACE(?, '_', ' ')) || '%%' OR REPLACE(t.links, '_', ' ') LIKE '%\"' || REPLACE(?, '_', ' ') || '\"%' OR REPLACE(m.links, '_', ' ') LIKE '%\"' || REPLACE(?, '_', ' ') || '\"%' OR REPLACE(m.header_fields, '_', ' ') LIKE '%\"links\":%' || REPLACE(?, '_', ' ') || '%')".to_string()
            );
            self.parameters
                .push(Parameter::Text(normalized_link.clone()));
            self.parameters
                .push(Parameter::Text(normalized_link.clone()));
            self.parameters
                .push(Parameter::Text(normalized_link.clone()));
            self.parameters
                .push(Parameter::Text(link.replace('_', " ")));
            self.parameters
                .push(Parameter::Text(link.replace('_', " ")));
            self.parameters
                .push(Parameter::Text(link.replace('_', " ")));
        }
    }

    fn add_attribute_conditions(&mut self, attributes: &[AttributePair]) {
        for attr in attributes {
            self.conditions
                .push("(m.header_fields LIKE '%' || ? || '%' || ? || '%')".to_string());
            self.parameters.push(Parameter::Text(attr.key.clone()));
            self.parameters.push(Parameter::Text(attr.value.clone()));
        }
    }

    fn add_text_condition(&mut self, text: Option<&str>) {
        if let Some(t) = text {
            if !t.is_empty() {
                self.conditions.push(
                    "(t.text LIKE '%' || ? || '%' OR m.header_fields LIKE '%' || ? || '%')"
                        .to_string(),
                );
                self.parameters.push(Parameter::Text(t.to_string()));
                self.parameters.push(Parameter::Text(t.to_string()));
            }
        }
    }

    fn add_body_search_condition(&mut self, search_body: Option<&str>) {
        if let Some(body) = search_body {
            if !body.is_empty() {
                // Case-insensitive search in body text using LOWER()
                self.conditions
                    .push("LOWER(m.body) LIKE '%' || LOWER(?) || '%'".to_string());
                self.parameters.push(Parameter::Text(body.to_string()));
            }
        }
    }

    fn add_priority_condition(&mut self, priority: Option<&str>) {
        if let Some(p) = priority {
            if !p.is_empty() {
                self.conditions.push("t.priority = ?".to_string());
                self.parameters.push(Parameter::Text(p.to_string()));
            }
        }
    }

    fn add_due_date_condition(&mut self, due_date: Option<&DueDateCriteria>) {
        if let Some(criteria) = due_date {
            if !criteria.date.is_empty() {
                if let Ok(date) = NaiveDate::parse_from_str(&criteria.date, "%Y%m%d") {
                    let operator = match criteria.comparison {
                        DateComparison::Equal => "=",
                        DateComparison::LessThan => "<=",
                        DateComparison::GreaterThan => ">=",
                    };
                    self.conditions.push(format!("t.due {} ?", operator));
                    self.parameters.push(Parameter::Text(date.to_string()));
                } else {
                    eprintln!("Invalid date format. Expected YYYYMMDD format.");
                }
            }
        }
    }

    fn add_date_range_condition(&mut self, date_range: Option<&DateRange>) {
        if let Some(range) = date_range {
            let (start_date, end_date) = range.to_date_range();

            // Parse and validate dates
            if let (Ok(start), Ok(end)) = (
                NaiveDate::parse_from_str(&start_date, "%Y%m%d"),
                NaiveDate::parse_from_str(&end_date, "%Y%m%d"),
            ) {
                // Use SQLite's json_extract to get the created date from header_fields
                // and compare with the date range
                // The date is stored as YYYY-MM-DD in the JSON, so we format our params accordingly
                let start_str = start.format("%Y-%m-%d").to_string();
                let end_str = end.format("%Y-%m-%d").to_string();

                self.conditions.push(
                    "json_extract(m.header_fields, '$.created') >= ? AND json_extract(m.header_fields, '$.created') <= ?"
                        .to_string(),
                );
                self.parameters.push(Parameter::Text(start_str));
                self.parameters.push(Parameter::Text(end_str));
            } else {
                eprintln!("Error computing date range for '{:?}'", range);
            }
        }
    }

    fn add_created_date_conditions(&mut self, start_date: Option<&str>, end_date: Option<&str>) {
        // Handle start date
        if let Some(start) = start_date {
            if !start.is_empty() {
                if let Ok(date) = NaiveDate::parse_from_str(start, "%Y%m%d") {
                    // Format as YYYY-MM-DD for comparison with JSON date
                    let date_str = date.format("%Y-%m-%d").to_string();
                    self.conditions
                        .push("json_extract(m.header_fields, '$.created') >= ?".to_string());
                    self.parameters.push(Parameter::Text(date_str));
                } else {
                    eprintln!("Invalid start date format. Expected YYYYMMDD format.");
                }
            }
        }

        // Handle end date
        if let Some(end) = end_date {
            if !end.is_empty() {
                if let Ok(date) = NaiveDate::parse_from_str(end, "%Y%m%d") {
                    // Format as YYYY-MM-DD for comparison with JSON date
                    let date_str = date.format("%Y-%m-%d").to_string();
                    self.conditions
                        .push("json_extract(m.header_fields, '$.created') <= ?".to_string());
                    self.parameters.push(Parameter::Text(date_str));
                } else {
                    eprintln!("Invalid end date format. Expected YYYYMMDD format.");
                }
            }
        }
    }

    fn add_open_condition(&mut self, open: Option<bool>) {
        if let Some(o) = open {
            self.conditions.push("t.closed = ?".to_string());
            self.parameters.push(Parameter::Int(if o { 0 } else { 1 }));
        }
    }

    fn add_where_clause(&mut self) {
        if !self.conditions.is_empty() {
            self.query.push_str("WHERE ");
            for (i, condition) in self.conditions.iter().enumerate() {
                if i > 0 {
                    self.query.push_str(" AND ");
                }
                self.query.push_str(condition);
            }
            self.query.push(' ');
        }
    }

    fn add_order_by(&mut self, sort_order: Option<&SortOrder>) {
        match sort_order {
            Some(SortOrder::DueDate) => {
                // Sort by due date, NULLs last
                self.query
                    .push_str("ORDER BY t.due IS NULL, t.due, t.filename, t.line_number");
            }
            Some(SortOrder::Priority) => {
                // Sort by priority, NULLs last
                self.query
                    .push_str("ORDER BY t.priority IS NULL, t.priority, t.filename, t.line_number");
            }
            Some(SortOrder::Filename) => {
                self.query.push_str("ORDER BY t.filename, t.line_number");
            }
            Some(SortOrder::Modified) => {
                // Sort by file modification time (updated field in markdown_data)
                self.query
                    .push_str("ORDER BY m.updated DESC, t.filename, t.line_number");
            }
            Some(SortOrder::Created) => {
                // Sort by note creation time, DESC for newest first
                self.query
                    .push_str("ORDER BY m.created DESC, t.filename, t.line_number");
            }
            Some(SortOrder::Attr(attr_name)) => {
                // Sort by attribute from header_fields JSON
                // Use JSON extraction - this is SQLite-specific
                self.query.push_str(&format!(
                    "ORDER BY json_extract(m.header_fields, '$.{attr}') IS NULL, json_extract(m.header_fields, '$.{attr}'), t.filename, t.line_number",
                    attr = attr_name
                ));
            }
            Some(SortOrder::Text) => {
                self.query
                    .push_str("ORDER BY t.text, t.filename, t.line_number");
            }
            None => {
                // Default sort order
                self.query.push_str("ORDER BY t.filename, t.line_number");
            }
        }
    }

    pub fn get_query(&self) -> &str {
        &self.query
    }

    pub fn get_parameters(&self) -> Vec<Parameter> {
        self.parameters.clone()
    }

    pub fn build_note_query(mut self, criteria: &SearchCriteria) -> Self {
        self.build_note_base_query();
        self.add_note_tag_conditions(&criteria.tags);
        self.add_note_link_conditions(&criteria.links);
        self.add_note_attribute_conditions(&criteria.attributes);
        self.add_note_text_condition(criteria.text.as_deref());
        self.add_body_search_condition(criteria.search_body.as_deref());
        self.add_date_range_condition(criteria.date_range.as_ref());
        self.add_created_date_conditions(
            criteria.created_start.as_deref(),
            criteria.created_end.as_deref(),
        );
        self.add_where_clause();
        self.add_note_order_by(criteria.sort_order.as_ref());
        self
    }

    fn build_note_base_query(&mut self) {
        self.query.push_str(
            "SELECT m.filename, m.title, m.header_fields, m.links, m.todo_count, m.link_count FROM markdown_data m "
        );
    }

    fn add_note_tag_conditions(&mut self, tags: &[String]) {
        for tag in tags {
            // Normalize tag: lowercase and replace underscores with spaces for matching
            let normalized_tag = tag.to_lowercase().replace('_', " ");
            // Search in markdown_data tags column (aggregated from todos) and header fields
            self.conditions.push(
                "(LOWER(REPLACE(m.tags, '_', ' ')) LIKE '%\"' || LOWER(REPLACE(?, '_', ' ')) || '\"%' OR LOWER(REPLACE(m.header_fields, '_', ' ')) LIKE '%\"tags\":%' || LOWER(REPLACE(?, '_', ' ')) || '%')".to_string()
            );
            self.parameters
                .push(Parameter::Text(normalized_tag.clone()));
            self.parameters.push(Parameter::Text(normalized_tag));
        }
    }

    fn add_note_link_conditions(&mut self, links: &[String]) {
        for link in links {
            let normalized_link = link.to_lowercase().replace('_', " ");
            self.conditions.push(
                "(LOWER(REPLACE(m.links, '_', ' ')) LIKE '%\"' || LOWER(REPLACE(?, '_', ' ')) || '\"%' OR LOWER(REPLACE(m.header_fields, '_', ' ')) LIKE '%\"links\":%' || LOWER(REPLACE(?, '_', ' ')) || '%%' OR REPLACE(m.links, '_', ' ') LIKE '%\"' || REPLACE(?, '_', ' ') || '\"%' OR REPLACE(m.header_fields, '_', ' ') LIKE '%\"links\":%' || REPLACE(?, '_', ' ') || '%')".to_string()
            );
            self.parameters
                .push(Parameter::Text(normalized_link.clone()));
            self.parameters
                .push(Parameter::Text(normalized_link.clone()));
            self.parameters
                .push(Parameter::Text(link.replace('_', " ")));
            self.parameters
                .push(Parameter::Text(link.replace('_', " ")));
        }
    }

    fn add_note_attribute_conditions(&mut self, attributes: &[AttributePair]) {
        for attr in attributes {
            self.conditions
                .push("(m.header_fields LIKE '%' || ? || '%' || ? || '%')".to_string());
            self.parameters.push(Parameter::Text(attr.key.clone()));
            self.parameters.push(Parameter::Text(attr.value.clone()));
        }
    }

    fn add_note_text_condition(&mut self, text: Option<&str>) {
        if let Some(t) = text {
            if !t.is_empty() {
                self.conditions.push(
                    "(m.title LIKE '%' || ? || '%' OR m.header_fields LIKE '%' || ? || '%')"
                        .to_string(),
                );
                self.parameters.push(Parameter::Text(t.to_string()));
                self.parameters.push(Parameter::Text(t.to_string()));
            }
        }
    }

    fn add_note_order_by(&mut self, sort_order: Option<&SortOrder>) {
        match sort_order {
            Some(SortOrder::Filename) => {
                self.query.push_str("ORDER BY m.filename");
            }
            Some(SortOrder::Modified) => {
                // Sort by file modification time (updated field in markdown_data)
                self.query.push_str("ORDER BY m.updated DESC, m.filename");
            }
            Some(SortOrder::Created) => {
                // Sort by creation time (created field in markdown_data)
                // DESC puts newest first (largest timestamp)
                self.query.push_str("ORDER BY m.created DESC, m.filename");
            }
            Some(SortOrder::Attr(attr_name)) => {
                // Sort by attribute from header_fields JSON
                // Use JSON extraction - this is SQLite-specific
                self.query.push_str(&format!(
                    "ORDER BY json_extract(m.header_fields, '$.{attr}') IS NULL, json_extract(m.header_fields, '$.{attr}'), m.filename",
                    attr = attr_name
                ));
            }
            Some(SortOrder::Text) => {
                self.query.push_str("ORDER BY m.title, m.filename");
            }
            _ => {
                // Default sort order for notes
                self.query.push_str("ORDER BY m.filename");
            }
        }
    }
}

impl Default for QueryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_query_with_no_criteria() {
        let criteria = SearchCriteria::default();
        let builder = QueryBuilder::new().build_query(&criteria);

        let query = builder.get_query();
        let params = builder.get_parameters();

        assert!(query.contains("SELECT t.filename, t.line_number, t.text, t.tags, t.links, t.priority, t.due, m.header_fields"));
        assert!(query.contains("FROM todo_entries t"));
        assert!(query.contains("JOIN markdown_data m"));
        assert!(query.contains("ORDER BY t.filename, t.line_number"));
        assert!(!query.contains("WHERE"));
        assert!(params.is_empty());
    }

    #[test]
    fn test_build_query_with_tags() {
        let mut criteria = SearchCriteria::default();
        criteria.tags = vec!["feature".to_string()];

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();
        let params = builder.get_parameters();

        assert!(query.contains("WHERE"));
        assert!(query.contains("LOWER(REPLACE"));
        assert!(query.contains("t.tags"));
        assert!(query.contains("m.header_fields"));
        assert_eq!(params.len(), 2);
        assert!(matches!(&params[0], Parameter::Text(s) if s == "feature"));
        assert!(matches!(&params[1], Parameter::Text(s) if s == "feature"));
    }

    #[test]
    fn test_build_query_with_multiple_tags() {
        let mut criteria = SearchCriteria::default();
        criteria.tags = vec!["feature".to_string(), "bug".to_string()];

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();
        let params = builder.get_parameters();

        assert!(query.contains("WHERE"));
        assert!(query.contains("AND"));
        assert_eq!(params.len(), 4);
    }

    #[test]
    fn test_build_query_with_links() {
        let mut criteria = SearchCriteria::default();
        criteria.links = vec!["doc1".to_string()];

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();
        let params = builder.get_parameters();

        assert!(query.contains("WHERE"));
        assert!(query.contains("LOWER(REPLACE(t.links"));
        assert!(query.contains("LOWER(REPLACE(m.links"));
        assert_eq!(params.len(), 6); // 3 lowercase + 3 case-sensitive
        assert!(matches!(&params[0], Parameter::Text(s) if s == "doc1"));
    }

    #[test]
    fn test_build_query_with_attributes() {
        let mut criteria = SearchCriteria::default();
        criteria.attributes = vec![AttributePair::new("type", "meeting")];

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();
        let params = builder.get_parameters();

        assert!(query.contains("WHERE"));
        assert!(query.contains("m.header_fields LIKE"));
        assert_eq!(params.len(), 2);
        assert!(matches!(&params[0], Parameter::Text(s) if s == "type"));
        assert!(matches!(&params[1], Parameter::Text(s) if s == "meeting"));
    }

    #[test]
    fn test_build_query_with_text() {
        let mut criteria = SearchCriteria::default();
        criteria.text = Some("search".to_string());

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();
        let params = builder.get_parameters();

        assert!(query.contains("WHERE"));
        assert!(query.contains("t.text LIKE"));
        assert!(query.contains("m.header_fields LIKE"));
        assert_eq!(params.len(), 2);
        assert!(matches!(&params[0], Parameter::Text(s) if s == "search"));
    }

    #[test]
    fn test_build_query_with_priority() {
        let mut criteria = SearchCriteria::default();
        criteria.priority = Some("high".to_string());

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();
        let params = builder.get_parameters();

        assert!(query.contains("WHERE"));
        assert!(query.contains("t.priority = ?"));
        assert_eq!(params.len(), 1);
        assert!(matches!(&params[0], Parameter::Text(s) if s == "high"));
    }

    #[test]
    fn test_build_query_with_due_date_equal() {
        let mut criteria = SearchCriteria::default();
        criteria.due_date = Some(DueDateCriteria {
            date: "20260315".to_string(),
            comparison: DateComparison::Equal,
        });

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();
        let params = builder.get_parameters();

        assert!(query.contains("WHERE"));
        assert!(query.contains("t.due = ?"));
        assert_eq!(params.len(), 1);
        assert!(matches!(&params[0], Parameter::Text(s) if s == "2026-03-15"));
    }

    #[test]
    fn test_build_query_with_due_date_less_than() {
        let mut criteria = SearchCriteria::default();
        criteria.due_date = Some(DueDateCriteria {
            date: "20260315".to_string(),
            comparison: DateComparison::LessThan,
        });

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();
        let params = builder.get_parameters();

        assert!(query.contains("WHERE"));
        assert!(query.contains("t.due <= ?"));
        assert_eq!(params.len(), 1);
        assert!(matches!(&params[0], Parameter::Text(s) if s == "2026-03-15"));
    }

    #[test]
    fn test_build_query_with_due_date_greater_than() {
        let mut criteria = SearchCriteria::default();
        criteria.due_date = Some(DueDateCriteria {
            date: "20260315".to_string(),
            comparison: DateComparison::GreaterThan,
        });

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();
        let params = builder.get_parameters();

        assert!(query.contains("WHERE"));
        assert!(query.contains("t.due >= ?"));
        assert_eq!(params.len(), 1);
        assert!(matches!(&params[0], Parameter::Text(s) if s == "2026-03-15"));
    }

    #[test]
    fn test_build_query_with_open() {
        let mut criteria = SearchCriteria::default();
        criteria.open = Some(true);

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();
        let params = builder.get_parameters();

        assert!(query.contains("WHERE"));
        assert!(query.contains("t.closed = ?"));
        assert_eq!(params.len(), 1);
        assert!(matches!(&params[0], Parameter::Int(0)));
    }

    #[test]
    fn test_build_query_with_closed() {
        let mut criteria = SearchCriteria::default();
        criteria.open = Some(false);

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();
        let params = builder.get_parameters();

        assert!(query.contains("WHERE"));
        assert!(query.contains("t.closed = ?"));
        assert_eq!(params.len(), 1);
        assert!(matches!(&params[0], Parameter::Int(1)));
    }

    #[test]
    fn test_build_query_with_multiple_criteria() {
        let mut criteria = SearchCriteria::default();
        criteria.tags = vec!["feature".to_string()];
        criteria.priority = Some("high".to_string());
        criteria.open = Some(true);

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();
        let params = builder.get_parameters();

        assert!(query.contains("WHERE"));
        // Should have AND between conditions
        let and_count = query.matches("AND").count();
        assert_eq!(and_count, 2);
        assert_eq!(params.len(), 4); // 2 for tag + 1 for priority + 1 for open
    }

    #[test]
    fn test_build_query_with_invalid_date() {
        let mut criteria = SearchCriteria::default();
        criteria.due_date = Some(DueDateCriteria {
            date: "invalid".to_string(),
            comparison: DateComparison::Equal,
        });

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();
        let params = builder.get_parameters();

        // Invalid date should be skipped in WHERE clause
        assert!(!query.contains("t.due = ?"));
        assert!(!query.contains("t.due <= ?"));
        assert!(!query.contains("t.due >= ?"));
        assert!(params.is_empty());
    }

    #[test]
    fn test_get_parameters_returns_defensive_copy() {
        let mut criteria = SearchCriteria::default();
        criteria.tags = vec!["feature".to_string()];

        let builder = QueryBuilder::new().build_query(&criteria);
        let params1 = builder.get_parameters();
        let params2 = builder.get_parameters();

        assert_eq!(params1, params2);
    }

    #[test]
    fn test_build_query_with_empty_tags() {
        let mut criteria = SearchCriteria::default();
        criteria.tags = vec![];

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();

        assert!(!query.contains("WHERE"));
    }

    #[test]
    fn test_default_impl() {
        let builder: QueryBuilder = Default::default();
        assert!(builder.get_query().is_empty());
        assert!(builder.get_parameters().is_empty());
    }

    #[test]
    fn test_build_query_sort_by_due_date() {
        let mut criteria = SearchCriteria::default();
        criteria.sort_order = Some(SortOrder::DueDate);

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();

        assert!(query.contains("ORDER BY t.due IS NULL, t.due, t.filename, t.line_number"));
    }

    #[test]
    fn test_build_query_sort_by_priority() {
        let mut criteria = SearchCriteria::default();
        criteria.sort_order = Some(SortOrder::Priority);

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();

        assert!(
            query.contains("ORDER BY t.priority IS NULL, t.priority, t.filename, t.line_number")
        );
    }

    #[test]
    fn test_build_query_sort_by_filename() {
        let mut criteria = SearchCriteria::default();
        criteria.sort_order = Some(SortOrder::Filename);

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();

        assert!(query.contains("ORDER BY t.filename, t.line_number"));
    }

    #[test]
    fn test_build_query_sort_by_text() {
        let mut criteria = SearchCriteria::default();
        criteria.sort_order = Some(SortOrder::Text);

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();

        assert!(query.contains("ORDER BY t.text, t.filename, t.line_number"));
    }

    #[test]
    fn test_build_query_sort_by_attr() {
        let mut criteria = SearchCriteria::default();
        criteria.sort_order = Some(SortOrder::Attr("author".to_string()));

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();

        assert!(query.contains("ORDER BY json_extract(m.header_fields, '$.author') IS NULL, json_extract(m.header_fields, '$.author'), t.filename, t.line_number"));
    }

    #[test]
    fn test_build_query_sort_by_modified() {
        let mut criteria = SearchCriteria::default();
        criteria.sort_order = Some(SortOrder::Modified);

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();

        assert!(query.contains("ORDER BY m.updated DESC, t.filename, t.line_number"));
    }

    #[test]
    fn test_build_query_sort_by_created() {
        let mut criteria = SearchCriteria::default();
        criteria.sort_order = Some(SortOrder::Created);

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();

        assert!(query.contains("ORDER BY m.created DESC, t.filename, t.line_number"));
    }

    #[test]
    fn test_build_query_with_date_range_today() {
        let mut criteria = SearchCriteria::default();
        criteria.date_range = Some(DateRange::Today);

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();
        let params = builder.get_parameters();

        assert!(query.contains("WHERE"));
        assert!(query.contains("json_extract(m.header_fields, '$.created') >= ? AND json_extract(m.header_fields, '$.created') <= ?"));
        assert_eq!(params.len(), 2);
        // Both params should be Text (YYYY-MM-DD format)
        assert!(matches!(&params[0], Parameter::Text(_)));
        assert!(matches!(&params[1], Parameter::Text(_)));
        // Both should be the same date (today)
        assert_eq!(params[0], params[1]);
    }

    #[test]
    fn test_build_query_with_date_range_this_week() {
        let mut criteria = SearchCriteria::default();
        criteria.date_range = Some(DateRange::ThisWeek);

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();
        let params = builder.get_parameters();

        assert!(query.contains("WHERE"));
        assert!(query.contains("json_extract(m.header_fields, '$.created') >= ? AND json_extract(m.header_fields, '$.created') <= ?"));
        assert_eq!(params.len(), 2);
        // Should be Text parameters (YYYY-MM-DD format)
        assert!(matches!(&params[0], Parameter::Text(_)));
        assert!(matches!(&params[1], Parameter::Text(_)));
    }

    #[test]
    fn test_build_query_with_created_start_date() {
        let mut criteria = SearchCriteria::default();
        criteria.created_start = Some("20260301".to_string());

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();
        let params = builder.get_parameters();

        assert!(query.contains("WHERE"));
        assert!(query.contains("json_extract(m.header_fields, '$.created') >= ?"));
        assert_eq!(params.len(), 1);
        // Should be Text parameter (YYYY-MM-DD format)
        assert!(matches!(&params[0], Parameter::Text(s) if s == "2026-03-01"));
    }

    #[test]
    fn test_build_query_with_created_end_date() {
        let mut criteria = SearchCriteria::default();
        criteria.created_end = Some("20260331".to_string());

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();
        let params = builder.get_parameters();

        assert!(query.contains("WHERE"));
        assert!(query.contains("json_extract(m.header_fields, '$.created') <= ?"));
        assert_eq!(params.len(), 1);
        // Should be Text parameter (YYYY-MM-DD format)
        assert!(matches!(&params[0], Parameter::Text(s) if s == "2026-03-31"));
    }

    #[test]
    fn test_build_query_with_created_date_range() {
        let mut criteria = SearchCriteria::default();
        criteria.created_start = Some("20260301".to_string());
        criteria.created_end = Some("20260331".to_string());

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();
        let params = builder.get_parameters();

        assert!(query.contains("WHERE"));
        assert!(query.contains("json_extract(m.header_fields, '$.created') >= ?"));
        assert!(query.contains("json_extract(m.header_fields, '$.created') <= ?"));
        assert_eq!(params.len(), 2);
        // Should be Text parameters (YYYY-MM-DD format)
        assert!(matches!(&params[0], Parameter::Text(s) if s == "2026-03-01"));
        assert!(matches!(&params[1], Parameter::Text(s) if s == "2026-03-31"));
    }

    #[test]
    fn test_build_query_with_custom_date_range() {
        let mut criteria = SearchCriteria::default();
        criteria.date_range = Some(DateRange::Custom {
            start: "20260101".to_string(),
            end: "20261231".to_string(),
        });

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();
        let params = builder.get_parameters();

        assert!(query.contains("WHERE"));
        assert!(query.contains("json_extract(m.header_fields, '$.created') >= ? AND json_extract(m.header_fields, '$.created') <= ?"));
        assert_eq!(params.len(), 2);
        // Should be Text parameters (YYYY-MM-DD format)
        assert!(matches!(&params[0], Parameter::Text(s) if s == "2026-01-01"));
        assert!(matches!(&params[1], Parameter::Text(s) if s == "2026-12-31"));
    }

    #[test]
    fn test_build_query_with_invalid_created_start_date() {
        let mut criteria = SearchCriteria::default();
        criteria.created_start = Some("invalid".to_string());

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();
        let params = builder.get_parameters();

        // Invalid date should be skipped in WHERE clause
        assert!(!query.contains("json_extract(m.header_fields, '$.created') >= ?"));
        assert!(params.is_empty());
    }

    #[test]
    fn test_build_query_with_date_range_and_tags() {
        let mut criteria = SearchCriteria::default();
        criteria.date_range = Some(DateRange::Today);
        criteria.tags = vec!["feature".to_string()];

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();
        let params = builder.get_parameters();

        assert!(query.contains("WHERE"));
        assert!(query.contains("json_extract(m.header_fields, '$.created') >= ? AND json_extract(m.header_fields, '$.created') <= ?"));
        assert!(query.contains("AND"));
        // 2 for tag search (tag appears twice in SQL) + 2 for date range
        assert_eq!(params.len(), 4);
        // Tag conditions are added first, then date range
        assert!(matches!(&params[0], Parameter::Text(_)));
        assert!(matches!(&params[1], Parameter::Text(_)));
        assert!(matches!(&params[2], Parameter::Text(_)));
        assert!(matches!(&params[3], Parameter::Text(_)));
    }
}
