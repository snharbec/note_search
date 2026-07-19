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
static LIST_ITEM_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^(\s*)(?:[-*+]|\d+\.)\s+(.*)$").unwrap());
static FENCE_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^\s*```").unwrap());

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
    pub elements: Vec<Element>,
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
                tags.push(tag_capture[1].to_string());
            }

            for tag_capture in TAG_ATTR_REGEX.captures_iter(content) {
                tags.push(tag_capture[1].to_string());
            }

            for link_capture in LINK_REGEX.captures_iter(content) {
                links.push(link_capture[2].to_string());
            }

            for link_capture in WIKI_LINK_REGEX.captures_iter(content) {
                links.push(link_capture[1].to_string());
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
    let wiki_date_re = regex::Regex::new(r"\[\[(\d{4}-\d{2}-\d{2})\]\]").unwrap();
    if let Some(c) = wiki_date_re.captures(content) {
        if let Some(ts) = yyyymmdd_to_timestamp(&c[1]) {
            return Some(ts);
        }
    }

    let date_re = regex::Regex::new(r"\b(\d{4}-\d{2}-\d{2})\b").unwrap();
    for c in date_re.captures_iter(content) {
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
        links.push(link_capture[2].to_string());
    }

    for link_capture in WIKI_LINK_REGEX.captures_iter(markdown_content) {
        links.push(link_capture[1].to_string());
    }

    links
}

/// Extract `#tag`s from `text`, expanding `a/b/c` into `a`, `a/b`, `a/b/c`
/// (same hierarchy rule used for the note-level `tags` aggregate).
fn extract_tags_with_hierarchy(text: &str) -> HashSet<String> {
    let mut tags = HashSet::new();
    for tag_capture in TAG_REGEX.captures_iter(text) {
        let tag = tag_capture[1].to_string();
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

/// A paragraph, list item (with nested children folded in), or heading
/// within a note's body, with tags/links already resolved (own text, plus
/// cascade from ancestor headings and the document's frontmatter links).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Element {
    pub start_line: usize,
    pub end_line: usize,
    pub heading_level: Option<u32>,
    pub text: String,
    pub tags: Vec<String>,
    pub links: Vec<String>,
}

struct HeadingFrame {
    level: u32,
    cascade_tags: HashSet<String>,
    cascade_links: HashSet<String>,
}

struct OpenListItem {
    start_line: usize,
    last_line: usize,
    indent: usize,
    lines: Vec<String>,
}

fn own_tags_links(text: &str) -> (Vec<String>, Vec<String>) {
    let tags: Vec<String> = extract_tags_with_hierarchy(text).into_iter().collect();
    let links = extract_links(text);
    (tags, links)
}

fn push_element(
    elements: &mut Vec<Element>,
    start_line: usize,
    end_line: usize,
    heading_level: Option<u32>,
    text: String,
    heading_stack: &[HeadingFrame],
    frontmatter_links: &[String],
) {
    if text.trim().is_empty() {
        return;
    }
    let (mut tags, mut links) = own_tags_links(&text);
    if let Some(frame) = heading_stack.last() {
        for t in &frame.cascade_tags {
            if !tags.contains(t) {
                tags.push(t.clone());
            }
        }
        for l in &frame.cascade_links {
            if !links.contains(l) {
                links.push(l.clone());
            }
        }
    }
    for l in frontmatter_links {
        if !links.contains(l) {
            links.push(l.clone());
        }
    }
    elements.push(Element {
        start_line,
        end_line,
        heading_level,
        text,
        tags,
        links,
    });
}

fn finalize_paragraph(
    paragraph_start: &mut Option<usize>,
    paragraph_lines: &mut Vec<String>,
    end_line: usize,
    heading_stack: &[HeadingFrame],
    frontmatter_links: &[String],
    elements: &mut Vec<Element>,
) {
    if let Some(start) = paragraph_start.take() {
        let text = paragraph_lines.join("\n");
        paragraph_lines.clear();
        push_element(elements, start, end_line, None, text, heading_stack, frontmatter_links);
    }
}

/// Close every open list item whose indentation is `>= indent`, emitting an
/// `Element` for each in document order (shallowest/parent first, since a
/// parent's element should be reported before the child block it contains).
/// Passing `indent = 0` closes all of them - used at headings, fence
/// boundaries, and EOF.
fn finalize_list_items_at_or_deeper(
    list_stack: &mut Vec<OpenListItem>,
    indent: usize,
    heading_stack: &[HeadingFrame],
    frontmatter_links: &[String],
    elements: &mut Vec<Element>,
) {
    let mut split_at = list_stack.len();
    while split_at > 0 && list_stack[split_at - 1].indent >= indent {
        split_at -= 1;
    }
    for item in list_stack.drain(split_at..) {
        let text = item.lines.join("\n");
        push_element(
            elements,
            item.start_line,
            item.last_line,
            None,
            text,
            heading_stack,
            frontmatter_links,
        );
    }
}

/// Split a note body into paragraph/list-item/heading elements. `frontmatter_links`
/// are the wiki-links found in the document's own frontmatter, unioned into
/// every element unconditionally (a reference in the header applies to the
/// full document). Line numbers are 1-based relative to `body` - the caller
/// is responsible for offsetting by the frontmatter's line count, same as
/// `extract_todo_entries`.
pub fn extract_elements(body: &str, frontmatter_links: &[String]) -> Vec<Element> {
    let lines: Vec<&str> = body.lines().collect();
    let mut elements: Vec<Element> = Vec::new();

    let mut heading_stack: Vec<HeadingFrame> = Vec::new();
    let mut list_stack: Vec<OpenListItem> = Vec::new();
    let mut paragraph_start: Option<usize> = None;
    let mut paragraph_lines: Vec<String> = Vec::new();
    let mut in_fence = false;

    for (idx, raw_line) in lines.iter().enumerate() {
        let line_number = idx + 1;
        let line = *raw_line;

        if FENCE_REGEX.is_match(line) {
            in_fence = !in_fence;
            finalize_paragraph(
                &mut paragraph_start,
                &mut paragraph_lines,
                line_number.saturating_sub(1),
                &heading_stack,
                frontmatter_links,
                &mut elements,
            );
            finalize_list_items_at_or_deeper(
                &mut list_stack,
                0,
                &heading_stack,
                frontmatter_links,
                &mut elements,
            );
            continue;
        }
        if in_fence {
            continue;
        }

        if line.trim().is_empty() {
            finalize_paragraph(
                &mut paragraph_start,
                &mut paragraph_lines,
                line_number.saturating_sub(1),
                &heading_stack,
                frontmatter_links,
                &mut elements,
            );
            continue;
        }

        if let Some(caps) = HEADING_REGEX.captures(line) {
            finalize_paragraph(
                &mut paragraph_start,
                &mut paragraph_lines,
                line_number.saturating_sub(1),
                &heading_stack,
                frontmatter_links,
                &mut elements,
            );
            finalize_list_items_at_or_deeper(
                &mut list_stack,
                0,
                &heading_stack,
                frontmatter_links,
                &mut elements,
            );

            let level = caps[1].len() as u32;
            let text = caps[2].to_string();

            while let Some(top) = heading_stack.last() {
                if top.level >= level {
                    heading_stack.pop();
                } else {
                    break;
                }
            }

            push_element(
                &mut elements,
                line_number,
                line_number,
                Some(level),
                text.clone(),
                &heading_stack,
                frontmatter_links,
            );

            let (own_tags, own_links) = own_tags_links(&text);
            let mut cascade_tags = heading_stack
                .last()
                .map(|f| f.cascade_tags.clone())
                .unwrap_or_default();
            let mut cascade_links = heading_stack
                .last()
                .map(|f| f.cascade_links.clone())
                .unwrap_or_default();
            cascade_tags.extend(own_tags);
            cascade_links.extend(own_links);
            heading_stack.push(HeadingFrame {
                level,
                cascade_tags,
                cascade_links,
            });
            continue;
        }

        if let Some(caps) = LIST_ITEM_REGEX.captures(line) {
            let indent = caps[1].len();
            let item_text = caps[2].to_string();

            finalize_paragraph(
                &mut paragraph_start,
                &mut paragraph_lines,
                line_number.saturating_sub(1),
                &heading_stack,
                frontmatter_links,
                &mut elements,
            );
            finalize_list_items_at_or_deeper(
                &mut list_stack,
                indent,
                &heading_stack,
                frontmatter_links,
                &mut elements,
            );

            for open in list_stack.iter_mut() {
                open.lines.push(item_text.clone());
                open.last_line = line_number;
            }

            list_stack.push(OpenListItem {
                start_line: line_number,
                last_line: line_number,
                indent,
                lines: vec![item_text],
            });
            continue;
        }

        // Plain text line: continuation of the deepest open list item(s) if
        // any are open, otherwise part of the current paragraph.
        if !list_stack.is_empty() {
            for open in list_stack.iter_mut() {
                open.lines.push(line.trim().to_string());
                open.last_line = line_number;
            }
        } else {
            if paragraph_start.is_none() {
                paragraph_start = Some(line_number);
            }
            paragraph_lines.push(line.to_string());
        }
    }

    finalize_paragraph(
        &mut paragraph_start,
        &mut paragraph_lines,
        lines.len(),
        &heading_stack,
        frontmatter_links,
        &mut elements,
    );
    finalize_list_items_at_or_deeper(&mut list_stack, 0, &heading_stack, frontmatter_links, &mut elements);

    elements
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
            let link = capture[1].to_string();
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
    let mapping_config = crate::commands::mapping::MappingConfig::load();
    mapping_config.apply_to_attributes(&mut header_fields);

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

    let mut elements = extract_elements(&body_without_dataview, &frontmatter_links);
    for element in &mut elements {
        element.start_line += frontmatter_line_count;
        element.end_line += frontmatter_line_count;
    }

    let mut body_links = extract_links(&body_with_date_links);

    body_links.extend(frontmatter_links);
    let mut seen = HashSet::new();
    let unique_links: Vec<String> = body_links
        .into_iter()
        .filter(|link| seen.insert(link.clone()))
        .collect();

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
        elements,
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

    // Element-level index: paragraphs, list items (nested children folded
    // in), and headings, with tags/links already resolved (own text, plus
    // cascade from ancestor headings and the document's frontmatter links).
    // No backfill is possible here (unlike note_tags/note_links above) -
    // there's no prior per-line data to reconstruct this from, so this table
    // stays empty for existing notes until they're re-imported.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS elements (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            filename TEXT NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            heading_level INTEGER,
            text TEXT NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_elements_filename ON elements(filename)",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS element_tags (
            element_id INTEGER NOT NULL,
            tag TEXT NOT NULL,
            PRIMARY KEY (element_id, tag)
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS element_links (
            element_id INTEGER NOT NULL,
            link TEXT NOT NULL,
            PRIMARY KEY (element_id, link)
        )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_element_tags_tag ON element_tags(tag)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_element_tags_element_id ON element_tags(element_id)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_element_links_link ON element_links(link)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_element_links_element_id ON element_links(element_id)",
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
    conn.execute(
        "DELETE FROM todo_tags WHERE todo_id IN (SELECT id FROM todo_entries WHERE filename = ?1)",
        rusqlite::params![data.filename],
    )?;
    conn.execute(
        "DELETE FROM todo_links WHERE todo_id IN (SELECT id FROM todo_entries WHERE filename = ?1)",
        rusqlite::params![data.filename],
    )?;

    conn.execute(
        "DELETE FROM todo_entries WHERE filename = ?1",
        rusqlite::params![data.filename],
    )?;

    conn.execute(
        "DELETE FROM note_tags WHERE filename = ?1",
        rusqlite::params![data.filename],
    )?;
    conn.execute(
        "DELETE FROM note_links WHERE filename = ?1",
        rusqlite::params![data.filename],
    )?;

    conn.execute(
        "DELETE FROM element_tags WHERE element_id IN (SELECT id FROM elements WHERE filename = ?1)",
        rusqlite::params![data.filename],
    )?;
    conn.execute(
        "DELETE FROM element_links WHERE element_id IN (SELECT id FROM elements WHERE filename = ?1)",
        rusqlite::params![data.filename],
    )?;
    conn.execute(
        "DELETE FROM elements WHERE filename = ?1",
        rusqlite::params![data.filename],
    )?;

    conn.execute(
        "INSERT OR REPLACE INTO markdown_data
         (filename, created, updated, title, todo_count, link_count, header_fields, links, body, tags)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
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
        ],
    )?;

    for tag in &all_tags {
        conn.execute(
            "INSERT OR IGNORE INTO note_tags (filename, tag) VALUES (?1, ?2)",
            rusqlite::params![data.filename, tag],
        )?;
    }
    for link in &data.link {
        conn.execute(
            "INSERT OR IGNORE INTO note_links (filename, link) VALUES (?1, ?2)",
            rusqlite::params![data.filename, link],
        )?;
    }

    for todo in &data.todo {
        let tags_json = serde_json::to_string(&todo.tags)?;
        let links_json = serde_json::to_string(&todo.links)?;

        conn.execute(
            "INSERT INTO todo_entries
             (filename, closed, priority, due, text, tags, links, line_number, updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                data.filename,
                todo.closed,
                todo.priority.as_deref(),
                todo.due.as_deref(),
                todo.text,
                tags_json,
                links_json,
                todo.line_number as i64,
                todo.updated
            ],
        )?;

        let todo_id = conn.last_insert_rowid();
        for tag in &todo.tags {
            conn.execute(
                "INSERT OR IGNORE INTO todo_tags (todo_id, tag) VALUES (?1, ?2)",
                rusqlite::params![todo_id, tag],
            )?;
        }
        for link in &todo.links {
            conn.execute(
                "INSERT OR IGNORE INTO todo_links (todo_id, link) VALUES (?1, ?2)",
                rusqlite::params![todo_id, link],
            )?;
        }
    }

    for element in &data.elements {
        conn.execute(
            "INSERT INTO elements (filename, start_line, end_line, heading_level, text)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                data.filename,
                element.start_line as i64,
                element.end_line as i64,
                element.heading_level.map(|l| l as i64),
                element.text,
            ],
        )?;

        let element_id = conn.last_insert_rowid();
        for tag in &element.tags {
            conn.execute(
                "INSERT OR IGNORE INTO element_tags (element_id, tag) VALUES (?1, ?2)",
                rusqlite::params![element_id, tag],
            )?;
        }
        for link in &element.links {
            conn.execute(
                "INSERT OR IGNORE INTO element_links (element_id, link) VALUES (?1, ?2)",
                rusqlite::params![element_id, link],
            )?;
        }
    }

    Ok(())
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
                "DELETE FROM element_tags WHERE element_id IN (SELECT id FROM elements WHERE filename = ?1)",
                rusqlite::params![filename],
            )?;
            conn.execute(
                "DELETE FROM element_links WHERE element_id IN (SELECT id FROM elements WHERE filename = ?1)",
                rusqlite::params![filename],
            )?;
            conn.execute(
                "DELETE FROM elements WHERE filename = ?1",
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

        match process_markdown_file(&file_path, input_dir) {
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

    for entry in walkdir::WalkDir::new(input_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|ext| ext == "md"))
    {
        let data = process_markdown_file(entry.path(), input_dir)?;
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
    fn test_extract_elements_nested_bullet() {
        let body = "- [[NeoVimNote]] project reference\n    * Sup note with indirect reference\n- [[Auto]] Reference to something else\n";
        let elements = extract_elements(body, &[]);

        // 3 elements: the parent bullet (incl. its child's text), the child
        // bullet on its own, and the second top-level bullet.
        assert_eq!(elements.len(), 3);

        let parent = &elements[0];
        assert_eq!(parent.start_line, 1);
        assert_eq!(parent.end_line, 2);
        assert_eq!(
            parent.text,
            "[[NeoVimNote]] project reference\nSup note with indirect reference"
        );
        assert_eq!(parent.links, vec!["NeoVimNote".to_string()]);

        let child = &elements[1];
        assert_eq!(child.start_line, 2);
        assert_eq!(child.end_line, 2);
        assert_eq!(child.text, "Sup note with indirect reference");
        assert!(child.links.is_empty());

        let second = &elements[2];
        assert_eq!(second.start_line, 3);
        assert_eq!(second.links, vec!["Auto".to_string()]);
    }

    #[test]
    fn test_extract_elements_paragraph() {
        let body = "First line of a paragraph\nsecond line, still the same paragraph.\n\nA new paragraph #tagged.\n";
        let elements = extract_elements(body, &[]);

        assert_eq!(elements.len(), 2);
        assert_eq!(
            elements[0].text,
            "First line of a paragraph\nsecond line, still the same paragraph."
        );
        assert_eq!(elements[0].start_line, 1);
        assert_eq!(elements[0].end_line, 2);

        assert_eq!(elements[1].text, "A new paragraph #tagged.");
        assert_eq!(elements[1].start_line, 4);
        assert_eq!(elements[1].tags, vec!["tagged".to_string()]);
    }

    #[test]
    fn test_extract_elements_heading_cascade() {
        let body = "\
# Section A #alpha

Text under A.

## Section B

Text under B, inherits from its parent section.

# Section C

Text under C, a sibling section that starts fresh.
";
        let elements = extract_elements(body, &[]);

        let text_under_a = elements
            .iter()
            .find(|e| e.text == "Text under A.")
            .unwrap();
        assert!(text_under_a.tags.contains(&"alpha".to_string()));

        let text_under_b = elements
            .iter()
            .find(|e| e.text.starts_with("Text under B"))
            .unwrap();
        assert!(text_under_b.tags.contains(&"alpha".to_string()));

        let text_under_c = elements
            .iter()
            .find(|e| e.text.starts_with("Text under C"))
            .unwrap();
        assert!(!text_under_c.tags.contains(&"alpha".to_string()));
    }

    #[test]
    fn test_extract_elements_frontmatter_cascade() {
        let body = "Some paragraph.\n\n- A bullet\n";
        let elements = extract_elements(body, &["ProjectX".to_string()]);

        assert_eq!(elements.len(), 2);
        for element in &elements {
            assert!(element.links.contains(&"ProjectX".to_string()));
        }
    }

    #[test]
    fn test_extract_elements_skips_fenced_code() {
        let body = "Before.\n\n```rust\nlet x = \"[[NotALink]] #notatag\";\n```\n\nAfter.\n";
        let elements = extract_elements(body, &[]);

        assert_eq!(elements.len(), 2);
        assert_eq!(elements[0].text, "Before.");
        assert_eq!(elements[1].text, "After.");
        for element in &elements {
            assert!(!element.links.contains(&"NotALink".to_string()));
            assert!(!element.tags.contains(&"notatag".to_string()));
        }
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
        assert!(todos[0].links.contains(&"Related Page".to_string()));
        assert!(todos[0].links.contains(&"Another Page".to_string()));
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
        assert!(links.contains(&"Page One".to_string()));
        assert!(links.contains(&"Page Two".to_string()));
    }

    #[test]
    fn test_extract_links_mixed() {
        let content = "[Web link](https://example.com) and [[Wiki link]]";
        let links = extract_links(content);
        assert!(links.contains(&"https://example.com".to_string()));
        assert!(links.contains(&"Wiki link".to_string()));
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
            elements: vec![],
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
        assert!(links.contains(&"Wiki Link".to_string()));
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
        assert!(links.contains(&"Wiki Link".to_string()));
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
            elements: vec![],
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
            elements: vec![],
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
