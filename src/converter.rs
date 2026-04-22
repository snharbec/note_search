use chrono::Local;
use docx_rs::{DocumentChild, ParagraphChild, RunChild};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

/// Metadata for a converted note
pub struct NoteMetadata {
    pub note_type: String,
    pub source: String,
    pub title: Option<String>,
    pub created: String,
    pub from: Option<String>,
    pub to: Option<Vec<String>>,
    pub mail_date: Option<String>,
    pub mail_date_dir: Option<String>,
    pub mail_date_file: Option<String>,
}

/// Convert a web page to markdown
pub fn convert_web_page(url: &str) -> Result<(String, NoteMetadata), Box<dyn Error>> {
    // Fetch the web page with a timeout to avoid hanging on unresponsive hosts
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let response = client.get(url).send()?;
    let html = response.text()?;

    // Parse the URL to extract the page name
    let parsed_url = url::Url::parse(url)?;
    let _page_name = parsed_url
        .path_segments()
        .and_then(|segments| segments.last())
        .unwrap_or("unnamed")
        .to_string();

    // Extract title from HTML
    let title = extract_title_from_html(&html);

    // Extract main content using readability-like approach
    let main_content = extract_main_content(&html);

    // Convert HTML to Markdown
    let markdown = html2md::parse_html(&main_content);

    let metadata = NoteMetadata {
        note_type: "web".to_string(),
        source: url.to_string(),
        title,
        created: Local::now().format("%Y-%m-%d %H:%M").to_string(),
        from: None,
        to: None,
        mail_date: None,
        mail_date_dir: None,
        mail_date_file: None,
    };

    Ok((markdown, metadata))
}

/// Convert a local document to markdown
pub fn convert_document(path: &Path) -> Result<(String, NoteMetadata), Box<dyn Error>> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let (markdown, title) = match extension.as_str() {
        "docx" => convert_docx(path)?,
        "pdf" => convert_pdf(path)?,
        "html" | "htm" => convert_html_file(path)?,
        "eml" => return convert_email(path),
        "msg" => return convert_msg(path),
        _ => convert_text_file(path)?,
    };

    let metadata = NoteMetadata {
        note_type: "document".to_string(),
        source: path.to_string_lossy().to_string(),
        title,
        created: Local::now().format("%Y-%m-%d %H:%M").to_string(),
        from: None,
        to: None,
        mail_date: None,
        mail_date_dir: None,
        mail_date_file: None,
    };

    Ok((markdown, metadata))
}

/// Create a note file with frontmatter
pub fn create_note(
    content: &str,
    metadata: &NoteMetadata,
    output_dir: &Path,
) -> Result<PathBuf, Box<dyn Error>> {
    // Determine subdirectory based on type
    let subdir: String = match metadata.note_type.as_str() {
        "web" => "web".to_string(),
        "reddit" => "web".to_string(),
        "document" => "documents".to_string(),
        "mail" => metadata
            .mail_date_dir
            .clone()
            .unwrap_or_else(|| format!("mail/{}", Local::now().format("%Y/%b"))),
        _ => "notes".to_string(),
    };

    // Create the full output directory path
    let full_output_dir = output_dir.join(&subdir);
    fs::create_dir_all(&full_output_dir)?;

    // Generate filename using the mail date if available
    let date = match metadata.note_type.as_str() {
        "mail" => metadata
            .mail_date_file
            .clone()
            .unwrap_or_else(|| Local::now().format("%Y-%m-%d").to_string()),
        _ => Local::now().format("%Y-%m-%d").to_string(),
    };

    let name_part = extract_name_from_source(&metadata.source, &metadata.note_type);
    let filename = format!("{}-{}-{}.md", metadata.note_type, date, name_part);
    let file_path = full_output_dir.join(&filename);

    // Generate frontmatter
    let frontmatter = generate_frontmatter(metadata);

    // Write the note file
    let full_content = format!("{}\n{}", frontmatter, content);
    fs::write(&file_path, full_content)?;

    // If it's a document, copy the original file
    if metadata.note_type == "document" {
        let original_path = Path::new(&metadata.source);
        if original_path.exists() {
            let original_filename = original_path.file_name().unwrap_or("original".as_ref());
            let original_dest = full_output_dir.join(original_filename);
            fs::copy(original_path, original_dest)?;
        }
    }

    Ok(file_path)
}

