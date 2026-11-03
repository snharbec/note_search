use crate::attribute_pair::AttributePair;
use crate::query_parser::QueryExpr;
use crate::search_criteria::{
    DateComparison, DateRange, DueDateCriteria, SearchCriteria, SortOrder,
};
use crate::markdown_parser::{compose_umlauts, stored_attribute_value, transliterate_umlauts};
use chrono::NaiveDate;
use std::collections::HashMap;

pub struct QueryBuilder {
    query: String,
    parameters: Vec<Parameter>,
    conditions: Vec<String>,
    /// Tag spelling -> canonical tag, from the `[Tags]` config section.
    tag_aliases: HashMap<String, String>,
}

/// bm25 column weights for `notes_fts(title, body, header_fields)`: a hit in
/// the title counts most, then frontmatter, then body.
const NOTES_BM25: &str = "bm25(notes_fts, 10.0, 1.0, 3.0)";

/// One full-text search term (FTS5 query syntax) for a word or quoted
/// phrase, or `None` if it has nothing searchable (e.g. just punctuation).
/// Single words match as prefixes (`meet` finds "meetings"); the porter
/// stemmer handles the reverse (`meetings` finds "meeting"). A term with
/// umlauts also matches its transliterated spelling (`Müller`/`Mueller`).
fn fts_term(text: &str) -> Option<String> {
    if !text.chars().any(char::is_alphanumeric) {
        return None;
    }
    let suffix = if text.contains(char::is_whitespace) { "" } else { "*" };
    let quoted = |t: &str| format!("\"{}\"{}", t.replace('"', "\"\""), suffix);
    let ascii = transliterate_umlauts(text);
    if ascii == text {
        Some(quoted(text))
    } else {
        Some(format!("({} OR {})", quoted(text), quoted(&ascii)))
    }
}

/// FTS5 query for free text: each whitespace-separated word is a term,
/// joined with AND (or OR when `any`).
fn fts_words(text: &str, any: bool) -> Option<String> {
    let terms: Vec<String> = text.split_whitespace().filter_map(fts_term).collect();
    (!terms.is_empty()).then(|| terms.join(if any { " OR " } else { " AND " }))
}

/// FTS5 query OR-ing all `terms`, for relevance ranking.
fn fts_any(terms: &[String]) -> Option<String> {
    let parts: Vec<String> = terms.iter().filter_map(|t| fts_term(t)).collect();
    (!parts.is_empty()).then(|| parts.join(" OR "))
}

#[derive(Clone, Debug, PartialEq)]
pub enum Parameter {
    Text(String),
    Int(i32),
}

/// Joins per-leaf SQL condition strings into one AND/OR group, parenthesized
/// when there's more than one. Shared by the And/Or arms of
/// expr_to_todo_condition/expr_to_note_condition/expr_to_segment_condition,
/// which are otherwise identical aside from which leaf-conversion method
/// they recurse into.
fn join_conditions(parts: Vec<String>, total_params: usize, joiner: &str) -> (String, usize) {
    if parts.is_empty() {
        (String::new(), 0)
    } else if parts.len() == 1 {
        (parts.into_iter().next().unwrap(), total_params)
    } else {
        (
            format!("({})", parts.join(&format!(" {} ", joiner))),
            total_params,
        )
    }
}

/// Lowercase and treat `_` as space, the normalization used for tag/link
/// matching on both sides (relies on the Unicode-aware `LOWER` that
/// `DatabaseService::connect` registers).
fn normalize_name(name: &str) -> String {
    compose_umlauts(name).to_lowercase().replace('_', " ")
}

impl QueryBuilder {
    pub fn new() -> Self {
        QueryBuilder {
            query: String::new(),
            parameters: Vec::new(),
            conditions: Vec::new(),
            tag_aliases: HashMap::new(),
        }
    }

    /// Treat tags mapped to the same canonical tag (`meetings=meeting`) as
    /// one tag when matching.
    pub fn with_tag_aliases(mut self, tag_aliases: HashMap<String, String>) -> Self {
        self.tag_aliases = tag_aliases
            .into_iter()
            .map(|(k, v)| (normalize_name(&k), normalize_name(&v)))
            .collect();
        self
    }

    /// Push a parameter and return its `?N` placeholder.
    fn param(&mut self, p: Parameter) -> String {
        self.parameters.push(p);
        format!("?{}", self.parameters.len())
    }

    /// SQL matching tag column `col` against `tag` or any of its configured
    /// aliases (case-insensitive, `_` == space).
    fn tag_match_sql(&mut self, col: &str, tag: &str) -> String {
        let tag = normalize_name(tag);
        let canonical = self.tag_aliases.get(&tag).cloned().unwrap_or_else(|| tag.clone());
        let mut variants = vec![tag];
        if !variants.contains(&canonical) {
            variants.push(canonical.clone());
        }
        let mut aliases: Vec<String> = self
            .tag_aliases
            .iter()
            .filter(|(k, v)| **v == canonical && !variants.contains(k))
            .map(|(k, _)| k.clone())
            .collect();
        aliases.sort();
        variants.extend(aliases);
        let placeholders: Vec<String> = variants
            .into_iter()
            .map(|v| self.param(Parameter::Text(v)))
            .collect();
        format!("LOWER(REPLACE({}, '_', ' ')) IN ({})", col, placeholders.join(", "))
    }

    /// SQL matching link column `col` against `link` or, when the linked
    /// note has frontmatter `aliases:`, any other name of that note.
    fn link_match_sql(&mut self, col: &str, link: &str) -> String {
        let p = self.param(Parameter::Text(normalize_name(link)));
        format!(
            "LOWER(REPLACE({col}, '_', ' ')) IN (SELECT {p} UNION \
             SELECT LOWER(REPLACE(a2.alias, '_', ' ')) FROM note_aliases a1 \
             JOIN note_aliases a2 ON a2.filename = a1.filename \
             WHERE LOWER(REPLACE(a1.alias, '_', ' ')) = {p})"
        )
    }

