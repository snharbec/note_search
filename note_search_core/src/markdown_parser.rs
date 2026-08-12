use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::LazyLock;
use std::time::SystemTime;

static TODO_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?m)^- \[([ xX])\] (.*)$").unwrap());
static PRIORITY_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"priority:\s*([A-Z])").unwrap());
static DUE_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"due:\s*(\d{4}-\d{2}-\d{2}|\d{8})").unwrap());
static TAG_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?:^|\s)#([A-Za-zäöüÄÖÜß][A-Za-zäöüÄÖÜß/_]*)").unwrap());
static TAG_ATTR_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"tag:\s*([a-zA-Z0-9_]+)").unwrap());
static LINK_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\[([^\]]+)\]\(([^)]+)\)").unwrap());
static WIKI_LINK_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\[\[([^\]]+)\]\]").unwrap());
static WIKI_LINK_FIELD_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^\[\[([^\]]+)\]\]$").unwrap());
static DATAVIEW_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?s)```dataview\n.*?```").unwrap());
static TASKS_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?s)```tasks\n.*?```").unwrap());
static HEADING_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^(#{1,6})\s+(.*)$").unwrap());
static FENCE_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^\s*```").unwrap());
static WIKI_DATE_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\[\[(\d{4}-\d{2}-\d{2})\]\]").unwrap());
static BARE_DATE_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\b(\d{4}-\d{2}-\d{2})\b").unwrap());

#[derive(Debug, Serialize, Deserialize)]
pub struct TodoEntry {
    pub closed: bool,
    pub priority: Option<String>,
    pub due: Option<String>,
    pub tags: Vec<String>,
    pub links: Vec<String>,
    pub line_number: usize,
    pub text: String,
    /// Timestamp (Unix seconds) for this todo, derived in priority order:
    /// 1. the todo's `due` date, 2. a date referenced in the todo text,
    /// 3. the note's `updated` frontmatter attribute, 4. the note's `created` attribute.
    pub updated: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Header {
    #[serde(flatten)]
    pub fields: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MarkdownData {
    pub filename: String,
    pub created: u64,
    pub updated: u64,
    pub title: String,
    pub header: Header,
    pub todo: Vec<TodoEntry>,
    pub link: Vec<String>,
    pub body: String,
    pub segments: Vec<Segment>,
}

pub fn remove_dataview_sections(content: &str) -> String {
    let without_dataview = DATAVIEW_REGEX.replace_all(content, "");
    TASKS_REGEX.replace_all(&without_dataview, "").to_string()
}

pub fn remove_hash_prefixes(content: &str) -> String {
    // Remove # prefixes from all values in the frontmatter content
    // This handles patterns like "key: #value" or "key: #value1 #value2 #value3"
    let mut result = String::new();

    for line in content.lines() {
        if let Some(colon_pos) = line.find(':') {
            // Get the key including the colon and any whitespace after it
            let before_value = &line[..colon_pos + 1];
            let rest = &line[colon_pos + 1..];

            // Find where the actual value starts (skip whitespace after colon)
            let value_start = rest.len() - rest.trim_start().len();
            let whitespace = &rest[..value_start];
            let value_part = &rest[value_start..];

            // Remove all # prefixes from the value part
            let cleaned_value = value_part
                .split_whitespace()
                .map(|word| {
                    if word.starts_with('#') {
                        &word[1..]
                    } else {
                        word
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");

            result.push_str(before_value);
            result.push_str(whitespace);
            result.push_str(&cleaned_value);
        } else {
            result.push_str(line);
        }
        result.push('\n');
    }

    // Remove trailing newline if original didn't have one
    if !content.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }

    result
}

/// Returns the byte offset immediately after the `n`th character of `s`, or
/// `None` if `s` has fewer than `n` characters. Unlike raw byte indexing
/// (`&s[..n]`), this never panics on non-ASCII input that has a multi-byte
/// character within the first `n` bytes.
fn char_boundary(s: &str, n: usize) -> Option<usize> {
    match s.char_indices().nth(n) {
        Some((byte_idx, _)) => Some(byte_idx),
        None => (s.chars().count() == n).then(|| s.len()),
    }
}

/// Returns the first `n` characters of `s`, or `None` if `s` has fewer than
/// `n` characters. See `char_boundary` for why this is char- rather than
/// byte-based.
fn char_prefix(s: &str, n: usize) -> Option<&str> {
    char_boundary(s, n).map(|idx| &s[..idx])
}

/// Extract the date part (YYYY-MM-DD) from a string
/// Supports formats like "YYYY-MM-DD", "[[YYYY-MM-DD]]", "YYYY-MM-DD HH:MM", etc.
pub fn extract_date_part(date_str: &str) -> Option<String> {
    let trimmed = date_str.trim();

    // Check for [[date]] format and extract it
    if trimmed.starts_with("[[") && trimmed.contains("]]") {
        if let Some(start) = trimmed.find("[[") {
            if let Some(end) = trimmed.find("]]") {
                let date_part = &trimmed[start + 2..end];
                if let Some(potential_date) = char_prefix(date_part, 10) {
                    if potential_date.chars().nth(4) == Some('-')
                        && potential_date.chars().nth(7) == Some('-')
                    {
                        return Some(potential_date.to_string());
                    }
                }
            }
        }
    } else if let Some(potential_date) = char_prefix(trimmed, 10) {
        // Check for yyyy-MM-dd format with optional time
        if potential_date.chars().nth(4) == Some('-') && potential_date.chars().nth(7) == Some('-')
        {
            return Some(potential_date.to_string());
        }
    }

    None
}

/// Parse a date string from frontmatter into a Unix timestamp (seconds since epoch)
/// Supports multiple formats:
/// - "[[yyyy-MM-dd]]" (e.g., "[[2024-08-05]]")
/// - "yyyy-MM-dd" (e.g., "2024-08-05")
/// - "[[yyyy-MM-dd]] hh:mm" (e.g., "[[2024-08-05]] 17:08")
/// - "yyyy-MM-dd hh:mm" (e.g., "2024-08-05 17:08")
/// - Unix timestamp (e.g., "1704067200")
pub fn parse_date_string(date_str: &str) -> Option<u64> {
    let trimmed = date_str.trim();

    // Try parsing as Unix timestamp (all digits)
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return trimmed.parse().ok();
    }

    // Extract date from [[yyyy-MM-dd]] format if present
    let mut date_part = trimmed;
    let mut time_part = "";

    // Check for [[date]] format and extract it
    if trimmed.starts_with("[[") && trimmed.contains("]]") {
        if let Some(start) = trimmed.find("[[") {
            if let Some(end) = trimmed.find("]]") {
                date_part = &trimmed[start + 2..end];
                // Check if there's a time part after ]]
                let after_brackets = &trimmed[end + 2..];
                if !after_brackets.is_empty() {
                    time_part = after_brackets.trim();
                }
            }
        }
    } else if let Some(split_idx) = char_boundary(trimmed, 10) {
        // Check for yyyy-MM-dd format with optional time
        let potential_date = &trimmed[..split_idx];
        if potential_date.chars().nth(4) == Some('-') && potential_date.chars().nth(7) == Some('-')
        {
            date_part = potential_date;
            time_part = trimmed[split_idx..].trim();
        }
    }

    // Parse the date part
    let date = chrono::NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()?;

    // Parse optional time part
    let (hour, minute) = if time_part.is_empty() {
        (0, 0)
    } else {
        // Try to parse hh:mm format
        let time_trimmed = time_part.trim();
        if time_trimmed.len() >= 5 && time_trimmed.chars().nth(2) == Some(':') {
            let hour_str = &time_trimmed[..2];
            let minute_str = &time_trimmed[3..5];
            let hour = hour_str.parse::<u32>().ok()?;
            let minute = minute_str.parse::<u32>().ok()?;
            (hour, minute)
        } else {
            (0, 0)
        }
    };

    let datetime = date.and_hms_opt(hour, minute, 0)?;
    Some(
        datetime
            .and_local_timezone(chrono::Local)
            .unwrap()
            .timestamp() as u64,
    )
}

pub fn extract_frontmatter(content: &str) -> Option<(String, String, usize)> {
    let lines: Vec<&str> = content.lines().collect();

    // Check if first line is "---"
    if lines.is_empty() || lines[0].trim() != "---" {
        return None;
    }

    // Count lines until we find the closing "---"
    let mut frontmatter_line_count = 1; // Start with 1 for the opening "---"
    let mut frontmatter_end = 0;

    for (i, line) in lines.iter().enumerate().skip(1) {
        frontmatter_line_count += 1;
        if line.trim() == "---" {
            frontmatter_end = i;
            break;
        }
    }

    // If we didn't find a closing "---", there's no valid frontmatter
    if frontmatter_end == 0 {
        return None;
    }

    // Extract frontmatter and body
    let frontmatter = lines[1..frontmatter_end].join("\n");
    let body = lines[frontmatter_end + 1..].join("\n");

    Some((frontmatter, body, frontmatter_line_count))
}

pub fn extract_title_from_filename(filename: &str) -> String {
    filename.trim_end_matches(".md").to_string()
}

pub fn extract_title_from_frontmatter(frontmatter_content: &str) -> Option<String> {
    let yaml = yaml_rust2::YamlLoader::load_from_str(frontmatter_content);
    if let Ok(yamls) = yaml {
        if let Some(yaml) = yamls.first() {
            if let Some(title) = yaml["title"].as_str() {
                return Some(title.to_string());
            }
        }
    }
    None
}

pub fn extract_todo_entries(
    markdown_content: &str,
    note_updated: Option<i64>,
    note_created: Option<i64>,
) -> Vec<TodoEntry> {
    let mut todos = Vec::new();
    let mut line_number = 0;

    for line in markdown_content.lines() {
        line_number += 1;
        if let Some(captures) = TODO_REGEX.captures(line) {
            let closed = captures[1].trim() == "x" || captures[1].trim() == "X";
            let content = captures[2].trim();

            let mut priority = None;
            let mut due = None;
            let mut tags = Vec::new();
            let mut links = Vec::new();

            if let Some(priority_match) = PRIORITY_REGEX.captures(content) {
                priority = Some(priority_match[1].to_string());
            }

            if let Some(due_match) = DUE_REGEX.captures(content) {
                let due_str = &due_match[1];
                // Normalize to YYYYMMDD format (remove dashes if present)
                let normalized_due = due_str.replace("-", "");
                due = Some(normalized_due);
            }

            for tag_capture in TAG_REGEX.captures_iter(content) {
                tags.push(tag_capture[1].to_lowercase());
            }

            for tag_capture in TAG_ATTR_REGEX.captures_iter(content) {
                tags.push(tag_capture[1].to_lowercase());
            }

            for link_capture in LINK_REGEX.captures_iter(content) {
                links.push(link_capture[2].to_lowercase());
            }

            for link_capture in WIKI_LINK_REGEX.captures_iter(content) {
                links.push(link_capture[1].to_lowercase());
            }

            todos.push(TodoEntry {
                closed,
                priority,
                due: due.clone(),
                tags,
                links,
                line_number,
                text: content.to_string(),
                updated: compute_todo_timestamp(
                    content,
                    due.as_deref(),
                    note_updated,
                    note_created,
                ),
            });
        }
    }

    todos
}

/// Compute the timestamp for a todo entry using the following priority:
/// 1. the todo's own `due` date,
/// 2. a date referenced inside the todo text (`[[YYYY-MM-DD]]` or bare `YYYY-MM-DD`),
/// 3. the surrounding note's `updated` frontmatter attribute,
/// 4. the surrounding note's `created` frontmatter attribute.
/// Returns 0 if none of the above yield a date.
fn compute_todo_timestamp(
    content: &str,
    due: Option<&str>,
    note_updated: Option<i64>,
    note_created: Option<i64>,
) -> i64 {
    // 1. Due date (stored as YYYYMMDD)
    if let Some(due_str) = due {
        if let Some(ts) = yyyymmdd_to_timestamp(due_str) {
            return ts;
        }
    }

    // 2. A date referenced inside the todo text
    if let Some(ts) = extract_date_from_text(content) {
        return ts;
    }

    // 3. Note's `updated` attribute
    if let Some(ts) = note_updated {
        return ts;
    }

    // 4. Note's `created` attribute
    if let Some(ts) = note_created {
        return ts;
    }

    0
}

/// Convert a `YYYYMMDD` (or `YYYY-MM-DD`) string to a Unix timestamp at midnight UTC.
fn yyyymmdd_to_timestamp(s: &str) -> Option<i64> {
    let normalized = s.replace('-', "");
    let date = chrono::NaiveDate::parse_from_str(&normalized, "%Y%m%d").ok()?;
    let dt = date.and_hms_opt(0, 0, 0)?;
    Some(dt.and_utc().timestamp())
}

/// Find the first date in `content` and return it as a Unix timestamp (midnight UTC).
/// Prefers `[[YYYY-MM-DD]]` wiki-link dates; otherwise picks the first bare
/// `YYYY-MM-DD` token that is not part of a larger wiki-link.
fn extract_date_from_text(content: &str) -> Option<i64> {
    if let Some(c) = WIKI_DATE_REGEX.captures(content) {
        if let Some(ts) = yyyymmdd_to_timestamp(&c[1]) {
            return Some(ts);
        }
    }

    for c in BARE_DATE_REGEX.captures_iter(content) {
        let m = c.get(0).unwrap();
        if is_inside_wiki_link(content, m.start(), m.end()) {
            continue;
        }
        if let Some(ts) = yyyymmdd_to_timestamp(&c[1]) {
            return Some(ts);
        }
    }

    None
}

/// Convert dates in YYYY-MM-DD format to wiki links [[YYYY-MM-DD]]
pub fn convert_dates_to_wiki_links(content: &str) -> String {
    // Pattern to match dates in YYYY-MM-DD format
    // Matches: word boundary + 4 digits + hyphen + 2 digits + hyphen + 2 digits + word boundary
    let date_regex = regex::Regex::new(r"\b(\d{4})-(\d{2})-(\d{2})\b").unwrap();

    let mut result = String::new();
    let mut last_end = 0;

    for caps in date_regex.captures_iter(content) {
        let mat = caps.get(0).unwrap();
        let start = mat.start();
        let end = mat.end();

        // Check if this date is already inside wiki links [[...]]
        // by looking for [[ before and ]] after the date
        let is_in_wiki_link = is_inside_wiki_link(content, start, end);

        // Add content before this match
        result.push_str(&content[last_end..start]);

        if is_in_wiki_link {
            // Keep the original date if it's already in a wiki link
            result.push_str(mat.as_str());
        } else {
            // Convert to wiki link
            let year = &caps[1];
            let month = &caps[2];
            let day = &caps[3];
            result.push_str(&format!("[[{}-{}-{}]]", year, month, day));
        }

        last_end = end;
    }

    // Add remaining content
    result.push_str(&content[last_end..]);

    result
}

/// Check if a position in content is inside a wiki link [[...]]
fn is_inside_wiki_link(content: &str, start: usize, end: usize) -> bool {
    // Look backwards for [[
    let before = &content[..start];
    let last_open = before.rfind("[[");
    let last_close = before.rfind("]]");

    // If we found [[ after the last ]], we might be inside a wiki link
    let inside_open_link = match (last_open, last_close) {
        (Some(open), Some(close)) => open > close,
        (Some(_), None) => true,
        _ => false,
    };

    if !inside_open_link {
        return false;
    }

    // Look forwards for ]]
    let after = &content[end..];
    let next_close = after.find("]]");
    let next_open = after.find("[[");

    // If we found ]] before the next [[, we're inside a wiki link
    match (next_close, next_open) {
        (Some(close), Some(open)) => close < open,
        (Some(_), None) => true,
        _ => false,
    }
}

pub fn extract_links(markdown_content: &str) -> Vec<String> {
    let mut links = Vec::new();
    for link_capture in LINK_REGEX.captures_iter(markdown_content) {
        links.push(link_capture[2].to_lowercase());
    }

    for link_capture in WIKI_LINK_REGEX.captures_iter(markdown_content) {
        links.push(link_capture[1].to_lowercase());
    }

    links
}

/// Extract `#tag`s from `text`, expanding `a/b/c` into `a`, `a/b`, `a/b/c`
/// (same hierarchy rule used for the note-level `tags` aggregate).
fn extract_tags_with_hierarchy(text: &str) -> HashSet<String> {
    let mut tags = HashSet::new();
    for tag_capture in TAG_REGEX.captures_iter(text) {
        let tag = tag_capture[1].to_lowercase();
        let mut parts: Vec<&str> = tag.split('/').collect();
        let mut current = String::new();
        while !parts.is_empty() {
            if current.is_empty() {
                current = parts.remove(0).to_string();
            } else {
                current.push('/');
                current.push_str(parts.remove(0));
            }
            tags.insert(current.clone());
        }
        tags.insert(tag);
    }
    tags
}

/// Transliterate German umlauts to their ASCII digraph counterparts.
fn transliterate_umlauts(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'ä' => result.push_str("ae"),
            'ö' => result.push_str("oe"),
            'ü' => result.push_str("ue"),
            'Ä' => result.push_str("Ae"),
            'Ö' => result.push_str("Oe"),
            'Ü' => result.push_str("Ue"),
            'ß' => result.push_str("ss"),
            _ => result.push(c),
        }
    }
    result
}