/// Extract title from HTML
fn extract_title_from_html(html: &str) -> Option<String> {
    let document = scraper::Html::parse_document(html);
    let title_selector = scraper::Selector::parse("title").ok()?;

    document
        .select(&title_selector)
        .next()
        .map(|element| element.text().collect::<String>().trim().to_string())
}

/// Extract main content from HTML (reader mode)
fn extract_main_content(html: &str) -> String {
    let document = scraper::Html::parse_document(html);

    // Try to find main content area
    let selectors = [
        "main",
        "article",
        "[role='main']",
        ".content",
        "#content",
        ".main",
        "#main",
        ".article",
        ".post",
        ".entry",
    ];

    for selector_str in &selectors {
        if let Ok(selector) = scraper::Selector::parse(selector_str) {
            if let Some(element) = document.select(&selector).next() {
                return element.html();
            }
        }
    }

    // Fallback: extract body content
    if let Ok(body_selector) = scraper::Selector::parse("body") {
        if let Some(body) = document.select(&body_selector).next() {
            return body.html();
        }
    }

    // Last resort: return full HTML
    html.to_string()
}

/// Convert Word document to markdown
fn convert_docx(path: &Path) -> Result<(String, Option<String>), Box<dyn Error>> {
    // Read file content
    let content = fs::read(path)?;
    let docx = docx_rs::read_docx(&content)?;

    let mut markdown = String::new();
    let mut title = None;

    // Extract paragraphs from document children
    for child in docx.document.children {
        if let DocumentChild::Paragraph(paragraph) = child {
            let text = extract_text_from_paragraph(&paragraph);

            if !text.trim().is_empty() {
                // Check if this might be a title (first non-empty paragraph)
                if title.is_none() && text.len() < 200 {
                    title = Some(text.clone());
                }

                // Simple heuristic: if paragraph is short and doesn't end with punctuation, treat as heading
                let is_heading = text.len() < 100
                    && !text.ends_with('.')
                    && !text.ends_with('?')
                    && !text.ends_with('!');

                if is_heading && title.as_ref() == Some(&text) {
                    markdown.push_str(&format!("# {}\n\n", text));
                } else {
                    markdown.push_str(&format!("{}\n\n", text));
                }
            }
        }
    }

    Ok((markdown, title))
}

/// Helper function to extract text from a paragraph
fn extract_text_from_paragraph(paragraph: &docx_rs::Paragraph) -> String {
    let mut text = String::new();

    for child in &paragraph.children {
        if let ParagraphChild::Run(run) = child {
            for run_child in &run.children {
                if let RunChild::Text(text_content) = run_child {
                    text.push_str(&text_content.text);
                }
            }
        }
    }

    text
}

/// Convert PDF to markdown
fn convert_pdf(path: &Path) -> Result<(String, Option<String>), Box<dyn Error>> {
    let doc = lopdf::Document::load(path)?;
    let mut markdown = String::new();
    let mut title = None;
    let mut is_first_text = true;

    // Get page numbers and extract text
    let pages: Vec<u32> = doc.get_pages().keys().copied().collect();

    for (page_num, page_id) in pages.iter().enumerate() {
        // Create a vector with the page ID for extract_text
        let page_ids = vec![*page_id];
        if let Ok(text) = doc.extract_text(&page_ids) {
            let trimmed_text = text.trim();
            if !trimmed_text.is_empty() {
                // Try to identify title from first page's first paragraph
                if is_first_text && page_num == 0 {
                    let first_para = trimmed_text.lines().next().unwrap_or("").trim();
                    if !first_para.is_empty() && first_para.len() < 200 {
                        title = Some(first_para.to_string());
                        markdown.push_str(&format!("# {}\n\n", first_para));
                    } else {
                        markdown.push_str(&format!("{}\n\n", trimmed_text));
                    }
                    is_first_text = false;
                } else {
                    markdown.push_str(&format!("{}\n\n", trimmed_text));
                }
            }
        }
    }

    Ok((markdown, title))
}