    /// SQL matching attribute `key` = `value` in a `json_each` over
    /// `m.header_fields` (scalar or array element), or a link to the note
    /// `value` whose `type` is `key` (see `linked_type_sql`).
    fn note_attr_sql(&mut self, key: &str, value: &str) -> String {
        let k = self.param(Parameter::Text(key.to_string()));
        let v = self.param(Parameter::Text(stored_attribute_value(key, value)));
        let typed = self.linked_type_sql(key, value);
        let link = self.note_link_sql(value);
        format!(
            "(EXISTS (SELECT 1 FROM json_each(m.header_fields, '$.' || {k}) WHERE LOWER(json_each.value) = LOWER({v})) \
              OR ({typed} AND {link}))"
        )
    }

    fn segment_attr_sql(&mut self, key: &str, value: &str) -> String {
        let k = self.param(Parameter::Text(key.to_string()));
        let v = self.param(Parameter::Text(stored_attribute_value(key, value)));
        let typed = self.linked_type_sql(key, value);
        let link = self.segment_link_sql(value);
        format!(
            "(EXISTS (SELECT 1 FROM segment_attributes sa WHERE sa.segment_id = e.id AND LOWER(sa.key) = LOWER({k}) AND LOWER(sa.value) = LOWER({v})) \
              OR ({typed} AND {link}))"
        )
    }

    /// SQL true when a note named `name` (file name or alias, case-
    /// insensitive, `_` == space) has frontmatter `type: key`. Linking to
    /// such a note counts as having the attribute `key=name`, so
    /// `[[tse]]` gives `project=tse` when `tse.md` has `type: project`.
    /// Uncorrelated, so SQLite evaluates it once per query.
    fn linked_type_sql(&mut self, key: &str, name: &str) -> String {
        let k = self.param(Parameter::Text(key.to_string()));
        let n = self.param(Parameter::Text(normalize_name(name)));
        format!(
            "EXISTS (SELECT 1 FROM markdown_data m2, json_each(m2.header_fields, '$.type') ty \
              WHERE LOWER(ty.value) = LOWER({k}) AND ( \
                LOWER(REPLACE(m2.filename, '_', ' ')) = {n} || '.md' \
                OR substr(LOWER(REPLACE(m2.filename, '_', ' ')), -(length({n}) + 4)) = '/' || {n} || '.md' \
                OR m2.filename IN (SELECT filename FROM note_aliases \
                                   WHERE LOWER(REPLACE(alias, '_', ' ')) = {n})))"
        )
    }

    /// Full-text condition on notes (title, body, frontmatter).
    fn note_fts_sql(&mut self, fts_query: Option<String>) -> String {
        match fts_query {
            Some(q) => {
                let p = self.param(Parameter::Text(q));
                format!("m.rowid IN (SELECT rowid FROM notes_fts WHERE notes_fts MATCH {p})")
            }
            None => "1 = 1".to_string(),
        }
    }

    /// Full-text condition on segments (text and breadcrumb).
    fn segment_fts_sql(&mut self, fts_query: Option<String>) -> String {
        match fts_query {
            Some(q) => {
                let p = self.param(Parameter::Text(q));
                format!("e.id IN (SELECT rowid FROM segments_fts WHERE segments_fts MATCH {p})")
            }
            None => "1 = 1".to_string(),
        }
    }