/// Recursively transliterate umlauts in every string within a JSON value
/// (arrays and nested objects included).
fn transliterate_json_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(s) => *s = transliterate_umlauts(s),
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                transliterate_json_value(item);
            }
        }
        serde_json::Value::Object(map) => {
            for v in map.values_mut() {
                transliterate_json_value(v);
            }
        }
        _ => {}
    }
}

/// Transliterate umlauts in every attribute *value* in `header_fields`.
/// Attribute keys are left untouched.
fn transliterate_attribute_values(header_fields: &mut HashMap<String, serde_json::Value>) {
    for value in header_fields.values_mut() {
        transliterate_json_value(value);
    }
}

/// Text below a markdown header, including the header itself, up to (but
/// not including) the next header of level <=4. Headers of level 5/6 don't
/// start a new segment - their line just becomes part of the enclosing
/// segment's text. Content before the first level-<=4 header (or in a
/// headerless document) is its own implicit root segment.
///
/// Tags/links are the union of: the segment's own text (header + body), its
/// ancestor headers' own text, and the whole document's aggregate tags/links
/// (`document_tags`/`document_links` passed into `extract_segments`) - so
/// every segment always carries the full document's tags/links plus
/// whatever's added by the headers above it. `breadcrumb` gives the
/// non-cascading, human-readable path (filename + ancestor header text) for
/// telling segments apart when their tag/link sets overlap.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Segment {
    pub start_line: usize,
    pub end_line: usize,
    pub heading_level: Option<u32>,
    pub text: String,
    pub breadcrumb: String,
    pub tags: Vec<String>,
    pub links: Vec<String>,
}

struct HeadingCrumb {
    level: u32,
    text: String,
    cascade_tags: HashSet<String>,
    cascade_links: HashSet<String>,
}

fn own_tags_links(text: &str) -> (Vec<String>, Vec<String>) {
    let tags: Vec<String> = extract_tags_with_hierarchy(text).into_iter().collect();
    let links = extract_links(text);
    (tags, links)
}

fn push_segment(
    segments: &mut Vec<Segment>,
    start_line: usize,
    lines: &[String],
    heading_level: Option<u32>,
    breadcrumb: &str,
    crumbs: &[HeadingCrumb],
    document_tags: &[String],
    document_links: &[String],
) {
    if lines.is_empty() {
        return;
    }
    let text = lines.join("\n");
    if text.trim().is_empty() {
        return;
    }
    let end_line = start_line + lines.len() - 1;
    let (mut tags, mut links) = own_tags_links(&text);
    if let Some(frame) = crumbs.last() {
        for t in &frame.cascade_tags {
            if !tags.contains(t) {
                tags.push(t.to_lowercase());
            }
        }
        for l in &frame.cascade_links {
            if !links.contains(l) {
                links.push(l.to_lowercase());
            }
        }
    }
    for t in document_tags {
        if !tags.contains(t) {
            tags.push(t.to_lowercase());
        }
    }
    for l in document_links {
        if !links.contains(l) {
            links.push(l.to_lowercase());
        }
    }
    segments.push(Segment {
        start_line,
        end_line,
        heading_level,
        text,
        breadcrumb: breadcrumb.to_string(),
        tags,
        links,
    });
}

/// Split a note body into header-anchored segments. `filename` is the
/// note's own relative path/title, used as the root of every segment's
/// breadcrumb. `document_tags`/`document_links` are the note's aggregate
/// tag/link set (same as what feeds `note_tags`/`note_links`) and are
/// unioned into every segment unconditionally. Line numbers are 1-based
/// relative to `body` - the caller is responsible for offsetting by the
/// frontmatter's line count, same as `extract_todo_entries`.
pub fn extract_segments(
    body: &str,
    filename: &str,
    document_tags: &[String],
    document_links: &[String],
) -> Vec<Segment> {
    let lines: Vec<&str> = body.lines().collect();
    let mut segments: Vec<Segment> = Vec::new();

    let mut crumbs: Vec<HeadingCrumb> = Vec::new();
    let mut in_fence = false;

    let mut current_start: usize = 1;
    let mut current_level: Option<u32> = None;
    let mut current_lines: Vec<String> = Vec::new();
    let mut current_breadcrumb: String = filename.to_string();

    for (idx, raw_line) in lines.iter().enumerate() {
        let line_number = idx + 1;
        let line = *raw_line;

        if FENCE_REGEX.is_match(line) {
            in_fence = !in_fence;
            current_lines.push(line.to_string());
            continue;
        }

        if !in_fence {
            if let Some(caps) = HEADING_REGEX.captures(line) {
                let level = caps[1].len() as u32;
                if level <= 4 {
                    push_segment(
                        &mut segments,
                        current_start,
                        &current_lines,
                        current_level,
                        &current_breadcrumb,
                        &crumbs,
                        document_tags,
                        document_links,
                    );

                    while let Some(top) = crumbs.last() {
                        if top.level >= level {
                            crumbs.pop();
                        } else {
                            break;
                        }
                    }

                    let mut parts: Vec<&str> = vec![filename];
                    parts.extend(crumbs.iter().map(|c| c.text.as_str()));
                    current_breadcrumb = parts.join(" > ");

                    let heading_text = caps[2].to_string();
                    let (own_tags, own_links) = own_tags_links(&heading_text);
                    let mut cascade_tags = crumbs
                        .last()
                        .map(|f| f.cascade_tags.clone())
                        .unwrap_or_default();
                    let mut cascade_links = crumbs
                        .last()
                        .map(|f| f.cascade_links.clone())
                        .unwrap_or_default();
                    cascade_tags.extend(own_tags);
                    cascade_links.extend(own_links);

                    crumbs.push(HeadingCrumb {
                        level,
                        text: heading_text,
                        cascade_tags,
                        cascade_links,
                    });

                    current_start = line_number;
                    current_level = Some(level);
                    current_lines = vec![line.to_string()];
                    continue;
                }
            }
        }

        current_lines.push(line.to_string());
    }

    push_segment(
        &mut segments,
        current_start,
        &current_lines,
        current_level,
        &current_breadcrumb,
        &crumbs,
        document_tags,
        document_links,
    );

    segments
}

pub fn yaml_to_json_value(value: &yaml_rust2::Yaml) -> serde_json::Value {
    match value {
        yaml_rust2::Yaml::Real(v) => serde_json::Value::String(v.to_string()),
        yaml_rust2::Yaml::Integer(v) => serde_json::Value::Number(serde_json::Number::from(*v)),
        yaml_rust2::Yaml::String(v) => {
            let v_str = v.as_str();
            if let Some(captures) = WIKI_LINK_FIELD_REGEX.captures(v_str) {
                serde_json::Value::String(captures[1].to_string())
            } else {
                serde_json::Value::String(v.clone())
            }
        }
        yaml_rust2::Yaml::Boolean(v) => serde_json::Value::Bool(*v),
        yaml_rust2::Yaml::Array(v) => {
            let mut vec = Vec::new();
            for item in v {
                vec.push(yaml_to_json_value(item));
            }
            serde_json::Value::Array(vec)
        }
        yaml_rust2::Yaml::Hash(v) => {
            let mut map = serde_json::Map::new();
            for (key, val) in v {
                if let Some(key_str) = key.as_str() {
                    map.insert(key_str.to_string(), yaml_to_json_value(val));
                }
            }
            serde_json::Value::Object(map)
        }
        yaml_rust2::Yaml::Null => serde_json::Value::Null,
        yaml_rust2::Yaml::Alias(_) | yaml_rust2::Yaml::BadValue => {
            serde_json::Value::String("".to_string())
        }
    }
}

/// Extract attributes from markdown headers that consist only of a list
pub fn extract_attributes_from_body(body: &str) -> HashMap<String, Vec<String>> {
    let mut attributes = HashMap::new();
    let lines: Vec<&str> = body.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();
        if line.starts_with('#') {
            // Found a header
            let header_name = line.trim_start_matches('#').trim().to_lowercase();
            if header_name.is_empty() || header_name.contains(' ') {
                i += 1;
                continue;
            }

            // Look ahead for list items
            let mut j = i + 1;
            let mut list_items = Vec::new();
            let mut valid_section = true;
            let mut found_list = false;

            while j < lines.len() {
                let next_line = lines[j].trim();
                if next_line.is_empty() {
                    j += 1;
                    continue;
                }
                if next_line.starts_with('#') {
                    // Next header found
                    break;
                }

                // Check if it's a list item
                if next_line.starts_with("- ")
                    || next_line.starts_with("* ")
                    || next_line.starts_with("+ ")
                {
                    found_list = true;
                    let item_content = next_line[2..].trim();
                    if item_content.is_empty() {
                        valid_section = false;
                        break;
                    }

                    // Check if it's a wiki link or one word
                    if let Some(captures) = WIKI_LINK_FIELD_REGEX.captures(item_content) {
                        list_items.push(captures[1].to_string());
                    } else if !item_content.contains(' ') {
                        list_items.push(item_content.to_string());
                    } else {
                        // Not a link and not one word
                        valid_section = false;
                        break;
                    }
                } else {
                    // Not a list item, not empty, and not a header -> invalidates the section
                    valid_section = false;
                    break;
                }
                j += 1;
            }

            if valid_section && found_list && !list_items.is_empty() {
                attributes.insert(header_name, list_items);
            }

            i = j;
        } else {
            i += 1;
        }
    }
    attributes
}