/// Convert HTML file to markdown
fn convert_html_file(path: &Path) -> Result<(String, Option<String>), Box<dyn Error>> {
    let html = fs::read_to_string(path)?;
    let title = extract_title_from_html(&html);
    let main_content = extract_main_content(&html);
    let markdown = html2md::parse_html(&main_content);

    Ok((markdown, title))
}

/// Convert text file to markdown
fn convert_text_file(path: &Path) -> Result<(String, Option<String>), Box<dyn Error>> {
    let content = fs::read_to_string(path)?;

    // Try to extract title from first line
    let title = content.lines().next().map(|line| line.trim().to_string());

    // Convert to markdown (mostly just preserve as-is, but wrap in paragraphs)
    let markdown = content
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                "\n".to_string()
            } else {
                format!("{}\n\n", trimmed)
            }
        })
        .collect();

    Ok((markdown, title))
}

/// Convert EML email file to markdown
pub fn convert_email(path: &Path) -> Result<(String, NoteMetadata), Box<dyn Error>> {
    let bytes = fs::read(path)?;
    let message = mail_parser::MessageParser::default()
        .parse(&bytes)
        .ok_or("Failed to parse EML file")?;

    let mut markdown = String::new();

    // Subject as title
    let subject = message
        .subject()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "(No Subject)".to_string());

    // Extract from
    let from = message.from().map(format_address_list);

    // Extract to recipients as a list
    let to = message
        .to()
        .map(|addr| addr.iter().map(format_single_addr).collect::<Vec<_>>());

    // Date from email header
    let mail_date = message.date().map(|d| d.to_rfc3339());

    // Format date for directory: mail/YYYY/Mon (e.g. mail/2026/Apr)
    let mail_date_dir = message
        .date()
        .map(|d| format!("mail/{}/{}", d.year, month_short(d.month as usize)));

    // Format date for filename: YYYY-MM-DD
    let mail_date_file = message
        .date()
        .map(|d| format!("{:04}-{:02}-{:02}", d.year, d.month, d.day));

    // Use email date for created timestamp, fallback to now
    let created = message
        .date()
        .map(|d| {
            format!(
                "{:04}-{:02}-{:02} {:02}:{:02}",
                d.year, d.month, d.day, d.hour, d.minute
            )
        })
        .unwrap_or_else(|| Local::now().format("%Y-%m-%d %H:%M").to_string());

    // Header section
    markdown.push_str(&format!("# {}\n\n", subject));

    // From
    if let Some(ref from_str) = from {
        markdown.push_str(&format!("**From:** {}\n\n", from_str));
    }

    // To
    if let Some(ref to_list) = to {
        if !to_list.is_empty() {
            markdown.push_str(&format!("**To:** {}\n\n", to_list.join(", ")));
        }
    }

    // Cc
    if let Some(cc) = message.cc() {
        let cc_str = format_address_list(cc);
        if !cc_str.is_empty() {
            markdown.push_str(&format!("**Cc:** {}\n\n", cc_str));
        }
    }

    // Date
    if let Some(date) = message.date() {
        markdown.push_str(&format!("**Date:** {}\n\n", date.to_rfc3339()));
    }

    // Message-ID
    if let Some(msg_id) = message.message_id() {
        markdown.push_str(&format!("**Message-ID:** {}\n\n", msg_id));
    }

    markdown.push_str("---\n\n");

    // Body - prefer HTML converted to markdown, fallback to plain text
    if let Some(html_body) = message.body_html(0) {
        let md = html2md::parse_html(&html_body);
        markdown.push_str(&md);
    } else if let Some(text_body) = message.body_text(0) {
        markdown.push_str(&text_body);
    }

    // Attachments info
    let attachment_count = message.attachment_count();
    if attachment_count > 0 {
        use mail_parser::MimeHeaders;
        markdown.push_str("\n\n---\n\n## Attachments\n\n");
        for i in 0..attachment_count {
            if let Some(attachment) = message.attachment(i as u32) {
                let name = attachment.attachment_name().unwrap_or("(unnamed)");
                if let Some(ct) = attachment.content_type() {
                    let content_desc = if let Some(ref subtype) = ct.c_subtype {
                        format!("{}/{}", ct.c_type, subtype)
                    } else {
                        ct.c_type.to_string()
                    };
                    markdown.push_str(&format!("- **{}** ({})\n", name, content_desc));
                } else {
                    markdown.push_str(&format!("- **{}**\n", name));
                }
            }
        }
    }

    let metadata = NoteMetadata {
        note_type: "mail".to_string(),
        source: path.to_string_lossy().to_string(),
        title: Some(subject),
        created,
        from,
        to,
        mail_date,
        mail_date_dir,
        mail_date_file,
    };

    Ok((markdown, metadata))
}