    /// Case-insensitive substring condition on a todo's text/frontmatter.
    fn todo_text_sql(&mut self, text: &str) -> String {
        let p = self.param(Parameter::Text(text.to_string()));
        format!(
            "(LOWER(t.text) LIKE '%' || LOWER({p}) || '%' OR LOWER(m.header_fields) LIKE '%' || LOWER({p}) || '%')"
        )
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

    // Tag search for todos checks the note's full tag aggregate (note_tags),
    // which already includes this todo's own tags plus any sibling todo/
    // body-hashtag tags - intentionally note-scoped, matching link search.
    fn todo_tag_sql(&mut self, tag: &str) -> String {
        let m = self.tag_match_sql("nt.tag", tag);
        format!("EXISTS (SELECT 1 FROM note_tags nt WHERE nt.filename = t.filename AND {m})")
    }

    // Checks this todo's own links plus the note's full link aggregate.
    fn todo_link_sql(&mut self, link: &str) -> String {
        let own = self.link_match_sql("tl.link", link);
        let note = self.link_match_sql("nl.link", link);
        format!(
            "(EXISTS (SELECT 1 FROM todo_links tl WHERE tl.todo_id = t.id AND {own}) \
              OR EXISTS (SELECT 1 FROM note_links nl WHERE nl.filename = t.filename AND {note}))"
        )
    }

    fn add_tag_conditions(&mut self, tags: &[String]) {
        for tag in tags {
            let c = self.todo_tag_sql(tag);
            self.conditions.push(c);
        }
    }

    fn add_link_conditions(&mut self, links: &[String]) {
        for link in links {
            let c = self.todo_link_sql(link);
            self.conditions.push(c);
        }
    }

    fn add_attribute_conditions(&mut self, attributes: &[AttributePair]) {
        for attr in attributes {
            let c = self.note_attr_sql(&attr.key, &attr.value);
            self.conditions.push(c);
        }
    }

    fn add_text_condition(&mut self, text: Option<&str>) {
        if let Some(t) = text {
            if !t.is_empty() {
                let c = self.todo_text_sql(t);
                self.conditions.push(c);
            }
        }
    }

    fn add_body_search_condition(&mut self, search_body: Option<&str>) {
        if let Some(body) = search_body {
            if !body.is_empty() {
                // Case-insensitive literal substring search in the body.
                let p = self.param(Parameter::Text(body.to_string()));
                self.conditions
                    .push(format!("LOWER(m.body) LIKE '%' || LOWER({p}) || '%'"));
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
        let rank = criteria.text.as_deref().and_then(|t| fts_words(t, true));
        let ranked = self.build_note_base_query(rank, criteria.sort_order.is_none());
        for tag in &criteria.tags {
            let c = self.note_tag_sql(tag);
            self.conditions.push(c);
        }
        for link in &criteria.links {
            let c = self.note_link_sql(link);
            self.conditions.push(c);
        }
        for attr in &criteria.attributes {
            let c = self.note_attr_sql(&attr.key, &attr.value);
            self.conditions.push(c);
        }
        if let Some(t) = criteria.text.as_deref().filter(|t| !t.is_empty()) {
            let c = self.note_fts_sql(fts_words(t, criteria.text_match_any));
            self.conditions.push(c);
        }
        self.add_body_search_condition(criteria.search_body.as_deref());
        self.add_date_range_condition(criteria.date_range.as_ref());
        self.add_created_date_conditions(
            criteria.created_start.as_deref(),
            criteria.created_end.as_deref(),
        );
        self.add_where_clause();
        self.add_note_order_by(criteria.sort_order.as_ref(), ranked);
        self
    }

    /// SELECT for notes. With a `rank` FTS query (and no explicit sort),
    /// left-joins its bm25 score so results come back best match first.
    /// Returns whether the ranking join was added.
    fn build_note_base_query(&mut self, rank: Option<String>, use_rank: bool) -> bool {
        self.query.push_str(
            "SELECT m.filename, m.title, m.header_fields, m.links, m.todo_count, m.link_count, m.created, m.updated, m.body FROM markdown_data m "
        );
        match rank.filter(|_| use_rank) {
            Some(q) => {
                let p = self.param(Parameter::Text(q));
                self.query.push_str(&format!(
                    "LEFT JOIN (SELECT rowid AS fts_rowid, {NOTES_BM25} AS fts_rank FROM notes_fts WHERE notes_fts MATCH {p}) r ON r.fts_rowid = m.rowid "
                ));
                true
            }
            None => false,
        }
    }

    fn note_tag_sql(&mut self, tag: &str) -> String {
        let m = self.tag_match_sql("nt.tag", tag);
        format!("EXISTS (SELECT 1 FROM note_tags nt WHERE nt.filename = m.filename AND {m})")
    }

    fn note_link_sql(&mut self, link: &str) -> String {
        let m = self.link_match_sql("nl.link", link);
        format!("EXISTS (SELECT 1 FROM note_links nl WHERE nl.filename = m.filename AND {m})")
    }

    fn add_note_order_by(&mut self, sort_order: Option<&SortOrder>, ranked: bool) {
        if ranked {
            // bm25: lower is better; notes matched only via non-text terms
            // (e.g. one side of an OR) have no score and go last.
            self.query
                .push_str("ORDER BY r.fts_rank IS NULL, r.fts_rank, m.filename");
            return;
        }
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

    /// Build a segment (header-anchored section) query. Supports the
    /// `--query` DSL (`#tag`, `[[link]]`, `[attr:value]`, `(a OR b)`, etc.)
    /// same as `build_query`/`build_note_query`, but not todo/note-only
    /// filters (priority, due date, date range). A segment's tags/links are
    /// scoped to its own header+body text only (no ancestor-header cascade -
    /// see `breadcrumb` for hierarchical context), so tag/link filtering is
    /// a plain `EXISTS` against `segment_tags`/`segment_links`.
    pub fn build_segment_query(mut self, criteria: &SearchCriteria) -> Self {
        if let Some(expr) = &criteria.query_expr {
            return self.build_segment_query_from_expr(criteria, expr);
        }
        let rank = criteria.text.as_deref().and_then(|t| fts_words(t, true));
        let ranked = self.build_segment_base_query(rank, criteria.sort_order.is_none());
        self.add_segment_tag_conditions(&criteria.tags);
        self.add_segment_link_conditions(&criteria.links);

        self.add_segment_attribute_conditions(&criteria.attributes);
        if let Some(t) = criteria.text.as_deref().filter(|t| !t.is_empty()) {
            let c = self.segment_fts_sql(fts_words(t, criteria.text_match_any));
            self.conditions.push(c);
        }
        self.add_where_clause();
        self.add_segment_order_by(criteria.sort_order.as_ref(), ranked);
        self
    }

    /// Build a segment query from a parsed QueryExpr (Obsidian-like syntax).
    pub fn build_segment_query_from_expr(
        mut self,
        criteria: &SearchCriteria,
        expr: &QueryExpr,
    ) -> Self {
        let rank = fts_any(&expr.positive_text_terms());
        let ranked = self.build_segment_base_query(rank, criteria.sort_order.is_none());
        let (condition, _params) = self.expr_to_segment_condition(expr);
        if !condition.is_empty() {
            self.conditions.push(condition);
        }
        self.add_where_clause();
        self.add_segment_order_by(criteria.sort_order.as_ref(), ranked);
        self
    }

    /// Segment SELECT; same optional bm25 ranking join as
    /// `build_note_base_query`.
    fn build_segment_base_query(&mut self, rank: Option<String>, use_rank: bool) -> bool {
        self.query.push_str(
            "SELECT e.filename, e.start_line, e.end_line, e.heading_level, e.text, e.breadcrumb, m.updated FROM segments e JOIN markdown_data m ON e.filename = m.filename "
        );
        match rank.filter(|_| use_rank) {
            Some(q) => {
                let p = self.param(Parameter::Text(q));
                self.query.push_str(&format!(
                    "LEFT JOIN (SELECT rowid AS fts_rowid, bm25(segments_fts) AS fts_rank FROM segments_fts WHERE segments_fts MATCH {p}) r ON r.fts_rowid = e.id "
                ));
                true
            }
            None => false,
        }
    }

    /// Build a query for phrase/similarity search: same tag/link filtering
    /// as `build_segment_query`, but selects `e.embedding` too (for cosine
    /// similarity scoring in Rust, since SQLite has no vector ops) and skips
    /// ORDER BY - ranking happens after scoring, not in SQL. Segments with
    /// no embedding yet (e.g. Ollama was unreachable at import time) are
    /// excluded outright rather than scored as a false zero-similarity match.
    pub fn build_segment_similarity_query(mut self, tags: &[String], links: &[String]) -> Self {
        self.query.push_str(
            "SELECT e.filename, e.start_line, e.end_line, e.heading_level, e.text, e.breadcrumb, e.embedding, m.updated FROM segments e JOIN markdown_data m ON e.filename = m.filename "
        );
        self.conditions.push("e.embedding IS NOT NULL".to_string());
        self.add_segment_tag_conditions(tags);
        self.add_segment_link_conditions(links);
        self.add_where_clause();
        self
    }

    fn segment_tag_sql(&mut self, tag: &str) -> String {
        let m = self.tag_match_sql("et.tag", tag);
        format!("EXISTS (SELECT 1 FROM segment_tags et WHERE et.segment_id = e.id AND {m})")
    }

    fn segment_link_sql(&mut self, link: &str) -> String {
        let m = self.link_match_sql("el.link", link);
        format!("EXISTS (SELECT 1 FROM segment_links el WHERE el.segment_id = e.id AND {m})")
    }

    fn add_segment_tag_conditions(&mut self, tags: &[String]) {
        for tag in tags {
            let c = self.segment_tag_sql(tag);
            self.conditions.push(c);
        }
    }

    fn add_segment_link_conditions(&mut self, links: &[String]) {
        for link in links {
            let c = self.segment_link_sql(link);
            self.conditions.push(c);
        }
    }

    fn add_segment_attribute_conditions(&mut self, attributes: &[AttributePair]) {
        for attr in attributes {
            let c = self.segment_attr_sql(&attr.key, &attr.value);
            self.conditions.push(c);
        }
    }

    fn add_segment_order_by(&mut self, sort_order: Option<&SortOrder>, ranked: bool) {
        if ranked {
            self.query
                .push_str("ORDER BY r.fts_rank IS NULL, r.fts_rank, e.filename, e.start_line");
            return;
        }
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

    /// Convert a QueryExpr to a SQL condition string for segment queries.
    fn expr_to_segment_condition(&mut self, expr: &QueryExpr) -> (String, usize) {
        self.expr_to_condition(expr, Target::Segment)
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
        let rank = fts_any(&expr.positive_text_terms());
        let ranked = self.build_note_base_query(rank, criteria.sort_order.is_none());
        let (condition, _params) = self.expr_to_note_condition(expr);
        if !condition.is_empty() {
            self.conditions.push(condition);
        }
        self.add_where_clause();
        self.add_note_order_by(criteria.sort_order.as_ref(), ranked);
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
        self.expr_to_condition(expr, Target::Todo)
    }

    /// Convert a QueryExpr to a SQL condition string for note queries.
    fn expr_to_note_condition(&mut self, expr: &QueryExpr) -> (String, usize) {
        self.expr_to_condition(expr, Target::Note)
    }

    /// Shared QueryExpr -> SQL conversion for todo/note/segment queries.
    /// Returns (condition_string, number_of_parameters_added).
    fn expr_to_condition(&mut self, expr: &QueryExpr, target: Target) -> (String, usize) {
        let before = self.parameters.len();
        let condition = match expr {
            // Todos: substring in the todo text/frontmatter. Notes and
            // segments: full-text index (title/body/frontmatter, or
            // segment text/breadcrumb).
            QueryExpr::Text(word) => match target {
                Target::Todo => self.todo_text_sql(word),
                Target::Note => self.note_fts_sql(fts_term(word)),
                Target::Segment => self.segment_fts_sql(fts_term(word)),
            },
            QueryExpr::Tag(tag) => match target {
                Target::Todo => self.todo_tag_sql(tag),
                Target::Note => self.note_tag_sql(tag),
                Target::Segment => self.segment_tag_sql(tag),
            },
            QueryExpr::Link(link) => match target {
                Target::Todo => self.todo_link_sql(link),
                Target::Note => self.note_link_sql(link),
                Target::Segment => self.segment_link_sql(link),
            },
            QueryExpr::Attribute { key, value } => {
                let key_lower = key.to_lowercase();
                match value {
                    // `created`/`updated` are the epoch columns on the
                    // joined `markdown_data` row, not frontmatter strings.
                    Some(v) if key_lower == "created" || key_lower == "updated" => {
                        match Self::date_to_timestamp_range(v) {
                            Some((start, end)) => {
                                let s = self.param(Parameter::Int(start as i32));
                                let e = self.param(Parameter::Int(end as i32));
                                format!("(m.{key_lower} >= {s} AND m.{key_lower} < {e})")
                            }
                            // Invalid date: match nothing
                            None => "0 = 1".to_string(),
                        }
                    }
                    // [attr:value] → the attribute is set to `value`, either
                    // directly (scalar) or as one element of an array.
                    // Segments use `segment_attributes`, which mirrors the
                    // note's frontmatter onto every segment.
                    Some(v) => match target {
                        Target::Segment => self.segment_attr_sql(key, v),
                        _ => self.note_attr_sql(key, v),
                    },
                    // [attr] → the attribute key exists
                    None => {
                        let k = self.param(Parameter::Text(key.clone()));
                        match target {
                            Target::Segment => format!(
                                "EXISTS (SELECT 1 FROM segment_attributes sa WHERE sa.segment_id = e.id AND LOWER(sa.key) = LOWER({k}))"
                            ),
                            _ => format!(
                                "EXISTS (SELECT 1 FROM json_each(m.header_fields, '$.' || {k}))"
                            ),
                        }
                    }
                }
            }
            QueryExpr::Not(inner) => {
                let (cond, _) = self.expr_to_condition(inner, target);
                format!("NOT ({cond})")
            }
            QueryExpr::And(exprs) | QueryExpr::Or(exprs) => {
                let joiner = if matches!(expr, QueryExpr::And(_)) { "AND" } else { "OR" };
                let parts: Vec<String> = exprs
                    .iter()
                    .map(|e| self.expr_to_condition(e, target).0)
                    .collect();
                return join_conditions(parts, self.parameters.len() - before, joiner);
            }
        };
        (condition, self.parameters.len() - before)
    }
}

/// Which table a QueryExpr is being converted for.
#[derive(Clone, Copy)]
enum Target {
    Todo,
    Note,
    Segment,
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
        extract_segments, write_markdown_data_to_sqlite, Header, MarkdownData, TodoEntry,
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
        assert_eq!(params.len(), 2); // one per link check (todo's own + note's)
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
        // key + value, then the linked-type check (key, name) and the link.
        assert_eq!(params.len(), 5);
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
        assert!(query.contains("LOWER(t.text) LIKE"));
        assert!(query.contains("LOWER(m.header_fields) LIKE"));
        assert_eq!(params.len(), 1);
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
            segments: vec![],
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
            segments: vec![],
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
            segments: vec![],
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
    fn test_search_notes_includes_body_preview_and_todos() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");

        let note = MarkdownData {
            filename: "c.md".to_string(),
            created: 0,
            updated: 0,
            title: "C".to_string(),
            header: Header {
                fields: HashMap::new(),
            },
            todo: vec![TodoEntry {
                closed: false,
                priority: Some("A".to_string()),
                due: None,
                tags: vec![],
                links: vec![],
                line_number: 3,
                text: "Buy milk".to_string(),
                updated: 0,
            }],
            link: vec![],
            body: "\n\nFirst line\n\nSecond line\nThird line\nFourth line (excluded)".to_string(),
            segments: vec![],
        };
        write_markdown_data_to_sqlite(&note, &db_path)?;

        let db_service = DatabaseService::new(db_path.to_str().unwrap());
        let criteria = SearchCriteria {
            database_path: db_path.to_str().unwrap().to_string(),
            ..Default::default()
        };
        let results = db_service.search_notes(&criteria)?;
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].body_preview.as_deref(),
            Some("First line\nSecond line\nThird line")
        );
        assert_eq!(results[0].todos.len(), 1);
        assert_eq!(results[0].todos[0].text, "Buy milk");
        assert_eq!(results[0].todos[0].priority.as_deref(), Some("A"));

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
            segments: vec![],
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
    fn test_search_segments_end_to_end() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");

        let body = "# NeoVim Notes\n\n[[NeoVimNote]] project reference.\n\n# Automation\n\n[[Auto]] reference to something else.\n";
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
            segments: extract_segments(body, "proj.md", &[], &[]),
        };
        write_markdown_data_to_sqlite(&data, &db_path)?;

        let db_service = DatabaseService::new(db_path.to_str().unwrap());
        let mut criteria = SearchCriteria {
            database_path: db_path.to_str().unwrap().to_string(),
            ..Default::default()
        };
        criteria.links = vec!["NeoVimNote".to_string()];

        let results = db_service.search_segments(&criteria)?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].filename, "proj.md");
        assert_eq!(results[0].start_line, 1);
        assert_eq!(results[0].heading_level, Some(1));
        assert!(results[0].text.contains("[[NeoVimNote]] project reference."));
        assert_eq!(results[0].breadcrumb, "proj.md");

