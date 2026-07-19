use crate::attribute_pair::AttributePair;
use crate::query_parser::QueryExpr;
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
        // If a query expression is set, use the Obsidian-like query path
        if let Some(expr) = &criteria.query_expr {
            return self.build_query_from_expr(criteria, expr);
        }
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
            "SELECT t.filename, t.line_number, t.text, t.tags, t.links, t.priority, t.due, m.header_fields, t.updated FROM todo_entries t JOIN markdown_data m ON t.filename = m.filename "
        );
    }

    fn add_tag_conditions(&mut self, tags: &[String]) {
        for tag in tags {
            // Normalize tag: lowercase and replace underscores with spaces for matching.
            // Checks the note's full tag aggregate (note_tags), which already includes
            // this todo's own tags plus any sibling todo/body-hashtag tags - a todo
            // tag search is intentionally note-scoped, matching link search below.
            let normalized_tag = tag.to_lowercase().replace('_', " ");
            self.conditions.push(
                "EXISTS (SELECT 1 FROM note_tags nt WHERE nt.filename = t.filename AND LOWER(REPLACE(nt.tag, '_', ' ')) = ?)".to_string()
            );
            self.parameters.push(Parameter::Text(normalized_tag));
        }
    }

    fn add_link_conditions(&mut self, links: &[String]) {
        for link in links {
            let normalized_lower = link.to_lowercase().replace('_', " ");
            let normalized_raw = link.replace('_', " ");
            // Match against both the lowercased and raw-cased versions to handle
            // Unicode chars that SQLite's LOWER() doesn't handle (e.g. German Ü, Ö, Ä).
            // Checks this todo's own links plus the note's full link aggregate.
            self.conditions.push(
                "(EXISTS (SELECT 1 FROM todo_links tl WHERE tl.todo_id = t.id AND (LOWER(REPLACE(tl.link, '_', ' ')) = ? OR REPLACE(tl.link, '_', ' ') = ?)) \
                  OR EXISTS (SELECT 1 FROM note_links nl WHERE nl.filename = t.filename AND (LOWER(REPLACE(nl.link, '_', ' ')) = ? OR REPLACE(nl.link, '_', ' ') = ?)))".to_string()
            );
            self.parameters.push(Parameter::Text(normalized_lower.clone()));
            self.parameters.push(Parameter::Text(normalized_raw.clone()));
            self.parameters.push(Parameter::Text(normalized_lower));
            self.parameters.push(Parameter::Text(normalized_raw));
        }
    }

    fn add_attribute_conditions(&mut self, attributes: &[AttributePair]) {
        for attr in attributes {
            self.conditions.push(
                "EXISTS (SELECT 1 FROM json_each(m.header_fields, '$.' || ?) WHERE LOWER(json_each.value) = LOWER(?))"
                    .to_string(),
            );
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
        // If a query expression is set, use the Obsidian-like query path
        if let Some(expr) = &criteria.query_expr {
            return self.build_note_query_from_expr(criteria, expr);
        }
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
            "SELECT m.filename, m.title, m.header_fields, m.links, m.todo_count, m.link_count, m.created, m.updated FROM markdown_data m "
        );
    }

    fn add_note_tag_conditions(&mut self, tags: &[String]) {
        for tag in tags {
            // Normalize tag: lowercase and replace underscores with spaces for matching
            let normalized_tag = tag.to_lowercase().replace('_', " ");
            self.conditions.push(
                "EXISTS (SELECT 1 FROM note_tags nt WHERE nt.filename = m.filename AND LOWER(REPLACE(nt.tag, '_', ' ')) = ?)".to_string()
            );
            self.parameters.push(Parameter::Text(normalized_tag));
        }
    }

    fn add_note_link_conditions(&mut self, links: &[String]) {
        for link in links {
            let normalized_lower = link.to_lowercase().replace('_', " ");
            let normalized_raw = link.replace('_', " ");
            self.conditions.push(
                "EXISTS (SELECT 1 FROM note_links nl WHERE nl.filename = m.filename AND (LOWER(REPLACE(nl.link, '_', ' ')) = ? OR REPLACE(nl.link, '_', ' ') = ?))".to_string()
            );
            self.parameters.push(Parameter::Text(normalized_lower));
            self.parameters.push(Parameter::Text(normalized_raw));
        }
    }

    fn add_note_attribute_conditions(&mut self, attributes: &[AttributePair]) {
        for attr in attributes {
            self.conditions.push(
                "EXISTS (SELECT 1 FROM json_each(m.header_fields, '$.' || ?) WHERE LOWER(json_each.value) = LOWER(?))"
                    .to_string(),
            );
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

    /// Build an element (paragraph/list-item/heading) query. Supports the
    /// `--query` DSL (`#tag`, `[[link]]`, `[attr:value]`, `(a OR b)`, etc.)
    /// same as `build_query`/`build_note_query`, but not todo/note-only
    /// filters (priority, due date, date range) - element_tags/element_links
    /// already have ancestor-heading and frontmatter cascade flattened in at
    /// write time, so tag/link filtering is a plain `EXISTS` with no
    /// recursion needed.
    pub fn build_element_query(mut self, criteria: &SearchCriteria) -> Self {
        if let Some(expr) = &criteria.query_expr {
            return self.build_element_query_from_expr(criteria, expr);
        }
        self.build_element_base_query();
        self.add_element_tag_conditions(&criteria.tags);
        self.add_element_link_conditions(&criteria.links);
        self.add_element_text_condition(criteria.text.as_deref());
        self.add_where_clause();
        self.add_element_order_by(criteria.sort_order.as_ref());
        self
    }

    /// Build an element query from a parsed QueryExpr (Obsidian-like syntax).
    pub fn build_element_query_from_expr(
        mut self,
        criteria: &SearchCriteria,
        expr: &QueryExpr,
    ) -> Self {
        self.build_element_base_query();
        let (condition, _params) = self.expr_to_element_condition(expr);
        if !condition.is_empty() {
            self.conditions.push(condition);
        }
        self.add_where_clause();
        self.add_element_order_by(criteria.sort_order.as_ref());
        self
    }

    fn build_element_base_query(&mut self) {
        self.query.push_str(
            "SELECT e.filename, e.start_line, e.end_line, e.heading_level, e.text, m.updated FROM elements e JOIN markdown_data m ON e.filename = m.filename "
        );
    }

    fn add_element_tag_conditions(&mut self, tags: &[String]) {
        for tag in tags {
            let normalized_tag = tag.to_lowercase().replace('_', " ");
            self.conditions.push(
                "EXISTS (SELECT 1 FROM element_tags et WHERE et.element_id = e.id AND LOWER(REPLACE(et.tag, '_', ' ')) = ?)".to_string()
            );
            self.parameters.push(Parameter::Text(normalized_tag));
        }
    }

    fn add_element_link_conditions(&mut self, links: &[String]) {
        for link in links {
            let normalized_lower = link.to_lowercase().replace('_', " ");
            let normalized_raw = link.replace('_', " ");
            self.conditions.push(
                "EXISTS (SELECT 1 FROM element_links el WHERE el.element_id = e.id AND (LOWER(REPLACE(el.link, '_', ' ')) = ? OR REPLACE(el.link, '_', ' ') = ?))".to_string()
            );
            self.parameters.push(Parameter::Text(normalized_lower));
            self.parameters.push(Parameter::Text(normalized_raw));
        }
    }

    fn add_element_text_condition(&mut self, text: Option<&str>) {
        if let Some(t) = text {
            if !t.is_empty() {
                self.conditions
                    .push("e.text LIKE '%' || ? || '%'".to_string());
                self.parameters.push(Parameter::Text(t.to_string()));
            }
        }
    }

    fn add_element_order_by(&mut self, sort_order: Option<&SortOrder>) {
        match sort_order {
            Some(SortOrder::Modified) => {
                self.query
                    .push_str("ORDER BY m.updated DESC, e.filename, e.start_line");
            }
            Some(SortOrder::Text) => {
                self.query.push_str("ORDER BY e.text, e.filename, e.start_line");
            }
            _ => {
                self.query.push_str("ORDER BY e.filename, e.start_line");
            }
        }
    }

    /// Convert a QueryExpr to a SQL condition string for element queries.
    fn expr_to_element_condition(&mut self, expr: &QueryExpr) -> (String, usize) {
        match expr {
            QueryExpr::Text(word) => {
                let param_idx = self.parameters.len();
                self.parameters.push(Parameter::Text(word.clone()));
                (
                    format!("e.text LIKE '%' || ?{idx} || '%'", idx = param_idx + 1),
                    1,
                )
            }
            QueryExpr::Tag(tag) => {
                let normalized_tag = tag.to_lowercase().replace('_', " ");
                let param_idx = self.parameters.len();
                self.parameters.push(Parameter::Text(normalized_tag));
                (
                    format!(
                        "EXISTS (SELECT 1 FROM element_tags et WHERE et.element_id = e.id AND LOWER(REPLACE(et.tag, '_', ' ')) = ?{idx})",
                        idx = param_idx + 1,
                    ),
                    1,
                )
            }
            QueryExpr::Link(link) => {
                let normalized_lower = link.to_lowercase().replace('_', " ");
                let normalized_raw = link.replace('_', " ");
                let param_idx = self.parameters.len();
                self.parameters.push(Parameter::Text(normalized_lower));
                self.parameters.push(Parameter::Text(normalized_raw));
                (
                    format!(
                        "EXISTS (SELECT 1 FROM element_links el WHERE el.element_id = e.id AND (LOWER(REPLACE(el.link, '_', ' ')) = ?{idx} OR REPLACE(el.link, '_', ' ') = ?{idx2}))",
                        idx = param_idx + 1,
                        idx2 = param_idx + 2,
                    ),
                    2,
                )
            }
            QueryExpr::Attribute { key, value } => {
                // Elements are joined to markdown_data as `m`, so the
                // containing note's attributes/timestamps are queryable the
                // same way the note query path handles them.
                let param_idx = self.parameters.len();
                match value {
                    Some(v) => {
                        let key_lower = key.to_lowercase();
                        if key_lower == "created" || key_lower == "updated" {
                            if let Some((start, end)) = Self::date_to_timestamp_range(v) {
                                self.parameters.push(Parameter::Int(start as i32));
                                self.parameters.push(Parameter::Int(end as i32));
                                (
                                    format!(
                                        "(m.{col} >= ?{idx} AND m.{col} < ?{idx2})",
                                        col = key_lower,
                                        idx = param_idx + 1,
                                        idx2 = param_idx + 2,
                                    ),
                                    2,
                                )
                            } else {
                                ("0 = 1".to_string(), 0)
                            }
                        } else {
                            self.parameters.push(Parameter::Text(key.clone()));
                            self.parameters.push(Parameter::Text(v.clone()));
                            (
                                format!(
                                    "EXISTS (SELECT 1 FROM json_each(m.header_fields, '$.' || ?{idx}) WHERE LOWER(json_each.value) = LOWER(?{idx2}))",
                                    idx = param_idx + 1,
                                    idx2 = param_idx + 2,
                                ),
                                2,
                            )
                        }
                    }
                    None => {
                        self.parameters.push(Parameter::Text(key.clone()));
                        (
                            format!(
                                "EXISTS (SELECT 1 FROM json_each(m.header_fields, '$.' || ?{idx}))",
                                idx = param_idx + 1,
                            ),
                            1,
                        )
                    }
                }
            }
            QueryExpr::And(exprs) => {
                let mut parts = Vec::new();
                let mut total_params = 0;
                for e in exprs {
                    let (cond, count) = self.expr_to_element_condition(e);
                    parts.push(cond);
                    total_params += count;
                }
                if parts.is_empty() {
                    (String::new(), 0)
                } else if parts.len() == 1 {
                    (parts.into_iter().next().unwrap(), total_params)
                } else {
                    (format!("({})", parts.join(" AND ")), total_params)
                }
            }
            QueryExpr::Or(exprs) => {
                let mut parts = Vec::new();
                let mut total_params = 0;
                for e in exprs {
                    let (cond, count) = self.expr_to_element_condition(e);
                    parts.push(cond);
                    total_params += count;
                }
                if parts.is_empty() {
                    (String::new(), 0)
                } else if parts.len() == 1 {
                    (parts.into_iter().next().unwrap(), total_params)
                } else {
                    (format!("({})", parts.join(" OR ")), total_params)
                }
            }
        }
    }

    /// Build a todo query from a parsed QueryExpr (Obsidian-like syntax).
    /// This replaces the individual criteria fields when query_expr is set.
    pub fn build_query_from_expr(mut self, criteria: &SearchCriteria, expr: &QueryExpr) -> Self {
        self.build_base_query();
        let (condition, _params) = self.expr_to_todo_condition(expr);
        if !condition.is_empty() {
            self.conditions.push(condition);
        }
        self.add_open_condition(criteria.open);
        self.add_where_clause();
        self.add_order_by(criteria.sort_order.as_ref());
        self
    }

    /// Build a note query from a parsed QueryExpr (Obsidian-like syntax).
    pub fn build_note_query_from_expr(
        mut self,
        criteria: &SearchCriteria,
        expr: &QueryExpr,
    ) -> Self {
        self.build_note_base_query();
        let (condition, _params) = self.expr_to_note_condition(expr);
        if !condition.is_empty() {
            self.conditions.push(condition);
        }
        self.add_where_clause();
        self.add_note_order_by(criteria.sort_order.as_ref());
        self
    }

    /// Convert a date string (YYYY-MM-DD or YYYYMMDD) to (start_of_day, end_of_day) Unix timestamps.
    /// Returns None if the date string is invalid.
    fn date_to_timestamp_range(date_str: &str) -> Option<(i64, i64)> {
        let normalized = date_str.replace('-', "");
        let date = NaiveDate::parse_from_str(&normalized, "%Y%m%d").ok()?;
        let start = date
            .and_hms_opt(0, 0, 0)?
            .and_local_timezone(chrono::Local)
            .unwrap()
            .timestamp();
        let end = start + 86400; // next day
        Some((start, end))
    }

    /// Convert a QueryExpr to a SQL condition string for todo queries.
    /// Returns (condition_string, number_of_parameters_added).
    fn expr_to_todo_condition(&mut self, expr: &QueryExpr) -> (String, usize) {
        match expr {
            QueryExpr::Text(word) => {
                let param_idx = self.parameters.len();
                self.parameters.push(Parameter::Text(word.clone()));
                self.parameters.push(Parameter::Text(word.clone()));
                (
                    format!(
                        "(t.text LIKE '%' || ?{idx} || '%' OR m.header_fields LIKE '%' || ?{idx2} || '%')",
                        idx = param_idx + 1,
                        idx2 = param_idx + 2,
                    ),
                    2,
                )
            }
            QueryExpr::Tag(tag) => {
                // Note-scoped: matches this todo's own tags, sibling todos'
                // tags, and body #hashtags via the note_tags aggregate.
                let normalized_tag = tag.to_lowercase().replace('_', " ");
                let param_idx = self.parameters.len();
                self.parameters.push(Parameter::Text(normalized_tag));
                (
                    format!(
                        "EXISTS (SELECT 1 FROM note_tags nt WHERE nt.filename = t.filename AND LOWER(REPLACE(nt.tag, '_', ' ')) = ?{idx})",
                        idx = param_idx + 1,
                    ),
                    1,
                )
            }
            QueryExpr::Link(link) => {
                let normalized_lower = link.to_lowercase().replace('_', " ");
                let normalized_raw = link.replace('_', " ");
                let param_idx = self.parameters.len();
                self.parameters
                    .push(Parameter::Text(normalized_lower.clone()));
                self.parameters.push(Parameter::Text(normalized_raw.clone()));
                self.parameters.push(Parameter::Text(normalized_lower));
                self.parameters.push(Parameter::Text(normalized_raw));
                (
                    format!(
                        "(EXISTS (SELECT 1 FROM todo_links tl WHERE tl.todo_id = t.id AND (LOWER(REPLACE(tl.link, '_', ' ')) = ?{idx} OR REPLACE(tl.link, '_', ' ') = ?{idx2})) \
                          OR EXISTS (SELECT 1 FROM note_links nl WHERE nl.filename = t.filename AND (LOWER(REPLACE(nl.link, '_', ' ')) = ?{idx3} OR REPLACE(nl.link, '_', ' ') = ?{idx4})))",
                        idx = param_idx + 1,
                        idx2 = param_idx + 2,
                        idx3 = param_idx + 3,
                        idx4 = param_idx + 4,
                    ),
                    4,
                )
            }
            QueryExpr::Attribute { key, value } => {
                let param_idx = self.parameters.len();
                match value {
                    Some(v) => {
                        // Check if this is a special timestamp key (created/updated)
                        let key_lower = key.to_lowercase();
                        if key_lower == "created" || key_lower == "updated" {
                            if let Some((start, end)) = Self::date_to_timestamp_range(v) {
                                self.parameters.push(Parameter::Int(start as i32));
                                self.parameters.push(Parameter::Int(end as i32));
                                (
                                    format!(
                                        "(m.{col} >= ?{idx} AND m.{col} < ?{idx2})",
                                        col = key_lower,
                                        idx = param_idx + 1,
                                        idx2 = param_idx + 2,
                                    ),
                                    2,
                                )
                            } else {
                                // Invalid date format, return a condition that matches nothing
                                ("0 = 1".to_string(), 0)
                            }
                        } else {
                            // [attr:value] → the attribute is set to `value`, either
                            // directly (scalar) or as one element of an array.
                            self.parameters.push(Parameter::Text(key.clone()));
                            self.parameters.push(Parameter::Text(v.clone()));
                            (
                                format!(
                                    "EXISTS (SELECT 1 FROM json_each(m.header_fields, '$.' || ?{idx}) WHERE LOWER(json_each.value) = LOWER(?{idx2}))",
                                    idx = param_idx + 1,
                                    idx2 = param_idx + 2,
                                ),
                                2,
                            )
                        }
                    }
                    None => {
                        // [attr] → the attribute key exists in header_fields
                        self.parameters.push(Parameter::Text(key.clone()));
                        (
                            format!(
                                "EXISTS (SELECT 1 FROM json_each(m.header_fields, '$.' || ?{idx}))",
                                idx = param_idx + 1,
                            ),
                            1,
                        )
                    }
                }
            }
            QueryExpr::And(exprs) => {
                let mut parts = Vec::new();
                let mut total_params = 0;
                for e in exprs {
                    let (cond, count) = self.expr_to_todo_condition(e);
                    parts.push(cond);
                    total_params += count;
                }
                if parts.is_empty() {
                    (String::new(), 0)
                } else if parts.len() == 1 {
                    (parts.into_iter().next().unwrap(), total_params)
                } else {
                    (format!("({})", parts.join(" AND ")), total_params)
                }
            }
            QueryExpr::Or(exprs) => {
                let mut parts = Vec::new();
                let mut total_params = 0;
                for e in exprs {
                    let (cond, count) = self.expr_to_todo_condition(e);
                    parts.push(cond);
                    total_params += count;
                }
                if parts.is_empty() {
                    (String::new(), 0)
                } else if parts.len() == 1 {
                    (parts.into_iter().next().unwrap(), total_params)
                } else {
                    (format!("({})", parts.join(" OR ")), total_params)
                }
            }
        }
    }

    /// Convert a QueryExpr to a SQL condition string for note queries.
    fn expr_to_note_condition(&mut self, expr: &QueryExpr) -> (String, usize) {
        match expr {
            QueryExpr::Text(word) => {
                let param_idx = self.parameters.len();
                self.parameters.push(Parameter::Text(word.clone()));
                self.parameters.push(Parameter::Text(word.clone()));
                self.parameters.push(Parameter::Text(word.clone()));
                (
                    format!(
                        "(m.title LIKE '%' || ?{idx} || '%' OR m.header_fields LIKE '%' || ?{idx2} || '%' OR LOWER(m.body) LIKE '%' || LOWER(?{idx3}) || '%')",
                        idx = param_idx + 1,
                        idx2 = param_idx + 2,
                        idx3 = param_idx + 3,
                    ),
                    3,
                )
            }
            QueryExpr::Tag(tag) => {
                let normalized_tag = tag.to_lowercase().replace('_', " ");
                let param_idx = self.parameters.len();
                self.parameters.push(Parameter::Text(normalized_tag));
                (
                    format!(
                        "EXISTS (SELECT 1 FROM note_tags nt WHERE nt.filename = m.filename AND LOWER(REPLACE(nt.tag, '_', ' ')) = ?{idx})",
                        idx = param_idx + 1,
                    ),
                    1,
                )
            }
            QueryExpr::Link(link) => {
                let normalized_lower = link.to_lowercase().replace('_', " ");
                let normalized_raw = link.replace('_', " ");
                let param_idx = self.parameters.len();
                self.parameters.push(Parameter::Text(normalized_lower));
                self.parameters.push(Parameter::Text(normalized_raw));
                (
                    format!(
                        "EXISTS (SELECT 1 FROM note_links nl WHERE nl.filename = m.filename AND (LOWER(REPLACE(nl.link, '_', ' ')) = ?{idx} OR REPLACE(nl.link, '_', ' ') = ?{idx2}))",
                        idx = param_idx + 1,
                        idx2 = param_idx + 2,
                    ),
                    2,
                )
            }
            QueryExpr::Attribute { key, value } => {
                let param_idx = self.parameters.len();
                match value {
                    Some(v) => {
                        // Check if this is a special timestamp key (created/updated)
                        let key_lower = key.to_lowercase();
                        if key_lower == "created" || key_lower == "updated" {
                            if let Some((start, end)) = Self::date_to_timestamp_range(v) {
                                self.parameters.push(Parameter::Int(start as i32));
                                self.parameters.push(Parameter::Int(end as i32));
                                (
                                    format!(
                                        "(m.{col} >= ?{idx} AND m.{col} < ?{idx2})",
                                        col = key_lower,
                                        idx = param_idx + 1,
                                        idx2 = param_idx + 2,
                                    ),
                                    2,
                                )
                            } else {
                                // Invalid date format, return a condition that matches nothing
                                ("0 = 1".to_string(), 0)
                            }
                        } else {
                            // [attr:value] → the attribute is set to `value`, either
                            // directly (scalar) or as one element of an array.
                            self.parameters.push(Parameter::Text(key.clone()));
                            self.parameters.push(Parameter::Text(v.clone()));
                            (
                                format!(
                                    "EXISTS (SELECT 1 FROM json_each(m.header_fields, '$.' || ?{idx}) WHERE LOWER(json_each.value) = LOWER(?{idx2}))",
                                    idx = param_idx + 1,
                                    idx2 = param_idx + 2,
                                ),
                                2,
                            )
                        }
                    }
                    None => {
                        // [attr] → the attribute key exists in header_fields
                        self.parameters.push(Parameter::Text(key.clone()));
                        (
                            format!(
                                "EXISTS (SELECT 1 FROM json_each(m.header_fields, '$.' || ?{idx}))",
                                idx = param_idx + 1,
                            ),
                            1,
                        )
                    }
                }
            }
            QueryExpr::And(exprs) => {
                let mut parts = Vec::new();
                let mut total_params = 0;
                for e in exprs {
                    let (cond, count) = self.expr_to_note_condition(e);
                    parts.push(cond);
                    total_params += count;
                }
                if parts.is_empty() {
                    (String::new(), 0)
                } else if parts.len() == 1 {
                    (parts.into_iter().next().unwrap(), total_params)
                } else {
                    (format!("({})", parts.join(" AND ")), total_params)
                }
            }
            QueryExpr::Or(exprs) => {
                let mut parts = Vec::new();
                let mut total_params = 0;
                for e in exprs {
                    let (cond, count) = self.expr_to_note_condition(e);
                    parts.push(cond);
                    total_params += count;
                }
                if parts.is_empty() {
                    (String::new(), 0)
                } else if parts.len() == 1 {
                    (parts.into_iter().next().unwrap(), total_params)
                } else {
                    (format!("({})", parts.join(" OR ")), total_params)
                }
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
    use crate::commands::backlinks::get_backlinks;
    use crate::database_service::DatabaseService;
    use crate::markdown_parser::{
        extract_elements, write_markdown_data_to_sqlite, Header, MarkdownData, TodoEntry,
    };
    use std::collections::HashMap;
    use tempfile::TempDir;

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
        assert!(query.contains("note_tags"));
        assert!(query.contains("EXISTS"));
        assert_eq!(params.len(), 1);
        assert!(matches!(&params[0], Parameter::Text(s) if s == "feature"));
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
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn test_build_query_with_links() {
        let mut criteria = SearchCriteria::default();
        criteria.links = vec!["doc1".to_string()];

        let builder = QueryBuilder::new().build_query(&criteria);
        let query = builder.get_query();
        let params = builder.get_parameters();

        assert!(query.contains("WHERE"));
        assert!(query.contains("todo_links"));
        assert!(query.contains("note_links"));
        assert_eq!(params.len(), 4); // 2 lowercase + 2 case-sensitive
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
        assert!(query.contains("json_each(m.header_fields"));
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
        // Should have AND between conditions (2 joiners) plus 1 inside the
        // tag EXISTS clause itself
        let and_count = query.matches("AND").count();
        assert_eq!(and_count, 3);
        assert_eq!(params.len(), 3); // 1 for tag + 1 for priority + 1 for open
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
        // 1 for tag search (note_tags EXISTS) + 2 for date range
        assert_eq!(params.len(), 3);
        // Tag conditions are added first, then date range
        assert!(matches!(&params[0], Parameter::Text(_)));
        assert!(matches!(&params[1], Parameter::Text(_)));
        assert!(matches!(&params[2], Parameter::Text(_)));
    }

    // --- End-to-end tests against the normalized tag/link junction tables ---
    // (query_builder tests above only assert SQL string shape; these exercise
    // the real EXISTS-based queries through DatabaseService.)

    #[test]
    fn test_todo_tag_search_is_note_scoped() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");

        // The todo has its own tag "todo_tag"; the note's body has a
        // separate #hashtag. A --tags search for the body hashtag should
        // still find this todo, since todo tag search is note-scoped (pins
        // the widened semantics agreed in the plan).
        let data = MarkdownData {
            filename: "proj.md".to_string(),
            created: 0,
            updated: 0,
            title: "Proj".to_string(),
            header: Header {
                fields: HashMap::new(),
            },
            todo: vec![TodoEntry {
                closed: false,
                priority: None,
                due: None,
                tags: vec!["todo_tag".to_string()],
                links: vec![],
                line_number: 1,
                text: "Do something".to_string(),
                updated: 0,
            }],
            link: vec![],
            body: "Some notes about #body_hashtag".to_string(),
            elements: vec![],
        };
        write_markdown_data_to_sqlite(&data, &db_path)?;

        let db_service = DatabaseService::new(db_path.to_str().unwrap());

        let mut criteria = SearchCriteria {
            database_path: db_path.to_str().unwrap().to_string(),
            ..Default::default()
        };
        criteria.tags = vec!["body_hashtag".to_string()];
        let results = db_service.search_todos(&criteria)?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].filename, "proj.md");

        // The todo's own tag still matches too.
        criteria.tags = vec!["todo_tag".to_string()];
        let results = db_service.search_todos(&criteria)?;
        assert_eq!(results.len(), 1);

        // A tag that appears nowhere in the note should not match.
        criteria.tags = vec!["nonexistent".to_string()];
        let results = db_service.search_todos(&criteria)?;
        assert!(results.is_empty());

        Ok(())
    }

    #[test]
    fn test_link_search_and_backlinks_use_note_links() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");

        let note_a = MarkdownData {
            filename: "a.md".to_string(),
            created: 0,
            updated: 0,
            title: "A".to_string(),
            header: Header {
                fields: HashMap::new(),
            },
            todo: vec![],
            link: vec!["b".to_string()],
            body: "".to_string(),
            elements: vec![],
        };
        let note_b = MarkdownData {
            filename: "b.md".to_string(),
            created: 0,
            updated: 0,
            title: "B".to_string(),
            header: Header {
                fields: HashMap::new(),
            },
            todo: vec![],
            link: vec![],
            body: "".to_string(),
            elements: vec![],
        };
        write_markdown_data_to_sqlite(&note_a, &db_path)?;
        write_markdown_data_to_sqlite(&note_b, &db_path)?;

        // --links search finds the note with the outgoing link.
        let db_service = DatabaseService::new(db_path.to_str().unwrap());
        let mut criteria = SearchCriteria {
            database_path: db_path.to_str().unwrap().to_string(),
            ..Default::default()
        };
        criteria.links = vec!["b".to_string()];
        let results = db_service.search_notes(&criteria)?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].filename, "a.md");

        // backlinks finds the same relationship from the other direction.
        let backlinks = get_backlinks(&db_path, "b.md")?;
        assert_eq!(backlinks, vec!["a.md".to_string()]);

        Ok(())
    }

    #[test]
    fn test_tag_and_link_search_underscore_normalization() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");

        let data = MarkdownData {
            filename: "note.md".to_string(),
            created: 0,
            updated: 0,
            title: "Note".to_string(),
            header: Header {
                fields: HashMap::new(),
            },
            todo: vec![TodoEntry {
                closed: false,
                priority: None,
                due: None,
                tags: vec![],
                links: vec!["My_Link".to_string()],
                line_number: 1,
                text: "Todo with a link".to_string(),
                updated: 0,
            }],
            link: vec!["My_Link".to_string()],
            body: "Tagged #my_tag here".to_string(),
            elements: vec![],
        };
        write_markdown_data_to_sqlite(&data, &db_path)?;

        let db_service = DatabaseService::new(db_path.to_str().unwrap());
        let mut criteria = SearchCriteria {
            database_path: db_path.to_str().unwrap().to_string(),
            ..Default::default()
        };

        // Searching with a space should match a tag stored with an underscore.
        criteria.tags = vec!["my tag".to_string()];
        assert_eq!(db_service.search_todos(&criteria)?.len(), 1);

        // Same underscore/space equivalence for links, from the todo path.
        criteria.tags = vec![];
        criteria.links = vec!["My Link".to_string()];
        assert_eq!(db_service.search_todos(&criteria)?.len(), 1);

        Ok(())
    }

    #[test]
    fn test_search_elements_end_to_end() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");

        let body = "- [[NeoVimNote]] project reference\n    * Sup note with indirect reference\n- [[Auto]] Reference to something else\n";
        let data = MarkdownData {
            filename: "proj.md".to_string(),
            created: 0,
            updated: 0,
            title: "Proj".to_string(),
            header: Header {
                fields: HashMap::new(),
            },
            todo: vec![],
            link: vec!["NeoVimNote".to_string(), "Auto".to_string()],
            body: body.to_string(),
            elements: extract_elements(body, &[]),
        };
        write_markdown_data_to_sqlite(&data, &db_path)?;

        let db_service = DatabaseService::new(db_path.to_str().unwrap());
        let mut criteria = SearchCriteria {
            database_path: db_path.to_str().unwrap().to_string(),
            ..Default::default()
        };
        criteria.links = vec!["NeoVimNote".to_string()];

        let results = db_service.search_elements(&criteria)?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].filename, "proj.md");
        assert_eq!(results[0].start_line, 1);
        assert_eq!(results[0].end_line, 2);
        assert_eq!(
            results[0].text,
            "[[NeoVimNote]] project reference\nSup note with indirect reference"
        );

        // Searching the second bullet's link returns only that element.
        criteria.links = vec!["Auto".to_string()];
        let results = db_service.search_elements(&criteria)?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].start_line, 3);

        Ok(())
    }

    #[test]
    fn test_search_elements_query_dsl() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");

        let body = "- [[NeoVimNote]] project reference\n    * Sup note with indirect reference\n- [[Auto]] Reference to something else\n\nA paragraph with #urgent tagged in it.\n";
        let data = MarkdownData {
            filename: "proj.md".to_string(),
            created: 0,
            updated: 0,
            title: "Proj".to_string(),
            header: Header {
                fields: HashMap::new(),
            },
            todo: vec![],
            link: vec!["NeoVimNote".to_string(), "Auto".to_string()],
            body: body.to_string(),
            elements: extract_elements(body, &[]),
        };
        write_markdown_data_to_sqlite(&data, &db_path)?;

        let db_service = DatabaseService::new(db_path.to_str().unwrap());

        // #tag via the query DSL
        let expr = crate::query_parser::parse_query("#urgent").unwrap();
        let criteria = SearchCriteria {
            database_path: db_path.to_str().unwrap().to_string(),
            query_expr: Some(expr),
            ..Default::default()
        };
        let results = db_service.search_elements(&criteria)?;
        assert_eq!(results.len(), 1);
        assert!(results[0].text.contains("urgent"));

        // [[link]] via the query DSL
        let expr = crate::query_parser::parse_query("[[NeoVimNote]]").unwrap();
        let criteria = SearchCriteria {
            database_path: db_path.to_str().unwrap().to_string(),
            query_expr: Some(expr),
            ..Default::default()
        };
        let results = db_service.search_elements(&criteria)?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].start_line, 1);

        // OR grouping
        let expr = crate::query_parser::parse_query("(#urgent OR [[Auto]])").unwrap();
        let criteria = SearchCriteria {
            database_path: db_path.to_str().unwrap().to_string(),
            query_expr: Some(expr),
            ..Default::default()
        };
        let results = db_service.search_elements(&criteria)?;
        assert_eq!(results.len(), 2);

        Ok(())
    }
}