/// Convert Outlook MSG email file to markdown
pub fn convert_msg(path: &Path) -> Result<(String, NoteMetadata), Box<dyn Error>> {
    use chrono::{Datelike, Timelike};
    use msg_parser::Outlook;

    let outlook = Outlook::from_path(path)?;

    let mut markdown = String::new();

    // Subject as title
    let subject = if outlook.subject.is_empty() {
        "(No Subject)".to_string()
    } else {
        outlook.subject.clone()
    };

    // Extract sender - use wiki link if only email is available (no display name)
    let from = if outlook.sender.name.is_empty() && outlook.sender.email.is_empty() {
        None
    } else {
        let sender_str = if outlook.sender.name.is_empty() {
            // Only email available - convert to person wiki link
            email_to_person_link(&outlook.sender.email)
        } else {
            // Use display name (with email if available)
            if outlook.sender.email.is_empty() {
                outlook.sender.name.clone()
            } else {
                format!("{} <{}>", outlook.sender.name, outlook.sender.email)
            }
        };
        Some(sender_str)
    };

    // Extract recipients - use wiki link if only email is available
    let to: Option<Vec<String>> = if outlook.to.is_empty() {
        None
    } else {
        Some(
            outlook
                .to
                .iter()
                .map(|p| {
                    if p.name.is_empty() {
                        // Only email available - convert to person wiki link
                        email_to_person_link(&p.email)
                    } else if p.email.is_empty() {
                        p.name.clone()
                    } else {
                        format!("{} <{}>", p.name, p.email)
                    }
                })
                .collect(),
        )
    };

    // Extract CC recipients (stored in metadata but not shown in markdown body)
    let _cc: Option<Vec<String>> = if outlook.cc.is_empty() {
        None
    } else {
        Some(outlook.cc.iter().map(|p| p.to_string()).collect())
    };

    // Date from message delivery time (ISO 8601 string)
    let mail_date = if outlook.message_delivery_time.is_empty() {
        None
    } else {
        Some(outlook.message_delivery_time.clone())
    };

    // Format date for directory: mail/YYYY/Mon (e.g. mail/2026/Apr)
    let mail_date_dir = if outlook.message_delivery_time.is_empty() {
        None
    } else {
        // Parse ISO 8601 date string to extract year and month
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&outlook.message_delivery_time) {
            Some(format!(
                "mail/{}/{}",
                dt.year(),
                month_short(dt.month() as usize)
            ))
        } else {
            None
        }
    };

    // Format date for filename: YYYY-MM-DD
    let mail_date_file = if outlook.message_delivery_time.is_empty() {
        None
    } else {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&outlook.message_delivery_time) {
            Some(format!(
                "{:04}-{:02}-{:02}",
                dt.year(),
                dt.month(),
                dt.day()
            ))
        } else {
            None
        }
    };

    // Use email date for created timestamp, fallback to now
    let created = if outlook.message_delivery_time.is_empty() {
        Local::now().format("%Y-%m-%d %H:%M").to_string()
    } else {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&outlook.message_delivery_time) {
            format!(
                "{:04}-{:02}-{:02} {:02}:{:02}",
                dt.year(),
                dt.month(),
                dt.day(),
                dt.hour(),
                dt.minute()
            )
        } else {
            Local::now().format("%Y-%m-%d %H:%M").to_string()
        }
    };

    // Only output subject heading and body content
    markdown.push_str(&format!("# {}\n\n", subject));

    // Body - prefer HTML converted to markdown, fallback to plain text, then RTF converted to HTML
    let raw_body = if !outlook.html.is_empty() {
        html2md::parse_html(&outlook.html)
    } else if !outlook.body.is_empty() {
        outlook.body.clone()
    } else if !outlook.rtf_compressed.is_empty() {
        // Convert RTF to HTML then to markdown
        outlook
            .html_from_rtf()
            .map(|html| html2md::parse_html(&html))
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Filter out font/style declaration lines (e.g., "Times New Roman", font names, CSS properties)
    let body_content: String = raw_body
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.starts_with("Times New Roman")
                && !trimmed.starts_with("Arial")
                && !trimmed.starts_with("Calibri")
                && !trimmed.starts_with("font-family:")
                && !trimmed.starts_with("font-size:")
                && !trimmed.starts_with("font-style:")
                && !trimmed.starts_with("font-weight:")
                && !trimmed.starts_with("color:")
                && !trimmed.starts_with("background-color:")
                && !trimmed.starts_with("mso-")
                && !trimmed.starts_with("@")
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Unescape markdown escape sequences (\\* -> *, \\_ -> _, etc.)
    let body_content = body_content
        .replace("\\*", "*")
        .replace("\\_", "_")
        .replace("\\[", "[")
        .replace("\\]", "]")
        .replace("\\(", "(")
        .replace("\\)", ")")
        .replace("\\{", "{")
        .replace("\\}", "}")
        .replace("\\#", "#")
        .replace("\\+", "+")
        .replace("\\-", "-")
        .replace("\\.", ".")
        .replace("\\!", "!")
        .replace("\\|", "|")
        .replace("\\\\", "\\");

    // Remove trailing asterisks from lines (multiple * at end of lines)
    let body_content: String = body_content
        .lines()
        .map(|line| {
            let trimmed = line.trim_end();
            let without_trailing_asterisks = trimmed.trim_end_matches('*');
            without_trailing_asterisks.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");

    markdown.push_str(&body_content);

    let metadata = NoteMetadata {
        note_type: "mail".to_string(),
        source: path.to_string_lossy().to_string(),
        title: Some(subject),
        created,
        from,
        to,
        mail_date,
        mail_date_dir,
        mail_date_file,
    };

    Ok((markdown, metadata))
}

fn month_short(month: usize) -> &'static str {
    match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "Jan",
    }
}