        // Searching the second segment's link returns only that segment.
        criteria.links = vec!["Auto".to_string()];
        let results = db_service.search_segments(&criteria)?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].start_line, 5);

        Ok(())
    }

    #[test]
    fn test_search_segments_query_dsl() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");

        let body = "# Alpha\n\n[[NeoVimNote]] alpha section.\n\n# Beta\n\n[[Auto]] beta section.\n\n# Gamma\n\nGamma has #urgent priority.\n";
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
            segments: extract_segments(body, "proj.md", &[], &[]),
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
        let results = db_service.search_segments(&criteria)?;
        assert_eq!(results.len(), 1);
        assert!(results[0].text.contains("urgent"));

        // [[link]] via the query DSL
        let expr = crate::query_parser::parse_query("[[NeoVimNote]]").unwrap();
        let criteria = SearchCriteria {
            database_path: db_path.to_str().unwrap().to_string(),
            query_expr: Some(expr),
            ..Default::default()
        };
        let results = db_service.search_segments(&criteria)?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].start_line, 1);

        // OR grouping: the #urgent tag and the [[Auto]] link live in
        // different segments, so this matches both.
        let expr = crate::query_parser::parse_query("(#urgent OR [[Auto]])").unwrap();
        let criteria = SearchCriteria {
            database_path: db_path.to_str().unwrap().to_string(),
            query_expr: Some(expr),
            ..Default::default()
        };
        let results = db_service.search_segments(&criteria)?;
        assert_eq!(results.len(), 2);

        Ok(())
    }

    #[test]
    fn test_search_segments_inherits_document_tags_and_attributes() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");

        // #shared appears only under "# Alpha", but as a document-level tag
        // it should still be found from every segment, including "# Beta"
        // which has no tags of its own.
        let body = "# Alpha\n\nAlpha text with #shared in it.\n\n# Beta\n\nBeta has nothing of its own.\n";
        let mut header_fields = HashMap::new();
        header_fields.insert(
            "status".to_string(),
            serde_json::Value::String("done".to_string()),
        );

        let data = MarkdownData {
            filename: "proj.md".to_string(),
            created: 0,
            updated: 0,
            title: "Proj".to_string(),
            header: Header {
                fields: header_fields,
            },
            todo: vec![],
            link: vec![],
            body: body.to_string(),
            segments: extract_segments(body, "proj.md", &["shared".to_string()], &[]),
        };
        write_markdown_data_to_sqlite(&data, &db_path)?;

        let db_service = DatabaseService::new(db_path.to_str().unwrap());

        // Document-level tag cascade: #shared matches every segment.
        let expr = crate::query_parser::parse_query("#shared").unwrap();
        let criteria = SearchCriteria {
            database_path: db_path.to_str().unwrap().to_string(),
            query_expr: Some(expr),
            ..Default::default()
        };
        let results = db_service.search_segments(&criteria)?;
        assert_eq!(results.len(), 2);

        // Attribute cascade via segment_attributes: [status:done] matches
        // every segment even though "status" only lives in the frontmatter.
        let expr = crate::query_parser::parse_query("[status:done]").unwrap();
        let criteria = SearchCriteria {
            database_path: db_path.to_str().unwrap().to_string(),
            query_expr: Some(expr),
            ..Default::default()
        };
        let results = db_service.search_segments(&criteria)?;
        assert_eq!(results.len(), 2);

        // Wrong attribute value matches nothing.
        let expr = crate::query_parser::parse_query("[status:open]").unwrap();
        let criteria = SearchCriteria {
            database_path: db_path.to_str().unwrap().to_string(),
            query_expr: Some(expr),
            ..Default::default()
        };
        let results = db_service.search_segments(&criteria)?;
        assert!(results.is_empty());

        Ok(())
    }

    #[test]
    fn test_search_similar_segments_ranks_by_cosine_similarity() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");

        // Segment content doesn't matter here - similarity is entirely
        // driven by the synthetic embeddings written below, so real Ollama
        // calls (best-effort inside write_markdown_data_to_sqlite) can't
        // make this test flaky either way.
        let body = "# Cats\n\nAbout cats.\n\n# Rockets #urgent\n\nAbout rockets.\n\n# Unembedded\n\nNever gets an embedding.\n";
        let data = MarkdownData {
            filename: "proj.md".to_string(),
            created: 0,
            updated: 0,
            title: "Proj".to_string(),
            header: Header {
                fields: HashMap::new(),
            },
            todo: vec![],
            link: vec![],
            body: body.to_string(),
            segments: extract_segments(body, "proj.md", &[], &[]),
        };
        write_markdown_data_to_sqlite(&data, &db_path)?;

        // Overwrite with known synthetic embeddings so ranking is
        // deterministic regardless of whether Ollama is reachable.
        let conn = rusqlite::Connection::open(&db_path)?;
        let cats_embedding = crate::embeddings::embedding_to_bytes(&[1.0, 0.0, 0.0]);
        let rockets_embedding = crate::embeddings::embedding_to_bytes(&[0.0, 1.0, 0.0]);
        conn.execute(
            "UPDATE segments SET embedding = ?1 WHERE text LIKE '%Cats%'",
            rusqlite::params![cats_embedding],
        )?;
        conn.execute(
            "UPDATE segments SET embedding = ?1 WHERE text LIKE '%Rockets%'",
            rusqlite::params![rockets_embedding],
        )?;
        conn.execute(
            "UPDATE segments SET embedding = NULL WHERE text LIKE '%Unembedded%'",
            [],
        )?;

        let db_service = DatabaseService::new(db_path.to_str().unwrap());

        // A phrase vector close to "Cats" should rank it first.
        let results =
            db_service.search_similar_segments(&[0.9, 0.1, 0.0], &[], &[], 10)?;
        assert_eq!(results.len(), 2, "the unembedded segment must be excluded");
        assert!(results[0].0.text.contains("Cats"));
        assert!(results[1].0.text.contains("Rockets"));
        assert!(results[0].1 > results[1].1);

        // Tag restriction narrows the candidate set before ranking - even
        // though the query vector favors "Cats", restricting to #urgent
        // (only on "Rockets") must exclude "Cats" entirely.
        let results = db_service.search_similar_segments(
            &[0.9, 0.1, 0.0],
            &["urgent".to_string()],
            &[],
            10,
        )?;
        assert_eq!(results.len(), 1);
        assert!(results[0].0.text.contains("Rockets"));

        // limit caps the result count.
        let results =
            db_service.search_similar_segments(&[0.9, 0.1, 0.0], &[], &[], 1)?;
        assert_eq!(results.len(), 1);
        assert!(results[0].0.text.contains("Cats"));

        Ok(())
    }

    // ---- full-text search, aliases, NOT, fallback ----

    fn note(filename: &str, title: &str, body: &str) -> MarkdownData {
        MarkdownData {
            filename: filename.to_string(),
            created: 0,
            updated: 0,
            title: title.to_string(),
            header: Header {
                fields: HashMap::new(),
            },
            todo: vec![],
            link: vec![],
            body: body.to_string(),
            segments: extract_segments(body, filename, &[], &[]),
        }
    }

    fn notes_for(db: &DatabaseService, criteria: SearchCriteria) -> Vec<String> {
        let criteria = SearchCriteria {
            database_path: db.database_path.clone(),
            ..criteria
        };
        db.search_notes(&criteria)
            .unwrap()
            .into_iter()
            .map(|n| n.filename)
            .collect()
    }

    fn query(q: &str) -> SearchCriteria {
        SearchCriteria {
            query_expr: Some(crate::query_parser::parse_query(q).unwrap()),
            ..Default::default()
        }
    }

    fn text(t: &str) -> SearchCriteria {
        SearchCriteria {
            text: Some(t.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn test_note_text_search_covers_body_stems_and_ranks_title_first(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");
        write_markdown_data_to_sqlite(&note("a.md", "Kickoff", "We had a meeting about the budget."), &db_path)?;
        write_markdown_data_to_sqlite(&note("b.md", "Meeting notes", "Nothing else."), &db_path)?;
        write_markdown_data_to_sqlite(&note("c.md", "Other", "Unrelated."), &db_path)?;
        let db = DatabaseService::new(db_path.to_str().unwrap());

        // Plural finds singular (stemming); the body counts; title hits rank first.
        assert_eq!(notes_for(&db, text("meetings")), vec!["b.md", "a.md"]);
        assert_eq!(notes_for(&db, query("meetings")), vec!["b.md", "a.md"]);
        // Prefix match while typing.
        assert_eq!(notes_for(&db, text("budg")), vec!["a.md"]);
        // All words must match by default.
        assert_eq!(notes_for(&db, text("meeting budget")), vec!["a.md"]);
        // An explicit sort still wins over relevance.
        let mut sorted = text("meeting");
        sorted.sort_order = Some(SortOrder::Filename);
        assert_eq!(notes_for(&db, sorted), vec!["a.md", "b.md"]);
        Ok(())
    }

    #[test]
    fn test_fts_index_follows_reimport_and_delete() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");
        write_markdown_data_to_sqlite(&note("a.md", "A", "alpha"), &db_path)?;
        write_markdown_data_to_sqlite(&note("b.md", "B", "other"), &db_path)?;
        // Re-import replaces the row (INSERT OR REPLACE) - old words must go.
        write_markdown_data_to_sqlite(&note("a.md", "A", "beta"), &db_path)?;
        let db = DatabaseService::new(db_path.to_str().unwrap());
        assert!(notes_for(&db, text("alpha")).is_empty());
        assert_eq!(notes_for(&db, text("beta")), vec!["a.md"]);
        let seg = |t: &str| {
            db.search_segments(&SearchCriteria {
                database_path: db.database_path.clone(),
                ..text(t)
            })
            .unwrap()
            .len()
        };
        assert_eq!((seg("alpha"), seg("beta")), (0, 1));

        let conn = db.connect()?;
        conn.execute("DELETE FROM segments WHERE filename = 'a.md'", [])?;
        conn.execute("DELETE FROM markdown_data WHERE filename = 'a.md'", [])?;
        assert!(notes_for(&db, text("beta")).is_empty());
        assert_eq!(seg("beta"), 0);
        Ok(())
    }

    #[test]
    fn test_umlauts_in_text_links_and_attributes() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");
        let mut n = note("a.md", "A", "Über die Besprechung mit Müller");
        // Import stores attribute values transliterated.
        n.header.fields.insert("author".to_string(), serde_json::json!("Jürgen Mueller".replace('ü', "ue")));
        n.link = vec!["Übersicht".to_string()];
        write_markdown_data_to_sqlite(&n, &db_path)?;
        let db = DatabaseService::new(db_path.to_str().unwrap());

        assert_eq!(notes_for(&db, text("über")), vec!["a.md"]);
        assert_eq!(notes_for(&db, query("[author:Jürgen Müller]")), vec!["a.md"]);
        assert_eq!(notes_for(&db, query("[[übersicht]]")), vec!["a.md"]);
        // "Müller" also matches the transliterated "Mueller" spelling.
        let mut m = note("b.md", "B", "Herr Mueller");
        m.header.fields.insert("x".to_string(), serde_json::json!("y"));
        write_markdown_data_to_sqlite(&m, &db_path)?;
        let mut found = notes_for(&db, text("Müller"));
        found.sort();
        assert_eq!(found, vec!["a.md", "b.md"]);
        Ok(())
    }

    #[test]
    fn test_not_and_relaxed_fallback() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");
        write_markdown_data_to_sqlite(&note("a.md", "A", "alpha beta"), &db_path)?;
        write_markdown_data_to_sqlite(&note("b.md", "B", "alpha #archived"), &db_path)?;
        let db = DatabaseService::new(db_path.to_str().unwrap());

        assert_eq!(notes_for(&db, query("alpha -#archived")), vec!["a.md"]);
        assert_eq!(notes_for(&db, query("alpha NOT beta")), vec!["b.md"]);

        for criteria in [query("alpha zeta"), text("alpha zeta")] {
            let criteria = SearchCriteria {
                database_path: db.database_path.clone(),
                ..criteria
            };
            let (results, relaxed) = crate::database_service::with_relaxed_fallback(
                &criteria,
                |c| db.search_notes(c),
            )?;
            assert!(relaxed);
            assert_eq!(results.len(), 2);
        }
        // A query that already matches is not loosened.
        let criteria = SearchCriteria {
            database_path: db.database_path.clone(),
            ..text("alpha beta")
        };
        let (results, relaxed) =
            crate::database_service::with_relaxed_fallback(&criteria, |c| db.search_notes(c))?;
        assert!(!relaxed);
        assert_eq!(results.len(), 1);
        Ok(())
    }

    #[test]
    fn test_link_to_typed_note_counts_as_attribute() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");
        let mut tse = note("project/tse.md", "TSE", "");
        tse.header.fields.insert("type".to_string(), serde_json::json!("project"));
        // File name and link use a decomposed ü (macOS), the query a typed one.
        let mut harald = note("person/harald stu\u{308}rmer.md", "Harald", "");
        harald.header.fields.insert("type".to_string(), serde_json::json!("person"));
        let body = "# Meeting\n\n- [[tse]] with [[Harald Stu\u{308}rmer]]\n";
        let mut meeting = note("project/tse/meeting.md", "Meeting", body);
        meeting.link = vec!["tse".to_string(), "Harald Stu\u{308}rmer".to_string()];
        // Links to an untyped note named like a project don't count.
        let mut other = note("other.md", "Other", "");
        other.link = vec!["untyped".to_string()];
        let untyped = note("untyped.md", "Untyped", "");
        for n in [&tse, &harald, &meeting, &other, &untyped] {
            write_markdown_data_to_sqlite(n, &db_path)?;
        }
        let db = DatabaseService::new(db_path.to_str().unwrap());
        let attr = |k: &str, v: &str| SearchCriteria {
            attributes: vec![AttributePair::new(k, v)],
            database_path: db.database_path.clone(),
            ..Default::default()
        };

        assert_eq!(notes_for(&db, attr("project", "tse")), vec!["project/tse/meeting.md"]);
        assert_eq!(notes_for(&db, attr("project", "TSE")), vec!["project/tse/meeting.md"]);
        assert_eq!(
            notes_for(&db, attr("person", "harald stürmer")),
            vec!["project/tse/meeting.md"]
        );
        // Wrong type for the linked note, or an untyped target: no match.
        assert_eq!(
            notes_for(
                &db,
                SearchCriteria {
                    links: vec!["harald stürmer".to_string()],
                    ..Default::default()
                }
            ),
            vec!["project/tse/meeting.md"]
        );
        assert!(notes_for(&db, attr("person", "tse")).is_empty());
        assert!(notes_for(&db, attr("project", "untyped")).is_empty());
        assert_eq!(notes_for(&db, query("[project:tse]")), vec!["project/tse/meeting.md"]);

        let segments = db.search_segments(&attr("project", "tse"))?;
        assert!(!segments.is_empty());
        assert!(segments.iter().all(|s| s.filename == "project/tse/meeting.md"));
        Ok(())
    }

    #[test]
    fn test_links_resolve_through_aliases() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");
        let mut target = note("projects/ProjectX.md", "Project X", "");
        target
            .header
            .fields
            .insert("aliases".to_string(), serde_json::json!(["Project X", "PX"]));
        let mut by_alias = note("m.md", "M", "");
        by_alias.link = vec!["project x".to_string()];
        let mut by_name = note("n.md", "N", "");
        by_name.link = vec!["ProjectX".to_string()];
        let mut unrelated = note("o.md", "O", "");
        unrelated.link = vec!["Other".to_string()];
        for n in [&target, &by_alias, &by_name, &unrelated] {
            write_markdown_data_to_sqlite(n, &db_path)?;
        }
        let db = DatabaseService::new(db_path.to_str().unwrap());

        for link in ["ProjectX", "Project X", "px"] {
            let mut found = notes_for(
                &db,
                SearchCriteria {
                    links: vec![link.to_string()],
                    ..Default::default()
                },
            );
            found.sort();
            assert_eq!(found, vec!["m.md", "n.md"], "link {link}");
        }
        let mut backlinks = get_backlinks(&db_path, "projects/ProjectX.md")?;
        backlinks.sort();
        assert_eq!(backlinks, vec!["m.md", "n.md"]);
        Ok(())
    }

    #[test]
    fn test_tag_aliases_expand_both_ways() {
        let aliases = HashMap::from([("Meetings".to_string(), "meeting".to_string())]);
        let params = |tag: &str| -> Vec<String> {
            QueryBuilder::new()
                .with_tag_aliases(aliases.clone())
                .build_note_query(&SearchCriteria {
                    tags: vec![tag.to_string()],
                    ..Default::default()
                })
                .get_parameters()
                .into_iter()
                .map(|p| match p {
                    Parameter::Text(s) => s,
                    Parameter::Int(i) => i.to_string(),
                })
                .collect()
        };
        assert_eq!(params("meeting"), vec!["meeting", "meetings"]);
        assert_eq!(params("Meetings"), vec!["meetings", "meeting"]);
        assert_eq!(params("other"), vec!["other"]);
    }

    #[test]
    fn test_hybrid_search_without_embeddings_ranks_by_keywords(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");
        write_markdown_data_to_sqlite(
            &note("a.md", "A", "# Garden\n\nTomato plants need water.\n\n# Kitchen\n\nTomato soup with basil and tomato paste."),
            &db_path,
        )?;
        write_markdown_data_to_sqlite(&note("b.md", "B", "Nothing here"), &db_path)?;
        let db = DatabaseService::new(db_path.to_str().unwrap());
        if crate::embeddings::embeddings_enabled() {
            return Ok(()); // would call a live Ollama
        }
        let results = db.search_hybrid("tomato basil", &[], &[], 10)?;
        let lines: Vec<i32> = results.iter().map(|(s, _)| s.start_line).collect();
        assert_eq!(lines.len(), 2, "any-word match, b.md excluded");
        assert!(results[0].0.text.contains("Kitchen"), "both words rank first");
        Ok(())
    }

    #[test]
    fn test_search_reads_while_another_connection_is_writing(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");
        write_markdown_data_to_sqlite(&note("a.md", "A", "committed text"), &db_path)?;

        // Simulate `import --watch` in the middle of a long write.
        let writer = rusqlite::Connection::open(&db_path)?;
        crate::markdown_parser::init_database_schema(&writer)?;
        let mode: String = writer.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
        assert_eq!(mode, "wal");
        writer.execute_batch(
            "BEGIN IMMEDIATE; INSERT INTO markdown_data (filename, title, body, todo_count, link_count) VALUES ('b.md', 'B', 'uncommitted text', 0, 0);",
        )?;

        // A search doesn't wait or fail, and sees only committed data.
        let db = DatabaseService::new(db_path.to_str().unwrap());
        let started = std::time::Instant::now();
        assert_eq!(notes_for(&db, text("text")), vec!["a.md"]);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));

        writer.execute_batch("COMMIT")?;
        let mut found = notes_for(&db, text("text"));
        found.sort();
        assert_eq!(found, vec!["a.md", "b.md"]);
        Ok(())
    }
}