pub fn process_markdown_file(
    file_path: &Path,
    input_dir: &Path,
) -> Result<MarkdownData, Box<dyn std::error::Error>> {
    let mapping_config = crate::commands::mapping::MappingConfig::load();
    let jira_min_words = crate::commands::mapping::jira_segment_min_words();
    process_markdown_file_with_config(file_path, input_dir, &mapping_config, jira_min_words)
}

/// Same as [`process_markdown_file`], but takes an already-loaded mapping
/// config instead of reading it from disk. Callers processing many files in
/// one run (import, batch reparse) should load the config once and reuse it
/// here rather than paying a file read + parse per note.
pub fn process_markdown_file_with_config(
    file_path: &Path,
    input_dir: &Path,
    mapping_config: &crate::commands::mapping::MappingConfig,
    jira_min_words: usize,
) -> Result<MarkdownData, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(file_path)?;
    let relative_path = file_path
        .strip_prefix(input_dir)?
        .to_str()
        .unwrap_or("")
        .to_string();

    let (frontmatter_content, markdown_body, frontmatter_line_count) =
        match extract_frontmatter(&content) {
            Some((front, body, line_count)) => (front, body, line_count),
            None => (String::new(), content, 0),
        };

    let mut header_fields = HashMap::new();
    let mut frontmatter_links = Vec::new();
    if !frontmatter_content.is_empty() {
        for capture in WIKI_LINK_REGEX.captures_iter(&frontmatter_content) {
            let link = capture[1].to_lowercase();
            if !frontmatter_links.contains(&link) {
                frontmatter_links.push(link);
            }
        }

        let frontmatter_for_fields =
            remove_hash_prefixes(&frontmatter_content.replace("[[", "").replace("]]", ""));
        let yaml = yaml_rust2::YamlLoader::load_from_str(&frontmatter_for_fields);
        if let Ok(yamls) = yaml {
            if let Some(yaml) = yamls.first() {
                if let Some(hash) = yaml.as_hash() {
                    for (key, value) in hash {
                        if let Some(key_str) = key.as_str() {
                            if key_str.contains(' ') {
                                continue;
                            }
                            let val = yaml_to_json_value(value);
                            header_fields.insert(key_str.to_string(), val.clone());

                            // If this is a date field, extract the date as a link
                            if matches!(key_str, "created" | "changed" | "modified") {
                                if let Some(val_str) = val.as_str() {
                                    if let Some(date_link) = extract_date_part(val_str) {
                                        if !frontmatter_links.contains(&date_link) {
                                            frontmatter_links.push(date_link);
                                        }
                                    }
                                }
                            }

                            // Labels (e.g. JIRA-imported issues) are also
                            // links, lowercased to match note-naming
                            // convention: a "KRAMFORS" label links to
                            // [[kramfors]].
                            if key_str == "labels" {
                                let label_strs: Vec<&str> = match &val {
                                    serde_json::Value::String(s) => vec![s.as_str()],
                                    serde_json::Value::Array(arr) => {
                                        arr.iter().filter_map(|v| v.as_str()).collect()
                                    }
                                    _ => vec![],
                                };
                                for label in label_strs {
                                    let label_link = label.to_lowercase();
                                    if !frontmatter_links.contains(&label_link) {
                                        frontmatter_links.push(label_link);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let title =
        extract_title_from_frontmatter(&frontmatter_content.replace("[[", "").replace("]]", ""))
            .unwrap_or_else(|| extract_title_from_filename(&relative_path));

    // Remove Dataview sections before parsing content
    let body_without_dataview = remove_dataview_sections(&markdown_body);

    // Extract attributes from body headings and merge with header_fields
    let body_attributes = extract_attributes_from_body(&body_without_dataview);
    for (key, values) in body_attributes {
        let entry = header_fields
            .entry(key)
            .or_insert(serde_json::Value::Array(Vec::new()));

        if !entry.is_array() {
            let old_val = entry.clone();
            *entry = serde_json::Value::Array(vec![old_val]);
        }

        if let Some(arr) = entry.as_array_mut() {
            for val in values {
                let json_val = serde_json::Value::String(val);
                if !arr.contains(&json_val) {
                    arr.push(json_val);
                }
            }
        }
    }

    // Apply attribute mappings from configuration
    mapping_config.apply_to_attributes(&mut header_fields);

    // Transliterate German umlauts in attribute values to their ASCII
    // counterparts (ä->ae, ö->oe, ü->ue, ß->ss) so stored values are
    // consistently ASCII regardless of how the frontmatter was typed.
    transliterate_attribute_values(&mut header_fields);

    // Convert dates to wiki links before extracting links
    let body_with_date_links = convert_dates_to_wiki_links(&body_without_dataview);

    // Derive per-todo timestamps from the note's `updated`/`created` frontmatter
    // attributes (used as fallbacks by `extract_todo_entries`).
    let note_updated = header_fields
        .get("updated")
        .and_then(|v| v.as_str())
        .and_then(parse_date_string)
        .map(|t| t as i64);
    let note_created = header_fields
        .get("created")
        .and_then(|v| v.as_str())
        .and_then(parse_date_string)
        .map(|t| t as i64);

    let mut todos = extract_todo_entries(&body_without_dataview, note_updated, note_created);

    // Adjust line numbers to account for frontmatter
    for todo in &mut todos {
        todo.line_number += frontmatter_line_count;
    }

    let mut body_links = extract_links(&body_with_date_links);

    body_links.extend(frontmatter_links);
    let mut seen = HashSet::new();
    let unique_links: Vec<String> = body_links
        .into_iter()
        .filter(|link| seen.insert(link.clone()))
        .collect();

    // Same aggregate every segment inherits "from the full document" -
    // mirrors the `all_tags` computation in `write_markdown_data_to_sqlite_with_conn`.
    let mut document_tags: HashSet<String> = HashSet::new();
    for todo in &todos {
        for tag in &todo.tags {
            document_tags.insert(tag.clone());
        }
    }
    document_tags.extend(extract_tags_with_hierarchy(&body_without_dataview));
    let document_tags: Vec<String> = document_tags.into_iter().collect();

    let mut segments = extract_segments(
        &body_without_dataview,
        &relative_path,
        &document_tags,
        &unique_links,
    );
    // JIRA notes (saved under a "jira/" folder by the JIRA import) tend to
    // produce many short, low-content segments (e.g. a status line or a
    // one-line comment) that just add noise to segment search - drop those
    // below a configurable word-count threshold.
    if relative_path.starts_with("jira/") {
        segments.retain(|segment| segment.text.split_whitespace().count() >= jira_min_words);
    }
    for segment in &mut segments {
        segment.start_line += frontmatter_line_count;
        segment.end_line += frontmatter_line_count;
    }

    // Get updated timestamp: prefer frontmatter `updated` field, fall back to file modified time
    let updated = header_fields
        .get("updated")
        .and_then(|v| v.as_str())
        .and_then(parse_date_string)
        .unwrap_or_else(|| {
            // Fall back to file modified time
            fs::metadata(file_path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0)
        });

    // Get created timestamp: prefer frontmatter `created` field, fall back to file birth time
    let created = header_fields
        .get("created")
        .and_then(|v| v.as_str())
        .and_then(parse_date_string)
        .or_else(|| {
            // Fall back to file birth time
            fs::metadata(file_path)
                .ok()
                .and_then(|m| m.created().ok())
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
        })
        .unwrap_or(updated); // Fall back to updated if all else fails

    Ok(MarkdownData {
        filename: relative_path,
        created,
        updated,
        title,
        header: Header {
            fields: header_fields,
        },
        todo: todos,
        link: unique_links,
        body: body_without_dataview,
        segments,
    })
}

pub fn init_database_schema(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS markdown_data (
            filename TEXT PRIMARY KEY,
            created INTEGER,
            updated INTEGER,
            title TEXT,
            todo_count INTEGER,
            link_count INTEGER,
            header_fields TEXT,
            links TEXT,
            body TEXT,
            tags TEXT
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS todo_entries (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            filename TEXT,
            closed BOOLEAN,
            priority TEXT,
            due TEXT,
            text TEXT,
            tags TEXT,
            links TEXT,
            line_number INTEGER,
            FOREIGN KEY (filename) REFERENCES markdown_data(filename)
        )",
        [],
    )?;

    let _ = conn.execute("ALTER TABLE markdown_data ADD COLUMN created INTEGER", []);
    let _ = conn.execute("ALTER TABLE markdown_data ADD COLUMN tags TEXT", []);
    let _ = conn.execute("ALTER TABLE todo_entries ADD COLUMN updated INTEGER", []);

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_todo_entries_filename ON todo_entries(filename)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_todo_entries_closed ON todo_entries(closed)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_todo_entries_priority ON todo_entries(priority)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_todo_entries_due ON todo_entries(due)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_markdown_data_filename ON markdown_data(filename)",
        [],
    )?;

    // Normalized tag/link junction tables, queried instead of the JSON
    // `tags`/`links` columns above (which remain for output/--format use).
    conn.execute(
        "CREATE TABLE IF NOT EXISTS note_tags (
            filename TEXT NOT NULL,
            tag TEXT NOT NULL,
            PRIMARY KEY (filename, tag)
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS note_links (
            filename TEXT NOT NULL,
            link TEXT NOT NULL,
            PRIMARY KEY (filename, link)
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS todo_tags (
            todo_id INTEGER NOT NULL,
            tag TEXT NOT NULL,
            PRIMARY KEY (todo_id, tag)
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS todo_links (
            todo_id INTEGER NOT NULL,
            link TEXT NOT NULL,
            PRIMARY KEY (todo_id, link)
        )",
        [],
    )?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_note_tags_tag ON note_tags(tag)", [])?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_note_links_link ON note_links(link)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_todo_tags_todo_id ON todo_tags(todo_id)",
        [],
    )?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_todo_tags_tag ON todo_tags(tag)", [])?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_todo_links_todo_id ON todo_links(todo_id)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_todo_links_link ON todo_links(link)",
        [],
    )?;

    // Segment-level index: header-anchored sections (header + everything
    // below it up to the next level-<=4 header), with tags/links resolved as
    // the union of the segment's own text, its ancestor headers' own text,
    // and the whole document's aggregate tags/links (`segment_attributes`
    // likewise mirrors the whole document's frontmatter onto every segment -
    // attributes have no per-heading concept). `breadcrumb` is the
    // non-cascading filename + ancestor-header path, for telling segments
    // with overlapping tags/links apart. `embedding` is a little-endian f32
    // BLOB from the local Ollama `nomic-embed-text` model, recomputed at
    // write time only when a segment's text has changed since the previous
    // import (see `write_markdown_data_to_sqlite_with_conn`).
    // No backfill is possible here (unlike note_tags/note_links above) -
    // there's no prior per-line data to reconstruct this from, so this table
    // stays empty for existing notes until they're re-imported.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS segments (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            filename TEXT NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            heading_level INTEGER,
            text TEXT NOT NULL,
            breadcrumb TEXT NOT NULL,
            embedding BLOB
        )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_segments_filename ON segments(filename)",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS segment_tags (
            segment_id INTEGER NOT NULL,
            tag TEXT NOT NULL,
            PRIMARY KEY (segment_id, tag)
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS segment_links (
            segment_id INTEGER NOT NULL,
            link TEXT NOT NULL,
            PRIMARY KEY (segment_id, link)
        )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_segment_tags_tag ON segment_tags(tag)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_segment_tags_segment_id ON segment_tags(segment_id)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_segment_links_link ON segment_links(link)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_segment_links_segment_id ON segment_links(segment_id)",
        [],
    )?;
    // Every segment's copy of the whole document's frontmatter attributes -
    // there's no per-heading attribute concept, so this is a flat mirror of
    // the note's `header_fields`, one row per (key, value) pair (arrays
    // expand to one row per element).
    conn.execute(
        "CREATE TABLE IF NOT EXISTS segment_attributes (
            segment_id INTEGER NOT NULL,
            key TEXT NOT NULL,
            value TEXT NOT NULL,
            PRIMARY KEY (segment_id, key, value)
        )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_segment_attributes_segment_id ON segment_attributes(segment_id)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_segment_attributes_key ON segment_attributes(key)",
        [],
    )?;

    // One-time backfill from the existing JSON columns, for databases created
    // before the junction tables existed. Tracked via a marker row rather than
    // "are the tables empty" so a vault with genuinely zero tags/links doesn't
    // re-scan the whole database on every invocation.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_meta (key TEXT PRIMARY KEY, value TEXT)",
        [],
    )?;
    let already_backfilled: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM schema_meta WHERE key = 'tags_links_backfilled')",
        [],
        |row| row.get(0),
    )?;
    if !already_backfilled {
        conn.execute(
            "INSERT OR IGNORE INTO note_tags (filename, tag)
             SELECT filename, value FROM markdown_data, json_each(markdown_data.tags)
             WHERE tags IS NOT NULL",
            [],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO note_links (filename, link)
             SELECT filename, value FROM markdown_data, json_each(markdown_data.links)
             WHERE links IS NOT NULL",
            [],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO todo_tags (todo_id, tag)
             SELECT todo_entries.id, json_each.value FROM todo_entries, json_each(todo_entries.tags)
             WHERE tags IS NOT NULL",
            [],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO todo_links (todo_id, link)
             SELECT todo_entries.id, json_each.value FROM todo_entries, json_each(todo_entries.links)
             WHERE links IS NOT NULL",
            [],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_meta (key, value) VALUES ('tags_links_backfilled', '1')",
            [],
        )?;
    }

    Ok(())
}

pub fn write_markdown_data_to_sqlite_with_conn(
    data: &MarkdownData,
    conn: &rusqlite::Connection,
) -> Result<(), Box<dyn std::error::Error>> {
    let header_json = serde_json::to_string(&data.header.fields)?;
    let links_json = serde_json::to_string(&data.link)?;

    let mut all_tags: HashSet<String> = HashSet::new();
    for todo in &data.todo {
        for tag in &todo.tags {
            all_tags.insert(tag.clone());
        }
    }
    all_tags.extend(extract_tags_with_hierarchy(&data.body));
    let tags_json = serde_json::to_string(&all_tags.iter().cloned().collect::<Vec<_>>())?;

    // Junction-table deletes must happen before the rows they reference are
    // deleted below, since the todo_id join info disappears otherwise.
    conn.prepare_cached(
        "DELETE FROM todo_tags WHERE todo_id IN (SELECT id FROM todo_entries WHERE filename = ?1)",
    )?
    .execute(rusqlite::params![data.filename])?;
    conn.prepare_cached(
        "DELETE FROM todo_links WHERE todo_id IN (SELECT id FROM todo_entries WHERE filename = ?1)",
    )?
    .execute(rusqlite::params![data.filename])?;

    conn.prepare_cached("DELETE FROM todo_entries WHERE filename = ?1")?
        .execute(rusqlite::params![data.filename])?;

    conn.prepare_cached("DELETE FROM note_tags WHERE filename = ?1")?
        .execute(rusqlite::params![data.filename])?;
    conn.prepare_cached("DELETE FROM note_links WHERE filename = ?1")?
        .execute(rusqlite::params![data.filename])?;

    // Carry over embeddings for segments whose text hasn't changed since the
    // previous import, so unchanged segments skip the Ollama call below.
    // Segment ids aren't stable across imports (the table is fully
    // delete-then-reinserted per file), so exact text is the reuse key.
    let mut previous_embeddings: HashMap<String, Vec<u8>> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT text, embedding FROM segments WHERE filename = ?1 AND embedding IS NOT NULL",
        )?;
        let rows = stmt.query_map(rusqlite::params![data.filename], |row| {
            let text: String = row.get(0)?;
            let embedding: Vec<u8> = row.get(1)?;
            Ok((text, embedding))
        })?;
        for row in rows {
            let (text, embedding) = row?;
            previous_embeddings.insert(text, embedding);
        }
    }

    conn.prepare_cached(
        "DELETE FROM segment_tags WHERE segment_id IN (SELECT id FROM segments WHERE filename = ?1)",
    )?
    .execute(rusqlite::params![data.filename])?;
    conn.prepare_cached(
        "DELETE FROM segment_links WHERE segment_id IN (SELECT id FROM segments WHERE filename = ?1)",
    )?
    .execute(rusqlite::params![data.filename])?;
    conn.prepare_cached(
        "DELETE FROM segment_attributes WHERE segment_id IN (SELECT id FROM segments WHERE filename = ?1)",
    )?
    .execute(rusqlite::params![data.filename])?;
    conn.prepare_cached("DELETE FROM segments WHERE filename = ?1")?
        .execute(rusqlite::params![data.filename])?;

    conn.prepare_cached(
        "INSERT OR REPLACE INTO markdown_data
         (filename, created, updated, title, todo_count, link_count, header_fields, links, body, tags)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?
    .execute(rusqlite::params![
        data.filename,
        data.created as i64,
        data.updated as i64,
        data.title,
        data.todo.len() as i64,
        data.link.len() as i64,
        header_json,
        links_json,
        data.body,
        tags_json
    ])?;

    {
        let mut note_tag_stmt =
            conn.prepare_cached("INSERT OR IGNORE INTO note_tags (filename, tag) VALUES (?1, ?2)")?;
        for tag in &all_tags {
            note_tag_stmt.execute(rusqlite::params![data.filename, tag])?;
        }
    }
    {
        let mut note_link_stmt = conn
            .prepare_cached("INSERT OR IGNORE INTO note_links (filename, link) VALUES (?1, ?2)")?;
        for link in &data.link {
            note_link_stmt.execute(rusqlite::params![data.filename, link])?;
        }
    }

    for todo in &data.todo {
        let tags_json = serde_json::to_string(&todo.tags)?;
        let links_json = serde_json::to_string(&todo.links)?;

        conn.prepare_cached(
            "INSERT INTO todo_entries
             (filename, closed, priority, due, text, tags, links, line_number, updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?
        .execute(rusqlite::params![
            data.filename,
            todo.closed,
            todo.priority.as_deref(),
            todo.due.as_deref(),
            todo.text,
            tags_json,
            links_json,
            todo.line_number as i64,
            todo.updated
        ])?;

        let todo_id = conn.last_insert_rowid();
        {
            let mut todo_tag_stmt = conn
                .prepare_cached("INSERT OR IGNORE INTO todo_tags (todo_id, tag) VALUES (?1, ?2)")?;
            for tag in &todo.tags {
                todo_tag_stmt.execute(rusqlite::params![todo_id, tag])?;
            }
        }
        {
            let mut todo_link_stmt = conn.prepare_cached(
                "INSERT OR IGNORE INTO todo_links (todo_id, link) VALUES (?1, ?2)",
            )?;
            for link in &todo.links {
                todo_link_stmt.execute(rusqlite::params![todo_id, link])?;
            }
        }
    }

    let document_attributes = flatten_attributes(&data.header.fields);
    let embeddings_enabled = crate::embeddings::embeddings_enabled();

    for segment in &data.segments {
        let embedding_bytes: Option<Vec<u8>> = match previous_embeddings.get(&segment.text) {
            Some(bytes) => Some(bytes.clone()),
            None if embeddings_enabled => match crate::embeddings::embed_text(&segment.text) {
                Ok(vector) => Some(crate::embeddings::embedding_to_bytes(&vector)),
                Err(e) => {
                    eprintln!(
                        "Warning: failed to compute embedding for {}:{}: {}",
                        data.filename, segment.start_line, e
                    );
                    None
                }
            },
            None => None,
        };

        conn.prepare_cached(
            "INSERT INTO segments (filename, start_line, end_line, heading_level, text, breadcrumb, embedding)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?
        .execute(rusqlite::params![
            data.filename,
            segment.start_line as i64,
            segment.end_line as i64,
            segment.heading_level.map(|l| l as i64),
            segment.text,
            segment.breadcrumb,
            embedding_bytes,
        ])?;

        let segment_id = conn.last_insert_rowid();
        {
            let mut segment_tag_stmt = conn.prepare_cached(
                "INSERT OR IGNORE INTO segment_tags (segment_id, tag) VALUES (?1, ?2)",
            )?;
            for tag in &segment.tags {
                segment_tag_stmt.execute(rusqlite::params![segment_id, tag])?;
            }
        }
        {
            let mut segment_link_stmt = conn.prepare_cached(
                "INSERT OR IGNORE INTO segment_links (segment_id, link) VALUES (?1, ?2)",
            )?;
            for link in &segment.links {
                segment_link_stmt.execute(rusqlite::params![segment_id, link])?;
            }
        }
        {
            let mut segment_attr_stmt = conn.prepare_cached(
                "INSERT OR IGNORE INTO segment_attributes (segment_id, key, value) VALUES (?1, ?2, ?3)",
            )?;
            for (key, value) in &document_attributes {
                segment_attr_stmt.execute(rusqlite::params![segment_id, key, value])?;
            }
        }
    }

    Ok(())
}

/// Flatten a note's frontmatter into (key, value) pairs, for mirroring onto
/// every one of its segments in `segment_attributes`. Arrays expand to one
/// row per element; objects and nulls have no scalar value to store and are
/// skipped (matching how `[attr:value]` query matching already treats them).
fn flatten_attributes(fields: &HashMap<String, serde_json::Value>) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for (key, value) in fields {
        match value {
            serde_json::Value::String(s) => pairs.push((key.clone(), s.clone())),
            serde_json::Value::Number(n) => pairs.push((key.clone(), n.to_string())),
            serde_json::Value::Bool(b) => pairs.push((key.clone(), b.to_string())),
            serde_json::Value::Array(items) => {
                for item in items {
                    match item {
                        serde_json::Value::String(s) => pairs.push((key.clone(), s.clone())),
                        serde_json::Value::Number(n) => pairs.push((key.clone(), n.to_string())),
                        serde_json::Value::Bool(b) => pairs.push((key.clone(), b.to_string())),
                        _ => {}
                    }
                }
            }
            serde_json::Value::Object(_) | serde_json::Value::Null => {}
        }
    }
    pairs
}

pub fn write_markdown_data_to_sqlite(
    data: &MarkdownData,
    db_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use rusqlite::Connection;

    let conn = Connection::open(db_path)?;
    init_database_schema(&conn)?;
    write_markdown_data_to_sqlite_with_conn(data, &conn)?;
    Ok(())
}

/// Remove notes from the database that no longer exist on the filesystem
pub fn remove_orphaned_notes(
    input_dir: &Path,
    conn: &rusqlite::Connection,
) -> Result<usize, Box<dyn std::error::Error>> {
    // Get all filenames currently in the database
    let mut stmt = conn.prepare("SELECT filename FROM markdown_data")?;
    let db_filenames: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(Result::ok)
        .collect();

    let mut removed_count = 0;
    for filename in db_filenames {
        let file_path = input_dir.join(&filename);
        if !file_path.exists() {
            conn.execute(
                "DELETE FROM todo_tags WHERE todo_id IN (SELECT id FROM todo_entries WHERE filename = ?1)",
                rusqlite::params![filename],
            )?;
            conn.execute(
                "DELETE FROM todo_links WHERE todo_id IN (SELECT id FROM todo_entries WHERE filename = ?1)",
                rusqlite::params![filename],
            )?;
            conn.execute(
                "DELETE FROM todo_entries WHERE filename = ?1",
                rusqlite::params![filename],
            )?;
            conn.execute(
                "DELETE FROM note_tags WHERE filename = ?1",
                rusqlite::params![filename],
            )?;
            conn.execute(
                "DELETE FROM note_links WHERE filename = ?1",
                rusqlite::params![filename],
            )?;
            conn.execute(
                "DELETE FROM segment_tags WHERE segment_id IN (SELECT id FROM segments WHERE filename = ?1)",
                rusqlite::params![filename],
            )?;
            conn.execute(
                "DELETE FROM segment_links WHERE segment_id IN (SELECT id FROM segments WHERE filename = ?1)",
                rusqlite::params![filename],
            )?;
            conn.execute(
                "DELETE FROM segment_attributes WHERE segment_id IN (SELECT id FROM segments WHERE filename = ?1)",
                rusqlite::params![filename],
            )?;
            conn.execute(
                "DELETE FROM segments WHERE filename = ?1",
                rusqlite::params![filename],
            )?;
            conn.execute(
                "DELETE FROM markdown_data WHERE filename = ?1",
                rusqlite::params![filename],
            )?;
            removed_count += 1;
        }
    }

    Ok(removed_count)
}

/// Summary of an `update_files_in_db` invocation.
#[derive(Debug, Default, Clone)]
pub struct UpdateSummary {
    /// Number of files that were re-parsed and written to the database.
    pub updated: usize,
    /// Number of files whose database rows were removed because the file
    /// no longer exists on disk.
    pub removed: usize,
    /// Per-file errors encountered while processing. The string is the
    /// relative filename, and the value is the error message.
    pub errors: Vec<(String, String)>,
}

/// Re-parse the given files and refresh all derived database state
/// (`markdown_data` row and its `todo_entries`).
///
/// For each entry in `filenames` (a relative path under `input_dir`):
///   - if the file exists on disk, it is parsed via `process_markdown_file`
///     and written to the database (the `markdown_data` row is upserted and
///     its `todo_entries` are replaced),
///   - if the file does not exist on disk, its existing database rows
///     (`markdown_data` + `todo_entries`) are removed.
///
/// The caller is responsible for wrapping the call in a transaction if
/// atomicity across multiple files is desired; this function does not
/// open or commit a transaction itself, but executes its writes on `conn`.
///
/// `filenames` should be the same relative paths that are stored in the
/// `markdown_data.filename` column (i.e. paths relative to `input_dir`).
pub fn update_files_in_db(
    filenames: &[String],
    input_dir: &Path,
    conn: &rusqlite::Connection,
) -> Result<UpdateSummary, Box<dyn std::error::Error>> {
    let mut summary = UpdateSummary::default();
    let mapping_config = crate::commands::mapping::MappingConfig::load();
    let jira_min_words = crate::commands::mapping::jira_segment_min_words();

    for filename in filenames {
        let file_path = input_dir.join(filename);

        if !file_path.exists() {
            conn.execute(
                "DELETE FROM todo_entries WHERE filename = ?1",
                rusqlite::params![filename],
            )?;
            let removed = conn.execute(
                "DELETE FROM markdown_data WHERE filename = ?1",
                rusqlite::params![filename],
            )?;
            if removed > 0 {
                summary.removed += 1;
            }
            continue;
        }

        match process_markdown_file_with_config(
            &file_path,
            input_dir,
            &mapping_config,
            jira_min_words,
        ) {
            Ok(data) => {
                if let Err(e) = write_markdown_data_to_sqlite_with_conn(&data, conn) {
                    summary.errors.push((filename.clone(), e.to_string()));
                } else {
                    summary.updated += 1;
                }
            }
            Err(e) => {
                summary.errors.push((filename.clone(), e.to_string()));
            }
        }
    }

    Ok(summary)
}

pub fn parse_markdown_directory_batch(
    input_dir: &Path,
    db_path: &Path,
) -> Result<usize, Box<dyn std::error::Error>> {
    use rusqlite::Connection;

    let mut conn = Connection::open(db_path)?;
    init_database_schema(&conn)?;

    let tx = conn.transaction()?;
    let mut count = 0;
    let mapping_config = crate::commands::mapping::MappingConfig::load();
    let jira_min_words = crate::commands::mapping::jira_segment_min_words();

    for entry in walkdir::WalkDir::new(input_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|ext| ext == "md"))
    {
        let data = process_markdown_file_with_config(
            entry.path(),
            input_dir,
            &mapping_config,
            jira_min_words,
        )?;
        write_markdown_data_to_sqlite_with_conn(&data, &tx)?;
        count += 1;
    }

    tx.commit()?;

    // Remove notes that no longer exist on the filesystem
    let removed = remove_orphaned_notes(input_dir, &conn)?;
    if removed > 0 {
        println!("Removed {} orphaned notes from database", removed);
    }

    Ok(count)
}

pub fn parse_markdown_directory(
    input_dir: &Path,
    db_path: &Path,
) -> Result<usize, Box<dyn std::error::Error>> {
    parse_markdown_directory_batch(input_dir, db_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_extract_segments_root_segment_before_first_heading() {
        let body = "Intro paragraph before any heading.\n\n# First Heading\n\nBody text.\n";
        let segments = extract_segments(body, "notes/a.md", &[], &[]);

        assert_eq!(segments.len(), 2);

        let root = &segments[0];
        assert_eq!(root.heading_level, None);
        assert_eq!(root.start_line, 1);
        assert_eq!(root.end_line, 2);
        assert_eq!(root.text, "Intro paragraph before any heading.\n");
        assert_eq!(root.breadcrumb, "notes/a.md");

        let first = &segments[1];
        assert_eq!(first.heading_level, Some(1));
        assert_eq!(first.text, "# First Heading\n\nBody text.");
        assert_eq!(first.breadcrumb, "notes/a.md");
    }

    #[test]
    fn test_extract_segments_no_heading_at_all() {
        let body = "Just a plain note with no headings at all.\n";
        let segments = extract_segments(body, "notes/plain.md", &[], &[]);

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].heading_level, None);
        assert_eq!(segments[0].breadcrumb, "notes/plain.md");
    }

    #[test]
    fn test_extract_segments_empty_body_yields_no_segments() {
        let segments = extract_segments("", "notes/empty.md", &[], &[]);
        assert!(segments.is_empty());

        let segments = extract_segments("\n\n   \n", "notes/blank.md", &[], &[]);
        assert!(segments.is_empty());
    }

    #[test]
    fn test_extract_segments_own_text_includes_header_and_body() {
        let body = "# Project #alpha\n\nSome text with [[ProjectX]] and #beta.\n";
        let segments = extract_segments(body, "notes/a.md", &[], &[]);

        assert_eq!(segments.len(), 1);
        let seg = &segments[0];
        assert_eq!(
            seg.text,
            "# Project #alpha\n\nSome text with [[ProjectX]] and #beta."
        );
        assert!(seg.tags.contains(&"alpha".to_string()));
        assert!(seg.tags.contains(&"beta".to_string()));
        assert!(seg.links.contains(&"projectx".to_string()));
    }

    #[test]
    fn test_extract_segments_cascades_from_ancestor_headers() {
        // A child segment inherits its parent heading's #tag.
        let body = "\
# Section A #alpha

Text under A.

## Section B

Text under B, inherits from its parent section.

# Section C

Text under C, a sibling section that starts fresh.
";
        let segments = extract_segments(body, "notes/a.md", &[], &[]);

        let section_a = segments
            .iter()
            .find(|s| s.heading_level == Some(1) && s.text.contains("Section A"))
            .unwrap();
        assert!(section_a.tags.contains(&"alpha".to_string()));

        let section_b = segments
            .iter()
            .find(|s| s.heading_level == Some(2))
            .unwrap();
        assert!(section_b.tags.contains(&"alpha".to_string()));

        let section_c = segments
            .iter()
            .find(|s| s.heading_level == Some(1) && s.text.contains("Section C"))
            .unwrap();
        assert!(!section_c.tags.contains(&"alpha".to_string()));
    }

    #[test]
    fn test_extract_segments_cascades_from_document_tags_and_links() {
        let body = "# Section\n\nPlain text, no tags or links of its own.\n";
        let document_tags = vec!["urgent".to_string()];
        let document_links = vec!["ProjectX".to_string()];
        let segments = extract_segments(body, "notes/a.md", &document_tags, &document_links);

        assert_eq!(segments.len(), 1);
        assert!(segments[0].tags.contains(&"urgent".to_string()));
        assert!(segments[0].links.contains(&"projectx".to_string()));
    }

    #[test]
    fn test_extract_segments_breadcrumb_propagation() {
        let body = "\
# Top

## Middle

### Deep

Text in the deepest section.
";
        let segments = extract_segments(body, "notes/a.md", &[], &[]);

        let top = segments.iter().find(|s| s.heading_level == Some(1)).unwrap();
        assert_eq!(top.breadcrumb, "notes/a.md");

        let middle = segments.iter().find(|s| s.heading_level == Some(2)).unwrap();
        assert_eq!(middle.breadcrumb, "notes/a.md > Top");

        let deep = segments.iter().find(|s| s.heading_level == Some(3)).unwrap();
        assert_eq!(deep.breadcrumb, "notes/a.md > Top > Middle");
    }

    #[test]
    fn test_extract_segments_breadcrumb_resets_for_sibling() {
        let body = "\
# Top

## First Child

Text A.

## Second Child

Text B.
";
        let segments = extract_segments(body, "notes/a.md", &[], &[]);

        let second_child = segments
            .iter()
            .find(|s| s.text.starts_with("## Second Child"))
            .unwrap();
        assert_eq!(second_child.breadcrumb, "notes/a.md > Top");
    }

    #[test]
    fn test_extract_segments_headers_above_level_4_do_not_split() {
        let body = "\
#### Level Four

##### Level Five stays inside

Still inside the same segment.

###### Level Six too

Also inside.
";
        let segments = extract_segments(body, "notes/a.md", &[], &[]);

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].heading_level, Some(4));
        assert!(segments[0].text.contains("Level Five stays inside"));
        assert!(segments[0].text.contains("Level Six too"));
    }

    #[test]
    fn test_extract_segments_includes_fenced_code_without_splitting() {
        let body = "# Title\n\nBefore.\n\n```bash\n# not a heading\necho hi\n```\n\nAfter.\n";
        let segments = extract_segments(body, "notes/a.md", &[], &[]);

        assert_eq!(segments.len(), 1);
        assert!(segments[0].text.contains("# not a heading"));
        assert!(segments[0].text.contains("echo hi"));
        assert!(segments[0].text.contains("After."));
    }

    #[test]
    fn test_extract_frontmatter_with_valid_frontmatter() {
        let content = "---\ntitle: Test\n---\n# Body\nSome text";
        let result = extract_frontmatter(content);
        assert!(result.is_some());
        let (front, body, line_count) = result.unwrap();
        // Frontmatter content without the delimiters
        assert_eq!(front, "title: Test");
        // Body should be everything after the frontmatter
        assert_eq!(body, "# Body\nSome text");
        // line count: opening --- (1) + title line (1) + closing --- (1) = 3
        assert_eq!(line_count, 3);
    }

    #[test]
    fn test_extract_frontmatter_without_frontmatter() {
        let content = "# Body\nSome text";
        let result = extract_frontmatter(content);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_frontmatter_empty() {
        let content = "";
        let result = extract_frontmatter(content);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_frontmatter_no_closing() {
        let content = "---\ntitle: Test\n# Body without closing";
        let result = extract_frontmatter(content);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_title_from_filename() {
        assert_eq!(extract_title_from_filename("test.md"), "test");
        assert_eq!(extract_title_from_filename("my-file.md"), "my-file");
        assert_eq!(
            extract_title_from_filename("path/to/file.md"),
            "path/to/file"
        );
    }

    #[test]
    fn test_extract_title_from_filename_without_extension() {
        assert_eq!(extract_title_from_filename("test"), "test");
    }

    #[test]
    fn test_extract_title_from_frontmatter() {
        let frontmatter = "title: My Document\nauthor: John";
        let result = extract_title_from_frontmatter(frontmatter);
        assert_eq!(result, Some("My Document".to_string()));
    }

    #[test]
    fn test_extract_title_from_frontmatter_no_title() {
        let frontmatter = "author: John\ndate: 2024-01-01";
        let result = extract_title_from_frontmatter(frontmatter);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_title_from_frontmatter_empty() {
        let frontmatter = "";
        let result = extract_title_from_frontmatter(frontmatter);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_todo_entries_open() {
        let content = "- [ ] First todo\n- [ ] Second todo";
        let todos = extract_todo_entries(content, None, None);
        assert_eq!(todos.len(), 2);
        assert!(!todos[0].closed);
        assert!(!todos[1].closed);
        assert_eq!(todos[0].text, "First todo");
        assert_eq!(todos[1].text, "Second todo");
    }

    #[test]
    fn test_extract_todo_entries_closed() {
        let content = "- [x] Completed todo\n- [X] Also completed";
        let todos = extract_todo_entries(content, None, None);
        assert_eq!(todos.len(), 2);
        assert!(todos[0].closed);
        assert!(todos[1].closed);
    }

    #[test]
    fn test_extract_todo_entries_with_priority() {
        let content = "- [ ] High priority priority: A\n- [ ] Low priority priority: C";
        let todos = extract_todo_entries(content, None, None);
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].priority, Some("A".to_string()));
        assert_eq!(todos[1].priority, Some("C".to_string()));
    }

    #[test]
    fn test_extract_todo_entries_with_due_date() {
        let content = "- [ ] Due soon due: 20241231";
        let todos = extract_todo_entries(content, None, None);
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].due, Some("20241231".to_string()));
    }

    #[test]
    fn test_extract_todo_entries_with_tags() {
        let content = "- [ ] Feature todo #feature #important";
        let todos = extract_todo_entries(content, None, None);
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].tags, vec!["feature", "important"]);
    }

    #[test]
    fn test_extract_todo_entries_with_tag_attr() {
        let content = "- [ ] Tagged todo tag: review tag: urgent";
        let todos = extract_todo_entries(content, None, None);
        assert_eq!(todos.len(), 1);
        assert!(todos[0].tags.contains(&"review".to_string()));
        assert!(todos[0].tags.contains(&"urgent".to_string()));
    }

    #[test]
    fn test_extract_todo_entries_with_markdown_links() {
        let content = "- [ ] Check [documentation](https://example.com)";
        let todos = extract_todo_entries(content, None, None);
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].links, vec!["https://example.com"]);
    }

    #[test]
    fn test_extract_todo_entries_with_wiki_links() {
        let content = "- [ ] Read [[Related Page]] and [[Another Page]]";
        let todos = extract_todo_entries(content, None, None);
        assert_eq!(todos.len(), 1);
        assert!(todos[0].links.contains(&"related page".to_string()));
        assert!(todos[0].links.contains(&"another page".to_string()));
    }

    #[test]
    fn test_extract_todo_entries_line_numbers() {
        let content = "Line 1\nLine 2\n- [ ] Todo on line 3\nLine 4\n- [ ] Todo on line 5";
        let todos = extract_todo_entries(content, None, None);
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].line_number, 3);
        assert_eq!(todos[1].line_number, 5);
    }

    #[test]
    fn test_extract_todo_entries_empty() {
        let content = "No todos here\nJust regular text";
        let todos = extract_todo_entries(content, None, None);
        assert!(todos.is_empty());
    }

    #[test]
    fn test_todo_timestamp_from_due_date() {
        // Step 1: due date takes priority over everything else
        let todos = extract_todo_entries("- [ ] Task due: 20260101", None, None);
        assert_eq!(todos.len(), 1);
        // 2026-01-01 00:00:00 UTC
        assert_eq!(todos[0].updated, 1767225600);
    }

    #[test]
    fn test_todo_timestamp_from_inline_date() {
        // Step 2: a bare date in the text is used when no due date
        let todos = extract_todo_entries("- [ ] Meeting on 2026-03-15", None, None);
        assert_eq!(todos.len(), 1);
        // 2026-03-15 00:00:00 UTC = 2026-01-01 (1767225600) + 73 days
        assert_eq!(todos[0].updated, 1767225600 + 73 * 86400);
    }

    #[test]
    fn test_todo_timestamp_from_inline_wiki_date() {
        // Step 2: a [[YYYY-MM-DD]] wiki-link date is used
        let todos = extract_todo_entries("- [ ] See [[2026-03-15]]", None, None);
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].updated, 1767225600 + 73 * 86400);
    }

    #[test]
    fn test_todo_timestamp_skips_date_in_complex_wiki_link() {
        // A date that is part of a larger wiki link should NOT be picked
        let todos = extract_todo_entries("- [ ] Ref [[Tasks-2026-03-15-DOIT]]", None, None);
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].updated, 0); // no usable date -> 0
    }

    #[test]
    fn test_todo_timestamp_from_note_updated() {
        // Step 3: note's `updated` attribute used when no due/inline date
        // 2024-01-01 00:00:00 UTC = 1704067200
        let todos = extract_todo_entries("- [ ] Some task", Some(1704067200), Some(1609459200));
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].updated, 1704067200);
    }

    #[test]
    fn test_todo_timestamp_from_note_created() {
        // Step 4: note's `created` attribute used as last resort
        // 2021-01-01 00:00:00 UTC = 1609459200
        let todos = extract_todo_entries("- [ ] Some task", None, Some(1609459200));
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].updated, 1609459200);
    }

    #[test]
    fn test_todo_timestamp_priority_due_over_inline() {
        // Due date wins over an inline date
        let todos = extract_todo_entries("- [ ] Task on 2026-03-15 due: 20260101", None, None);
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].updated, 1767225600); // 2026-01-01
    }

    #[test]
    fn test_todo_timestamp_no_date_returns_zero() {
        let todos = extract_todo_entries("- [ ] Plain task with no dates", None, None);
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].updated, 0);
    }

    #[test]
    fn test_extract_links_markdown() {
        let content = "Check [link1](https://a.com) and [link2](https://b.com)";
        let links = extract_links(content);
        assert_eq!(links, vec!["https://a.com", "https://b.com"]);
    }

    #[test]
    fn test_extract_links_wiki() {
        let content = "See [[Page One]] and [[Page Two]]";
        let links = extract_links(content);
        assert!(links.contains(&"page one".to_string()));
        assert!(links.contains(&"page two".to_string()));
    }

    #[test]
    fn test_extract_links_mixed() {
        let content = "[Web link](https://example.com) and [[Wiki link]]";
        let links = extract_links(content);
        assert!(links.contains(&"https://example.com".to_string()));
        assert!(links.contains(&"wiki link".to_string()));
    }

    #[test]
    fn test_extract_links_empty() {
        let content = "No links here";
        let links = extract_links(content);
        assert!(links.is_empty());
    }

    #[test]
    fn test_convert_dates_to_wiki_links() {
        // Case 1: Plain dates should be converted
        let content = "Meeting scheduled for 2026-04-12 and follow-up on 2026-04-15";
        let result = convert_dates_to_wiki_links(content);
        assert_eq!(
            result,
            "Meeting scheduled for [[2026-04-12]] and follow-up on [[2026-04-15]]"
        );
    }

    #[test]
    fn test_convert_dates_preserves_existing_simple_links() {
        // Case 2: Dates already in [[YYYY-MM-DD]] should NOT be touched
        let content = "See [[2026-04-14]] and also 2026-04-15";
        let result = convert_dates_to_wiki_links(content);
        assert!(
            result.contains("[[2026-04-14]]"),
            "Simple wiki link should be preserved"
        );
        assert!(
            result.contains("[[2026-04-15]]"),
            "Plain date should be converted"
        );
        // Make sure we don't get double brackets
        assert!(
            !result.contains("[[[[2026-04-14]]]]"),
            "Should not create double brackets"
        );
    }

    #[test]
    fn test_convert_dates_preserves_complex_wiki_links() {
        // Case 3: Dates inside complex wiki links like [[Tasks-2026-04-14-DOIT]] should NOT be touched
        let content = "Task [[Tasks-2026-04-14-DOIT]] and date 2026-04-12";
        let result = convert_dates_to_wiki_links(content);
        // The date inside the complex wiki link should remain unchanged
        assert!(
            result.contains("[[Tasks-2026-04-14-DOIT]]"),
            "Complex wiki link should be preserved"
        );
        // The plain date should be converted
        assert!(
            result.contains("[[2026-04-12]]"),
            "Plain date should be converted"
        );
    }

    #[test]
    fn test_convert_dates_no_dates() {
        let content = "No dates in this content";
        let result = convert_dates_to_wiki_links(content);
        assert_eq!(result, content);
    }

    #[test]
    fn test_convert_dates_multiple_in_complex_link() {
        // Multiple dates inside a complex wiki link
        let content = "[[Project-2026-04-12-to-2026-04-15]] and 2026-04-20";
        let result = convert_dates_to_wiki_links(content);
        // Both dates in the complex link should be preserved
        assert!(
            result.contains("[[Project-2026-04-12-to-2026-04-15]]"),
            "Complex link with multiple dates should be preserved"
        );
        // The plain date should be converted
        assert!(
            result.contains("[[2026-04-20]]"),
            "Plain date should be converted"
        );
    }

    #[test]
    fn test_yaml_to_json_value_string() {
        let yaml = yaml_rust2::Yaml::String("test".to_string());
        let json = yaml_to_json_value(&yaml);
        assert_eq!(json, serde_json::json!("test"));
    }

    #[test]
    fn test_yaml_to_json_value_integer() {
        let yaml = yaml_rust2::Yaml::Integer(42);
        let json = yaml_to_json_value(&yaml);
        assert_eq!(json, serde_json::json!(42));
    }

    #[test]
    fn test_yaml_to_json_value_boolean() {
        let yaml = yaml_rust2::Yaml::Boolean(true);
        let json = yaml_to_json_value(&yaml);
        assert_eq!(json, serde_json::json!(true));
    }

    #[test]
    fn test_yaml_to_json_value_null() {
        let yaml = yaml_rust2::Yaml::Null;
        let json = yaml_to_json_value(&yaml);
        assert_eq!(json, serde_json::json!(null));
    }

    #[test]
    fn test_yaml_to_json_value_array() {
        let yaml = yaml_rust2::Yaml::Array(vec![
            yaml_rust2::Yaml::String("a".to_string()),
            yaml_rust2::Yaml::String("b".to_string()),
        ]);
        let json = yaml_to_json_value(&yaml);
        assert_eq!(json, serde_json::json!(["a", "b"]));
    }

    #[test]
    fn test_yaml_to_json_value_wiki_link() {
        let yaml = yaml_rust2::Yaml::String("[[Page Name]]".to_string());
        let json = yaml_to_json_value(&yaml);
        assert_eq!(json, serde_json::json!("Page Name"));
    }

    #[test]
    fn test_process_markdown_file_no_frontmatter() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let file_path = input_dir.join("test.md");

        let mut file = fs::File::create(&file_path)?;
        writeln!(file, "# Title\n\n- [ ] Todo item")?;

        let data = process_markdown_file(&file_path, input_dir)?;
        assert_eq!(data.filename, "test.md");
        assert_eq!(data.title, "test");
        assert_eq!(data.todo.len(), 1);
        assert_eq!(data.header.fields.len(), 0);

        Ok(())
    }

    #[test]
    fn test_process_markdown_file_with_frontmatter() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let file_path = input_dir.join("test.md");

        let mut file = fs::File::create(&file_path)?;
        writeln!(
            file,
            "---\ntitle: My Document\nauthor: John\n---\n\n# Body\n\n- [ ] Todo"
        )?;

        let data = process_markdown_file(&file_path, input_dir)?;
        assert_eq!(data.filename, "test.md");
        assert_eq!(data.title, "My Document");
        assert_eq!(data.todo.len(), 1);
        assert!(data.header.fields.contains_key("author"));

        Ok(())
    }

    #[test]
    fn test_process_markdown_file_jira_note_drops_short_segments() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let jira_dir = input_dir.join("jira");
        fs::create_dir(&jira_dir)?;
        let file_path = jira_dir.join("PROJ-1.md");

        let long_words = (0..25).map(|i| format!("word{}", i)).collect::<Vec<_>>().join(" ");
        let mut file = fs::File::create(&file_path)?;
        writeln!(file, "# Short\n\nToo short.\n\n# Long\n\n{}\n", long_words)?;

        let data = process_markdown_file(&file_path, input_dir)?;

        assert!(!data
            .segments
            .iter()
            .any(|s| s.text.starts_with("# Short")));
        assert!(data
            .segments
            .iter()
            .any(|s| s.text.starts_with("# Long")));

        Ok(())
    }

    #[test]
    fn test_process_markdown_file_non_jira_note_keeps_short_segments() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let file_path = input_dir.join("test.md");

        let mut file = fs::File::create(&file_path)?;
        writeln!(file, "# Short\n\nToo short.\n")?;

        let data = process_markdown_file(&file_path, input_dir)?;

        assert!(data.segments.iter().any(|s| s.text.starts_with("# Short")));

        Ok(())
    }

    #[test]
    fn test_process_markdown_file_subdir() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let subdir = input_dir.join("subdir");
        fs::create_dir(&subdir)?;
        let file_path = subdir.join("test.md");

        let mut file = fs::File::create(&file_path)?;
        writeln!(file, "- [ ] Todo in subdir")?;

        let data = process_markdown_file(&file_path, input_dir)?;
        assert_eq!(data.filename, "subdir/test.md");

        Ok(())
    }

    #[test]
    fn test_write_markdown_data_to_sqlite() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");

        let data = MarkdownData {
            filename: "test.md".to_string(),
            created: 1234567890,
            updated: 1234567890,
            title: "Test".to_string(),
            header: Header {
                fields: HashMap::new(),
            },
            todo: vec![TodoEntry {
                closed: false,
                priority: Some("A".to_string()),
                due: Some("20241231".to_string()),
                tags: vec!["feature".to_string()],
                links: vec!["https://example.com".to_string()],
                line_number: 5,
                text: "Test todo".to_string(),
                updated: 0,
            }],
            link: vec!["https://example.com".to_string()],
            body: "This is the test note body content.".to_string(),
            segments: vec![],
        };

        write_markdown_data_to_sqlite(&data, &db_path)?;

        // Verify database was created and has data
        let conn = rusqlite::Connection::open(&db_path)?;
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM markdown_data", [], |row| row.get(0))?;
        assert_eq!(count, 1);

        let todo_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM todo_entries", [], |row| row.get(0))?;
        assert_eq!(todo_count, 1);

        Ok(())
    }

    #[test]
    fn test_parse_markdown_directory() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let db_path = temp_dir.path().join("test.db");

        // Create test files
        fs::write(input_dir.join("file1.md"), "- [ ] Todo 1")?;
        fs::write(input_dir.join("file2.md"), "- [ ] Todo 2")?;

        // Create subdirectory with file
        let subdir = input_dir.join("subdir");
        fs::create_dir(&subdir)?;
        fs::write(subdir.join("file3.md"), "- [ ] Todo 3")?;

        let count = parse_markdown_directory(input_dir, &db_path)?;
        assert_eq!(count, 3);

        // Verify database contents
        let conn = rusqlite::Connection::open(&db_path)?;
        let file_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM markdown_data", [], |row| row.get(0))?;
        assert_eq!(file_count, 3);

        Ok(())
    }

    #[test]
    fn test_parse_markdown_directory_empty() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let db_path = temp_dir.path().join("test.db");

        let count = parse_markdown_directory(input_dir, &db_path)?;
        assert_eq!(count, 0);

        Ok(())
    }

    #[test]
    fn test_remove_dataview_sections() {
        let content = r#"# My Note

Some content here

```dataview
LIST
FROM "Projects"
WHERE completed = false
```

More content after dataview

- [ ] A real todo

```dataview
TABLE file.name, file.size
FROM "Documents"
```

Final content
"#;

        let filtered = remove_dataview_sections(content);

        // Should contain the todos and regular content
        assert!(filtered.contains("Some content here"));
        assert!(filtered.contains("More content after dataview"));
        assert!(filtered.contains("A real todo"));
        assert!(filtered.contains("Final content"));

        // Should NOT contain dataview content
        assert!(!filtered.contains("FROM \"Projects\""));
        assert!(!filtered.contains("file.size"));
        assert!(!filtered.contains("```dataview"));
    }

    #[test]
    fn test_extract_todo_entries_ignores_dataview() {
        let content = r#"- [ ] Real todo
```dataview
- [ ] This is dataview syntax not a todo
```
- [ ] Another real todo"#;

        // Must filter dataview sections before extracting, as done in process_markdown_file
        let filtered = remove_dataview_sections(content);
        let todos = extract_todo_entries(&filtered, None, None);
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].text, "Real todo");
        assert_eq!(todos[1].text, "Another real todo");
    }

    #[test]
    fn test_extract_links_ignores_dataview() {
        let content = r#"Check [this link](https://example.com)
```dataview
[[Page in dataview]]
```
See [[Wiki Link]]"#;

        // Must filter dataview sections before extracting, as done in process_markdown_file
        let filtered = remove_dataview_sections(content);
        let links = extract_links(&filtered);
        assert!(links.contains(&"https://example.com".to_string()));
        assert!(links.contains(&"wiki link".to_string()));
        assert!(!links.contains(&"Page in dataview".to_string()));
    }

    #[test]
    fn test_remove_tasks_sections() {
        let content = r#"# My Note

Some content here

```tasks
not done
path includes Projects
```

More content after tasks

- [ ] A real todo

```tasks
done
sort by due date
```

Final content
"#;

        let filtered = remove_dataview_sections(content);

        // Should contain the todos and regular content
        assert!(filtered.contains("Some content here"));
        assert!(filtered.contains("More content after tasks"));
        assert!(filtered.contains("A real todo"));
        assert!(filtered.contains("Final content"));

        // Should NOT contain tasks content
        assert!(!filtered.contains("not done"));
        assert!(!filtered.contains("sort by due date"));
        assert!(!filtered.contains("```tasks"));
    }

    #[test]
    fn test_extract_todo_entries_ignores_tasks() {
        let content = r#"- [ ] Real todo
```tasks
- [ ] This is tasks syntax not a real todo
not done
```
- [ ] Another real todo"#;

        // Must filter dataview sections before extracting, as done in process_markdown_file
        let filtered = remove_dataview_sections(content);
        let todos = extract_todo_entries(&filtered, None, None);
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].text, "Real todo");
        assert_eq!(todos[1].text, "Another real todo");
    }

    #[test]
    fn test_extract_links_ignores_tasks() {
        let content = r#"Check [this link](https://example.com)
```tasks
[[Page in tasks block]]
```
See [[Wiki Link]]"#;

        // Must filter dataview sections before extracting, as done in process_markdown_file
        let filtered = remove_dataview_sections(content);
        let links = extract_links(&filtered);
        assert!(links.contains(&"https://example.com".to_string()));
        assert!(links.contains(&"wiki link".to_string()));
        assert!(!links.contains(&"Page in tasks block".to_string()));
    }

    #[test]
    fn test_remove_mixed_code_sections() {
        let content = r#"# My Note

Some content here

```dataview
LIST
FROM "Projects"
```

Middle content

```tasks
not done
```

- [ ] A real todo

```dataview
TABLE file.name
```

Final content
"#;

        let filtered = remove_dataview_sections(content);

        // Should contain the todos and regular content
        assert!(filtered.contains("Some content here"));
        assert!(filtered.contains("Middle content"));
        assert!(filtered.contains("A real todo"));
        assert!(filtered.contains("Final content"));

        // Should NOT contain any code block content
        assert!(!filtered.contains("FROM \"Projects\""));
        assert!(!filtered.contains("not done"));
        assert!(!filtered.contains("file.name"));
        assert!(!filtered.contains("```dataview"));
        assert!(!filtered.contains("```tasks"));
    }

    #[test]
    fn test_file_update_replaces_old_content() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");

        // First import - file with 2 todos
        let data1 = MarkdownData {
            filename: "test.md".to_string(),
            created: 1234567880,
            updated: 1234567890,
            title: "Test".to_string(),
            header: Header {
                fields: HashMap::new(),
            },
            todo: vec![
                TodoEntry {
                    closed: false,
                    priority: Some("A".to_string()),
                    due: Some("20241231".to_string()),
                    tags: vec!["old".to_string()],
                    links: vec![],
                    line_number: 1,
                    text: "First todo".to_string(),
                    updated: 0,
                },
                TodoEntry {
                    closed: true,
                    priority: None,
                    due: None,
                    tags: vec![],
                    links: vec![],
                    line_number: 2,
                    text: "Second todo".to_string(),
                    updated: 0,
                },
            ],
            link: vec![],
            body: "Old body content".to_string(),
            segments: vec![],
        };

        write_markdown_data_to_sqlite(&data1, &db_path)?;

        // Verify first import
        let conn = rusqlite::Connection::open(&db_path)?;
        let todo_count1: i64 = conn.query_row(
            "SELECT COUNT(*) FROM todo_entries WHERE filename = ?1",
            ["test.md"],
            |row| row.get(0),
        )?;
        assert_eq!(todo_count1, 2);

        let body1: String = conn.query_row(
            "SELECT body FROM markdown_data WHERE filename = ?1",
            ["test.md"],
            |row| row.get(0),
        )?;
        assert_eq!(body1, "Old body content");

        // Second import - same file with different content (1 todo, different body)
        let data2 = MarkdownData {
            filename: "test.md".to_string(),
            created: 1234567880,
            updated: 1234567891,
            title: "Updated Test".to_string(),
            header: Header {
                fields: HashMap::new(),
            },
            todo: vec![TodoEntry {
                closed: false,
                priority: Some("B".to_string()),
                due: Some("20250101".to_string()),
                tags: vec!["new".to_string()],
                links: vec!["https://example.com".to_string()],
                line_number: 5,
                text: "New todo".to_string(),
                updated: 0,
            }],
            link: vec!["https://example.com".to_string()],
            body: "New body content".to_string(),
            segments: vec![],
        };

        write_markdown_data_to_sqlite(&data2, &db_path)?;

        // Verify second import - old todos should be deleted, new ones added
        let todo_count2: i64 = conn.query_row(
            "SELECT COUNT(*) FROM todo_entries WHERE filename = ?1",
            ["test.md"],
            |row| row.get(0),
        )?;
        assert_eq!(todo_count2, 1); // Should have only 1 todo now

        let todo_text: String = conn.query_row(
            "SELECT text FROM todo_entries WHERE filename = ?1",
            ["test.md"],
            |row| row.get(0),
        )?;
        assert_eq!(todo_text, "New todo"); // Should be the new todo, not old ones

        // Verify body was updated
        let body2: String = conn.query_row(
            "SELECT body FROM markdown_data WHERE filename = ?1",
            ["test.md"],
            |row| row.get(0),
        )?;
        assert_eq!(body2, "New body content");

        // Verify title was updated
        let title: String = conn.query_row(
            "SELECT title FROM markdown_data WHERE filename = ?1",
            ["test.md"],
            |row| row.get(0),
        )?;
        assert_eq!(title, "Updated Test");

        Ok(())
    }

    #[test]
    fn test_yaml_to_json_value_regular_string() {
        let yaml = yaml_rust2::Yaml::String("normal value".to_string());
        let json = yaml_to_json_value(&yaml);
        assert_eq!(json, serde_json::json!("normal value"));
    }

    #[test]
    fn test_process_markdown_file_with_tag_attribute() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let file_path = input_dir.join("test.md");

        let mut file = fs::File::create(&file_path)?;
        writeln!(
            file,
            "---\ntitle: My Document\ntype: #meeting\n---\n\n# Body\n\n- [ ] Todo"
        )?;

        let data = process_markdown_file(&file_path, input_dir)?;
        assert_eq!(data.filename, "test.md");
        assert_eq!(data.title, "My Document");

        // Check that the type field has the # stripped
        assert!(data.header.fields.contains_key("type"));
        assert_eq!(
            data.header.fields.get("type"),
            Some(&serde_json::json!("meeting"))
        );

        Ok(())
    }

    #[test]
    fn test_process_markdown_file_transliterates_umlauts_in_attribute_values(
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Isolate from any real ~/.config/note_search/config on the machine
        // running the test (it may map away fields like "participants").
        let config_dir = TempDir::new()?;
        let config_path = config_dir.path().join("config");
        fs::write(&config_path, "[Mapping]\n")?;
        std::env::set_var("NOTE_SEARCH_CONFIG", &config_path);

        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let file_path = input_dir.join("test.md");

        let mut file = fs::File::create(&file_path)?;
        writeln!(
            file,
            "---\ntitle: My Document\nauthor: Jürgen Müller\ncity: Köln\nteam_members:\n  - Björn\n  - Weiß\n---\n\n# Body"
        )?;

        let data = process_markdown_file(&file_path, input_dir)?;

        assert_eq!(
            data.header.fields.get("author"),
            Some(&serde_json::json!("Juergen Mueller"))
        );
        assert_eq!(
            data.header.fields.get("city"),
            Some(&serde_json::json!("Koeln"))
        );
        assert_eq!(
            data.header.fields.get("team_members"),
            Some(&serde_json::json!(["Bjoern", "Weiss"]))
        );

        // Keys and other fields are left alone; "title" has no umlauts.
        assert_eq!(
            data.header.fields.get("title"),
            Some(&serde_json::json!("My Document"))
        );

        Ok(())
    }

    #[test]
    fn test_transliterate_umlauts() {
        assert_eq!(transliterate_umlauts("Müller"), "Mueller");
        assert_eq!(transliterate_umlauts("Straße"), "Strasse");
        assert_eq!(transliterate_umlauts("ÄÖÜäöüß"), "AeOeUeaeoeuess");
        assert_eq!(transliterate_umlauts("no umlauts here"), "no umlauts here");
    }

    #[test]
    fn test_remove_hash_prefixes() {
        let content = "type: #meeting";
        let cleaned = remove_hash_prefixes(content);
        assert_eq!(cleaned, "type: meeting");
    }

    #[test]
    fn test_remove_hash_prefixes_multiple() {
        let content = "type: #meeting\ncategory: #work\nstatus: active";
        let cleaned = remove_hash_prefixes(content);
        assert_eq!(cleaned, "type: meeting\ncategory: work\nstatus: active");
    }

    #[test]
    fn test_remove_hash_prefixes_no_tags() {
        let content = "title: My Document\nstatus: active";
        let cleaned = remove_hash_prefixes(content);
        assert_eq!(cleaned, "title: My Document\nstatus: active");
    }

    #[test]
    fn test_remove_hash_prefixes_with_spaces() {
        let content = "type:   #meeting";
        let cleaned = remove_hash_prefixes(content);
        assert_eq!(cleaned, "type:   meeting");
    }

    #[test]
    fn test_remove_hash_prefixes_in_values() {
        let content = "tags: #feature #bug #urgent";
        let cleaned = remove_hash_prefixes(content);
        // Should remove all # from the value
        assert_eq!(cleaned, "tags: feature bug urgent");
    }

    #[test]
    fn test_process_markdown_file_labels_become_lowercase_links() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let file_path = input_dir.join("test.md");

        let mut file = fs::File::create(&file_path)?;
        writeln!(
            file,
            "---\nkey: \"PROJ-1\"\nlabels:\n  - \"KRAMFORS\"\n  - \"Other_Label\"\n---\n\n# Body"
        )?;

        let data = process_markdown_file(&file_path, input_dir)?;
        assert!(data.link.contains(&"kramfors".to_string()));
        assert!(data.link.contains(&"other_label".to_string()));
        // Original casing is preserved in the attribute itself.
        let labels = data.header.fields.get("labels").unwrap();
        assert!(labels.as_array().unwrap().contains(&serde_json::json!("KRAMFORS")));

        Ok(())
    }

    #[test]
    fn test_process_markdown_file_with_mixed_attributes() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let file_path = input_dir.join("test.md");

        let mut file = fs::File::create(&file_path)?;
        writeln!(
            file,
            "---\ntitle: My Document\ntype: #meeting\ncategory: #work\nstatus: active\n---\n\n# Body"
        )?;

        let data = process_markdown_file(&file_path, input_dir)?;

        // Tag-like values should have # stripped
        assert_eq!(
            data.header.fields.get("type"),
            Some(&serde_json::json!("meeting"))
        );
        assert_eq!(
            data.header.fields.get("category"),
            Some(&serde_json::json!("work"))
        );
        // Regular strings stay as-is
        assert_eq!(
            data.header.fields.get("status"),
            Some(&serde_json::json!("active"))
        );

        Ok(())
    }

    #[test]
    fn test_extract_attributes_from_body() {
        let body = "# Participants\n- [[daniela]]\n- [[michael]]\n\n# Content\n- Bla\n\n# Mixed\n- [[Valid]]\n- Invalid Space\n\n# NotAList\nSome text here\n- Item\n";
        let attrs = extract_attributes_from_body(body);
        assert_eq!(attrs.len(), 2);
        assert_eq!(
            attrs.get("participants"),
            Some(&vec!["daniela".to_string(), "michael".to_string()])
        );
        assert_eq!(attrs.get("content"), Some(&vec!["Bla".to_string()]));
        assert!(attrs.get("mixed").is_none());
        assert!(attrs.get("notalist").is_none());
    }

    #[test]
    fn test_process_markdown_file_merges_attributes() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let file_path = input_dir.join("test.md");

        // Isolate from the user's real mapping config so this test is deterministic.
        let empty_cfg = temp_dir.path().join("empty_config.ini");
        fs::write(&empty_cfg, "[Mapping]\n")?;
        std::env::set_var("NOTE_SEARCH_CONFIG", empty_cfg.to_str().unwrap());

        let mut file = fs::File::create(&file_path)?;
        writeln!(
            file,
            "---\nparticipants:\n- [[stefan]]\n- [[carsten]]\n---\n# Participants\n- [[daniela]]\n- [[michael]]"
        )?;

        let data = process_markdown_file(&file_path, input_dir)?;
        std::env::remove_var("NOTE_SEARCH_CONFIG");
        let participants = data.header.fields.get("participants").unwrap();
        assert!(participants.is_array());
        let arr = participants.as_array().unwrap();
        assert_eq!(arr.len(), 4);
        assert!(arr.contains(&serde_json::json!("stefan")));
        assert!(arr.contains(&serde_json::json!("carsten")));
        assert!(arr.contains(&serde_json::json!("daniela")));
        assert!(arr.contains(&serde_json::json!("michael")));

        Ok(())
    }

    #[test]
    fn test_extract_date_part() {
        assert_eq!(
            extract_date_part("2026-05-19"),
            Some("2026-05-19".to_string())
        );
        assert_eq!(
            extract_date_part("2026-05-19 15:11"),
            Some("2026-05-19".to_string())
        );
        assert_eq!(
            extract_date_part("[[2026-05-19]]"),
            Some("2026-05-19".to_string())
        );
        assert_eq!(
            extract_date_part("[[2026-05-19]] 10:00"),
            Some("2026-05-19".to_string())
        );
        assert_eq!(extract_date_part("invalid"), None);
        assert_eq!(extract_date_part("2026-05-1"), None);
    }

    #[test]
    fn test_parse_date_string_unix_timestamp() {
        let result = parse_date_string("1704067200");
        assert_eq!(result, Some(1704067200));
    }

    #[test]
    fn test_parse_date_string_iso_date() {
        // 2024-01-01 at midnight local time
        let result = parse_date_string("2024-01-01");
        assert!(result.is_some());
        let expected = chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(chrono::Local)
            .unwrap()
            .timestamp() as u64;
        assert_eq!(result, Some(expected));
    }

    #[test]
    fn test_parse_date_string_with_brackets() {
        // [[yyyy-MM-dd]] format
        let result = parse_date_string("[[2024-01-01]]");
        assert!(result.is_some());
        let expected = chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(chrono::Local)
            .unwrap()
            .timestamp() as u64;
        assert_eq!(result, Some(expected));
    }

    #[test]
    fn test_parse_date_string_with_brackets_and_time() {
        // [[yyyy-MM-dd]] hh:mm format
        let result = parse_date_string("[[2024-01-01]] 17:08");
        assert!(result.is_some());
        let expected = chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .and_hms_opt(17, 8, 0)
            .unwrap()
            .and_local_timezone(chrono::Local)
            .unwrap()
            .timestamp() as u64;
        assert_eq!(result, Some(expected));
    }

    #[test]
    fn test_parse_date_string_with_time() {
        // yyyy-MM-dd hh:mm format
        let result = parse_date_string("2024-01-01 17:08");
        assert!(result.is_some());
        let expected = chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .and_hms_opt(17, 8, 0)
            .unwrap()
            .and_local_timezone(chrono::Local)
            .unwrap()
            .timestamp() as u64;
        assert_eq!(result, Some(expected));
    }

    #[test]
    fn test_parse_date_string_midnight() {
        let result = parse_date_string("2024-01-01 00:00");
        assert!(result.is_some());
        let expected = chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(chrono::Local)
            .unwrap()
            .timestamp() as u64;
        assert_eq!(result, Some(expected));
    }

    #[test]
    fn test_parse_date_string_invalid() {
        let result = parse_date_string("not a date");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_date_string_empty() {
        let result = parse_date_string("");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_markdown_directory_no_md_files() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let db_path = temp_dir.path().join("test.db");

        // Create non-markdown files
        fs::write(input_dir.join("file.txt"), "Text file")?;
        fs::write(input_dir.join("file.json"), "{}")?;

        let count = parse_markdown_directory(input_dir, &db_path)?;
        assert_eq!(count, 0);

        Ok(())
    }

    #[test]
    fn test_remove_orphaned_notes() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let db_path = temp_dir.path().join("test.db");

        // Create initial files
        fs::write(input_dir.join("existing.md"), "- [ ] Existing todo")?;
        fs::write(input_dir.join("to_be_deleted.md"), "- [ ] Will be deleted")?;

        // First import - both files exist
        let count1 = parse_markdown_directory(input_dir, &db_path)?;
        assert_eq!(count1, 2);

        // Verify both files are in database
        let conn = rusqlite::Connection::open(&db_path)?;
        let count_before: i64 =
            conn.query_row("SELECT COUNT(*) FROM markdown_data", [], |row| row.get(0))?;
        assert_eq!(count_before, 2);

        // Delete one file from filesystem
        fs::remove_file(input_dir.join("to_be_deleted.md"))?;

        // Run import again - should remove orphaned note
        let count2 = parse_markdown_directory(input_dir, &db_path)?;
        assert_eq!(count2, 1); // Only existing.md was imported

        // Verify only one file remains in database
        let count_after: i64 =
            conn.query_row("SELECT COUNT(*) FROM markdown_data", [], |row| row.get(0))?;
        assert_eq!(count_after, 1);

        // Verify it's the correct file
        let remaining_file: String =
            conn.query_row("SELECT filename FROM markdown_data LIMIT 1", [], |row| {
                row.get(0)
            })?;
        assert_eq!(remaining_file, "existing.md");

        // Verify orphaned todos were also removed
        let todo_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM todo_entries", [], |row| row.get(0))?;
        assert_eq!(todo_count, 1); // Only one todo from existing.md

        Ok(())
    }

    #[test]
    fn test_remove_orphaned_notes_with_subdirs() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let db_path = temp_dir.path().join("test.db");

        // Create subdirectories with files
        let subdir = input_dir.join("subdir");
        fs::create_dir(&subdir)?;
        fs::write(subdir.join("keep.md"), "- [ ] Keep this")?;
        fs::write(subdir.join("remove.md"), "- [ ] Remove this")?;

        // First import
        let count1 = parse_markdown_directory(input_dir, &db_path)?;
        assert_eq!(count1, 2);

        // Delete one file
        fs::remove_file(subdir.join("remove.md"))?;

        // Re-import
        let count2 = parse_markdown_directory(input_dir, &db_path)?;
        assert_eq!(count2, 1);

        // Verify database state
        let conn = rusqlite::Connection::open(&db_path)?;
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM markdown_data", [], |row| row.get(0))?;
        assert_eq!(count, 1);

        let remaining: String =
            conn.query_row("SELECT filename FROM markdown_data LIMIT 1", [], |row| {
                row.get(0)
            })?;
        assert_eq!(remaining, "subdir/keep.md");

        Ok(())
    }

    #[test]
    fn test_update_files_in_db_refreshes_existing() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let db_path = temp_dir.path().join("test.db");

        // Seed a file and import it
        let file_path = input_dir.join("a.md");
        fs::write(&file_path, "---\ntitle: Original\n---\n\n# Original\n")?;
        let _ = parse_markdown_directory(input_dir, &db_path)?;
        assert!(file_path.exists());

        // Mutate the file (add a todo + change title)
        fs::write(
            &file_path,
            "---\ntitle: Updated\n---\n\n# Updated\n\n- [ ] Fresh todo\n",
        )?;

        let conn = rusqlite::Connection::open(&db_path)?;
        let summary = update_files_in_db(&["a.md".to_string()], input_dir, &conn)?;
        assert_eq!(summary.updated, 1);
        assert_eq!(summary.removed, 0);
        assert!(summary.errors.is_empty());

        // Verify the new title and todo made it in
        let title: String = conn.query_row(
            "SELECT title FROM markdown_data WHERE filename = 'a.md'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(title, "Updated");
        let todo_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM todo_entries WHERE filename = 'a.md'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(todo_count, 1);

        Ok(())
    }

    #[test]
    fn test_update_files_in_db_removes_missing() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let db_path = temp_dir.path().join("test.db");

        fs::write(input_dir.join("keep.md"), "# Keep\n")?;
        fs::write(input_dir.join("gone.md"), "# Gone\n")?;
        let _ = parse_markdown_directory(input_dir, &db_path)?;
        assert!(input_dir.join("keep.md").exists());
        assert!(input_dir.join("gone.md").exists());

        // Delete one file from disk, then update both
        fs::remove_file(input_dir.join("gone.md"))?;
        let conn = rusqlite::Connection::open(&db_path)?;
        let summary = update_files_in_db(
            &["keep.md".to_string(), "gone.md".to_string()],
            input_dir,
            &conn,
        )?;
        assert_eq!(summary.updated, 1);
        assert_eq!(summary.removed, 1);
        assert!(summary.errors.is_empty());

        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM markdown_data", [], |row| row.get(0))?;
        assert_eq!(count, 1);

        Ok(())
    }

    #[test]
    fn test_update_files_in_db_replaces_todos() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let db_path = temp_dir.path().join("test.db");

        fs::write(input_dir.join("a.md"), "# New\n\n- [ ] Keep\n")?;
        let _ = parse_markdown_directory(input_dir, &db_path)?;
        let conn = rusqlite::Connection::open(&db_path)?;
        let before: i64 = conn.query_row(
            "SELECT COUNT(*) FROM todo_entries WHERE filename = 'a.md'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(before, 1);

        // Overwrite a.md with different todos
        fs::write(input_dir.join("a.md"), "# New\n\n- [ ] One\n- [x] Two\n")?;

        let summary = update_files_in_db(&["a.md".to_string()], input_dir, &conn)?;
        assert_eq!(summary.updated, 1);
        assert!(summary.errors.is_empty());

        let after: i64 = conn.query_row(
            "SELECT COUNT(*) FROM todo_entries WHERE filename = 'a.md'",
            [],
            |row| row.get(0),
        )?;
        // Old todos replaced with exactly 2 new ones
        assert_eq!(after, 2);

        Ok(())
    }

    #[test]
    fn test_implicit_date_links() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let file_path = input_dir.join("test.md");

        let mut file = fs::File::create(&file_path)?;
        writeln!(
            file,
            "---\ntitle: Date Test\ncreated: 2026-05-19 15:11\nchanged: [[2026-05-20]] 10:00\nmodified: 2026-05-21\n---"
        )?;

        let data = process_markdown_file(&file_path, input_dir)?;

        // Should have implicit links to the dates
        assert!(data.link.contains(&"2026-05-19".to_string()));
        assert!(data.link.contains(&"2026-05-20".to_string()));
        assert!(data.link.contains(&"2026-05-21".to_string()));

        Ok(())
    }

    #[test]
    fn test_updated_uses_frontmatter_attribute() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let file_path = input_dir.join("test.md");

        let mut file = fs::File::create(&file_path)?;
        writeln!(
            file,
            "---\ntitle: Date Test\ncreated: 2026-05-19 15:11\nupdated: 2024-01-01 17:08\n---"
        )?;

        let data = process_markdown_file(&file_path, input_dir)?;

        // The note's `updated` field should match the frontmatter `updated` attribute,
        // not the file's modification time.
        let expected = parse_date_string("2024-01-01 17:08").unwrap();
        assert_eq!(data.updated, expected);

        Ok(())
    }

    #[test]
    fn test_updated_falls_back_to_file_mtime() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let input_dir = temp_dir.path();
        let file_path = input_dir.join("test.md");

        let mut file = fs::File::create(&file_path)?;
        writeln!(file, "---\ntitle: Date Test\n---\n\n# Body")?;

        let data = process_markdown_file(&file_path, input_dir)?;

        // Without a frontmatter `updated` attribute, the file's modification time
        // should be used as a fallback.
        let file_mtime = fs::metadata(&file_path)?
            .modified()?
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_secs();
        assert_eq!(data.updated, file_mtime);

        Ok(())
    }
}