/// Format an Address type into a display string
fn format_address_list(addr: &mail_parser::Address) -> String {
    addr.iter()
        .map(format_single_addr)
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_single_addr(addr: &mail_parser::Addr) -> String {
    match (addr.name(), addr.address()) {
        (Some(name), Some(_email)) => name.to_string(),
        (Some(name), None) => name.to_string(),
        (None, Some(email)) => email_to_person_link(email),
        (None, None) => "(unknown)".to_string(),
    }
}

/// Check if a source is a URL
pub fn is_url(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://")
}

/// Generate YAML frontmatter
fn generate_frontmatter(metadata: &NoteMetadata) -> String {
    let title_line = metadata
        .title
        .as_ref()
        .map(|t| format!("title: \"{}\"\n", t.replace('"', "\\\"")))
        .unwrap_or_default();

    let from_line = metadata
        .from
        .as_ref()
        .map(|f| format!("from: \"{}\"\n", f.replace('"', "\\\"")))
        .unwrap_or_default();

    let to_line = match &metadata.to {
        Some(recipients) if !recipients.is_empty() => {
            let items: Vec<String> = recipients
                .iter()
                .map(|r| format!("  - \"{}\"", r.replace('"', "\\\"")))
                .collect();
            format!("to:\n{}\n", items.join("\n"))
        }
        _ => String::new(),
    };

    let mail_date_line = metadata
        .mail_date
        .as_ref()
        .map(|d| format!("mail_date: \"{}\"\n", d.replace('"', "\\\"")))
        .unwrap_or_default();

    format!(
        "---\ncreated: {}\ntype: {}\nsource: \"{}\"\n{}{}{}{}---\n\n",
        metadata.created,
        metadata.note_type,
        metadata.source.replace('"', "\\\""),
        title_line,
        from_line,
        to_line,
        mail_date_line,
    )
}

/// Convert an email address to a person wiki link [[PERSON]]
/// - Ignore domain (everything after @)
/// - Replace dots with spaces
fn email_to_person_link(email: &str) -> String {
    // Extract local part (before @)
    let local_part = email.split('@').next().unwrap_or(email);
    // Replace dots with underscores and wrap in wiki link
    let person_name = local_part.replace('.', "_");
    format!("[[{}]]", person_name)
}

/// Extract a clean name from the source for filename
fn extract_name_from_source(source: &str, note_type: &str) -> String {
    match note_type {
        "web" | "reddit" => {
            // For URLs, extract the last path segment
            if let Ok(url) = url::Url::parse(source) {
                url.path_segments()
                    .and_then(|segments| segments.last())
                    .and_then(|s| {
                        let s = s.trim();
                        if s.is_empty() {
                            None
                        } else {
                            Some(s.to_string())
                        }
                    })
                    .unwrap_or_else(|| "unnamed".to_string())
            } else {
                "unnamed".to_string()
            }
        }
        "document" | "mail" => {
            // For documents and mail, extract filename without extension
            Path::new(source)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "unnamed".to_string())
        }
        _ => "unnamed".to_string(),
    }
    .replace(" ", "_")
    .replace("/", "_")
    .replace("\\", "_")
}

/// Check if URL is from Reddit
pub fn is_reddit_url(url: &str) -> bool {
    url.contains("reddit.com") || url.contains("redd.it")
}

/// Convert Reddit discussion to markdown using the JSON API
/// Supports browser cookie authentication for private subreddits
pub fn convert_reddit_discussion(url: &str) -> Result<(String, NoteMetadata), Box<dyn Error>> {
    // Build JSON API URL
    let json_url = build_reddit_json_url(url);
    eprintln!("Fetching Reddit JSON API: {}", json_url);

    // Build client with proper headers
    let client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    // Try to fetch JSON with optional authentication
    let cookie_file = std::env::var("REDDIT_COOKIE_FILE").ok();

    let request_builder = client
        .get(&json_url)
        .header("Accept", "application/json")
        .header("Accept-Language", "en-US,en;q=0.5");

    // Add cookies if available
    let request_builder = if let Some(ref cookie_path) = cookie_file {
        match load_browser_cookies(cookie_path) {
            Ok(cookies) => {
                eprintln!("Using browser cookies from: {}", cookie_path);
                let cookie_header = build_cookie_header(&cookies);
                request_builder.header("Cookie", cookie_header)
            }
            Err(e) => {
                eprintln!("Warning: Failed to load cookie file: {}", e);
                request_builder
            }
        }
    } else {
        request_builder
    };

    let response = request_builder.send()?;
    let status = response.status();

    if !status.is_success() {
        return Err(format!("Reddit API returned error status: {}", status).into());
    }

    let json_data: serde_json::Value = response.json()?;

    // Parse the Reddit JSON structure
    let (post, comments) = parse_reddit_json(&json_data)?;

    // Convert to markdown
    let markdown = format_reddit_to_markdown(&post, &comments);

    // Extract title from post
    let title = post["title"].as_str().map(|s| s.to_string());

    let metadata = NoteMetadata {
        note_type: "reddit".to_string(),
        source: url.to_string(),
        title,
        created: Local::now().format("%Y-%m-%d %H:%M").to_string(),
        from: None,
        to: None,
        mail_date: None,
        mail_date_dir: None,
        mail_date_file: None,
    };

    Ok((markdown, metadata))
}

/// Build JSON API URL from regular Reddit URL
fn build_reddit_json_url(url: &str) -> String {
    // Reddit JSON API is accessed by appending .json to the URL
    // Remove any trailing slashes and append .json
    let url = url.trim_end_matches('/');

    // Handle different URL formats
    if url.contains("?") {
        // URL has query parameters, insert .json before them
        let parts: Vec<&str> = url.split('?').collect();
        format!("{}.json?{}", parts[0], parts[1])
    } else {
        // Simple URL, just append .json
        format!("{}.json", url)
    }
}

/// Parse Reddit JSON response into post and comments
fn parse_reddit_json(
    json: &serde_json::Value,
) -> Result<(serde_json::Value, Vec<serde_json::Value>), Box<dyn Error>> {
    // Reddit JSON structure for a post with comments:
    // [post_listing, comments_listing]

    let listings = json
        .as_array()
        .ok_or("Invalid Reddit JSON: expected array")?;

    if listings.len() < 1 {
        return Err("Invalid Reddit JSON: no listings found".into());
    }

    // Extract post data from first listing
    let post_listing = &listings[0];
    let post = post_listing["data"]["children"][0]["data"].clone();

    // Extract comments from second listing (if available)
    let comments = if listings.len() >= 2 {
        extract_comments_recursive(&listings[1]["data"]["children"])
    } else {
        Vec::new()
    };

    Ok((post, comments))
}

/// Recursively extract all comments from the JSON structure
fn extract_comments_recursive(children: &serde_json::Value) -> Vec<serde_json::Value> {
    let mut comments = Vec::new();

    if let Some(children_array) = children.as_array() {
        for child in children_array {
            let kind = child["kind"].as_str().unwrap_or("");

            match kind {
                "t1" => {
                    // This is a comment
                    let comment_data = child["data"].clone();
                    comments.push(comment_data.clone());

                    // Recursively extract replies
                    if let Some(replies) = comment_data["replies"].as_object() {
                        if !replies.is_empty() {
                            let nested = extract_comments_recursive(&replies["data"]["children"]);
                            comments.extend(nested);
                        }
                    }
                }
                "more" => {
                    // This indicates there are more comments not loaded
                    // We could potentially fetch them, but for now we skip
                }
                _ => {}
            }
        }
    }

    comments
}

/// Format Reddit post and comments as markdown
fn format_reddit_to_markdown(post: &serde_json::Value, comments: &[serde_json::Value]) -> String {
    let mut markdown = String::new();

    // Post title
    if let Some(title) = post["title"].as_str() {
        markdown.push_str(&format!("# {}\n\n", title));
    }

    // Post metadata
    if let Some(author) = post["author"].as_str() {
        markdown.push_str(&format!("**Author:** u/{}  \n", author));
    }
    if let Some(subreddit) = post["subreddit"].as_str() {
        markdown.push_str(&format!("**Subreddit:** r/{}  \n", subreddit));
    }
    if let Some(score) = post["score"].as_i64() {
        markdown.push_str(&format!("**Score:** {}  \n", score));
    }
    if let Some(created_utc) = post["created_utc"].as_f64() {
        let timestamp = created_utc as i64;
        let datetime = chrono::DateTime::from_timestamp(timestamp, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        markdown.push_str(&format!("**Posted:** {}  \n", datetime));
    }
    markdown.push_str("\n");

    // Post content (selftext)
    if let Some(selftext) = post["selftext"].as_str() {
        if !selftext.is_empty() && selftext != "[deleted]" {
            markdown.push_str("## Original Post\n\n");
            markdown.push_str(selftext);
            markdown.push_str("\n\n---\n\n");
        }
    }

    // Post URL (if it's a link post)
    if let Some(url) = post["url"].as_str() {
        let permalink = post["permalink"].as_str().unwrap_or("");
        // Only add URL if it's different from the permalink (meaning it's a link post)
        if !permalink.is_empty() && !url.contains("reddit.com") {
            markdown.push_str(&format!("**Link:** [{}]({})\n\n", url, url));
        }
    }

    // Comments section
    markdown.push_str("## Comments\n\n");

    if comments.is_empty() {
        markdown.push_str("*No comments found.*\n\n");
    } else {
        for (i, comment) in comments.iter().enumerate() {
            format_comment(&mut markdown, comment, i + 1);
        }
    }

    markdown
}

/// Format a single comment as markdown
fn format_comment(markdown: &mut String, comment: &serde_json::Value, index: usize) {
    let author = comment["author"].as_str().unwrap_or("[deleted]");
    let body = comment["body"].as_str().unwrap_or("");
    let score = comment["score"].as_i64().unwrap_or(0);

    // Skip deleted/removed comments with no content
    if body.is_empty() || body == "[deleted]" || body == "[removed]" {
        return;
    }

    // Comment header
    markdown.push_str(&format!("### Comment {} (by u/{})\n\n", index, author));

    // Comment metadata
    if let Some(created_utc) = comment["created_utc"].as_f64() {
        let timestamp = created_utc as i64;
        let datetime = chrono::DateTime::from_timestamp(timestamp, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        markdown.push_str(&format!("*Score: {} | Posted: {}*\n\n", score, datetime));
    }

    // Comment body
    markdown.push_str(body);
    markdown.push_str("\n\n");
}

/// Load browser cookies from a Netscape-format cookie file
fn load_browser_cookies(path: &str) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let content = fs::read_to_string(path)?;
    let mut cookies = Vec::new();

    for line in content.lines() {
        // Skip comments and empty lines
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Parse Netscape cookie format:
        // domain	flag	path	secure	expiration	name	value
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 7 {
            let domain = parts[0];
            let name = parts[5];
            let value = parts[6];

            // Only include reddit cookies
            if domain.contains("reddit") {
                cookies.push((name.to_string(), value.to_string()));
            }
        }
    }

    if cookies.is_empty() {
        return Err("No Reddit cookies found in cookie file".into());
    }

    eprintln!("Loaded {} Reddit cookies from file", cookies.len());
    Ok(cookies)
}

/// Build Cookie header string from cookie pairs
fn build_cookie_header(cookies: &[(String, String)]) -> String {
    cookies
        .iter()
        .map(|(name, value)| format!("{}={}", name, value))
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_url() {
        assert!(is_url("https://example.com"));
        assert!(is_url("http://example.com"));
        assert!(!is_url("/path/to/file.txt"));
        assert!(!is_url("file.txt"));
    }

    #[test]
    fn test_is_reddit_url() {
        assert!(is_reddit_url("https://www.reddit.com/r/rust/comments/xyz"));
        assert!(is_reddit_url("https://redd.it/xyz"));
        assert!(!is_reddit_url("https://example.com"));
    }

    #[test]
    fn test_extract_name_from_source_web() {
        let name = extract_name_from_source("https://example.com/page/article-name", "web");
        assert_eq!(name, "article-name");
    }

    #[test]
    fn test_extract_name_from_source_document() {
        let name = extract_name_from_source("/path/to/My Document.docx", "document");
        assert_eq!(name, "My_Document");
    }

    #[test]
    fn test_generate_frontmatter() {
        let metadata = NoteMetadata {
            note_type: "web".to_string(),
            source: "https://example.com".to_string(),
            title: Some("Test Title".to_string()),
            created: "2026-04-12 14:30".to_string(),
            from: None,
            to: None,
            mail_date: None,
            mail_date_dir: None,
            mail_date_file: None,
        };

        let frontmatter = generate_frontmatter(&metadata);
        assert!(frontmatter.contains("type: web"));
        assert!(frontmatter.contains("created: 2026-04-12 14:30"));
        assert!(frontmatter.contains("source: \"https://example.com\""));
        assert!(frontmatter.contains("title: \"Test Title\""));
        assert!(!frontmatter.contains("from:"));
        assert!(!frontmatter.contains("to:"));
    }

    #[test]
    fn test_generate_frontmatter_with_mail_fields() {
        let metadata = NoteMetadata {
            note_type: "mail".to_string(),
            source: "/path/to/email.eml".to_string(),
            title: Some("Meeting tomorrow".to_string()),
            created: "2026-04-13 10:30".to_string(),
            from: Some("John Doe <john@example.com>".to_string()),
            to: Some(vec![
                "jane@example.com".to_string(),
                "bob@example.com".to_string(),
            ]),
            mail_date: Some("2026-04-13T08:30:00+02:00".to_string()),
            mail_date_dir: Some("mail/2026/Apr".to_string()),
            mail_date_file: Some("2026-04-13".to_string()),
        };

        let frontmatter = generate_frontmatter(&metadata);
        assert!(frontmatter.contains("type: mail"));
        assert!(frontmatter.contains("from: \"John Doe <john@example.com>\""));
        assert!(frontmatter.contains("to:"));
        assert!(frontmatter.contains("  - \"jane@example.com\""));
        assert!(frontmatter.contains("  - \"bob@example.com\""));
        assert!(frontmatter.contains("mail_date: \"2026-04-13T08:30:00+02:00\""));
        assert!(frontmatter.contains("title: \"Meeting tomorrow\""));
    }
}
