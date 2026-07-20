use base64::{engine::general_purpose, Engine as _};
use reqwest::blocking::{Client, ClientBuilder};
use reqwest::{Certificate, Identity};
use serde::Deserialize;
use std::env;
use std::error::Error;
use std::fs;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Deserialize)]
struct SearchResponse {
    issues: Vec<Issue>,
}

#[derive(Debug, Deserialize)]
struct Issue {
    key: String,
    fields: IssueFields,
}

#[derive(Debug, Deserialize)]
struct IssueFields {
    summary: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    status: Option<StatusValue>,
    #[serde(default)]
    issuetype: Option<IssueTypeValue>,
    #[serde(default)]
    priority: Option<PriorityValue>,
    #[serde(default)]
    assignee: Option<UserValue>,
    #[serde(default)]
    reporter: Option<UserValue>,
    #[serde(default)]
    created: Option<String>,
    #[serde(default)]
    updated: Option<String>,
    #[serde(default)]
    resolutiondate: Option<String>,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    comment: Option<CommentWrapper>,
}

#[derive(Debug, Deserialize)]
struct StatusValue {
    name: String,
}

#[derive(Debug, Deserialize)]
struct IssueTypeValue {
    name: String,
}

#[derive(Debug, Deserialize)]
struct PriorityValue {
    name: String,
}

#[derive(Debug, Deserialize)]
struct UserValue {
    #[serde(rename = "displayName")]
    display_name: String,
}

#[derive(Debug, Deserialize)]
struct CommentWrapper {
    comments: Vec<Comment>,
}

#[derive(Debug, Deserialize)]
struct Comment {
    author: Option<UserValue>,
    created: String,
    body: String,
}

/// Resolve the JIRA API token.
///
/// Uses `JIRA_API_TOKEN` when set, falling back to the legacy `JIRA_KEY`
/// variable for backwards compatibility.
fn api_token() -> Result<String, Box<dyn Error>> {
    if let Ok(tok) = env::var("JIRA_API_TOKEN") {
        if !tok.trim().is_empty() {
            return Ok(tok);
        }
    }
    env::var("JIRA_KEY")
        .map_err(|_| "JIRA_API_TOKEN (or JIRA_KEY) environment variable not set".into())
}

/// Build a blocking `reqwest` client configured for optional mutual TLS.
///
/// - `JIRA_CA_CERTIFICATE`: path to a PEM bundle used to verify the JIRA
///   server's host certificate. Added as an additional root certificate.
/// - `JIRA_HOST_CERTIFICATE`: path to a PKCS#12 archive (`.p12`/`.pfx`) used
///   as the client identity for mutual TLS. Decrypted with
///   `JIRA_HOST_CERTIFICATE_PASSWORD`.
///
/// When neither variable is set the client behaves exactly like before.
fn build_jira_client() -> Result<Client, Box<dyn Error>> {
    let mut builder = ClientBuilder::new().timeout(Duration::from_secs(30));

    if let Ok(ca_path) = env::var("JIRA_CA_CERTIFICATE") {
        if !ca_path.trim().is_empty() {
            let cert_data = fs::read(&ca_path)
                .map_err(|e| format!("Failed to read JIRA_CA_CERTIFICATE '{}': {}", ca_path, e))?;

            // Try to parse as PEM first
            let cert = if cert_data.starts_with(b"-----BEGIN CERTIFICATE-----") {
                Certificate::from_pem(&cert_data)
                    .map_err(|e| format!("Failed to parse JIRA_CA_CERTIFICATE as PEM '{}': {}", ca_path, e))?
            } else {
                // If it doesn't look like PEM, try to wrap it as PEM (assuming it might be DER)
                let b64 = general_purpose::STANDARD.encode(&cert_data);
                let pem = format!("-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----", b64);
                Certificate::from_pem(pem.as_bytes())
                    .map_err(|e| format!("Failed to parse JIRA_CA_CERTIFICATE '{}' (tried as PEM and DER): {}", ca_path, e))?
            };
            builder = builder.add_root_certificate(cert);
        }
    }

    if let Ok(cert_path) = env::var("JIRA_HOST_CERTIFICATE") {
        if !cert_path.trim().is_empty() {
            let der = fs::read(&cert_path).map_err(|e| {
                format!(
                    "Failed to read JIRA_HOST_CERTIFICATE '{}': {}",
                    cert_path, e
                )
            })?;
            let password = env::var("JIRA_HOST_CERTIFICATE_PASSWORD")
                .map_err(|_| "JIRA_HOST_CERTIFICATE_PASSWORD environment variable not set")?;
            let identity = Identity::from_pkcs12_der(&der, &password).map_err(|e| {
                format!(
                    "Failed to parse JIRA_HOST_CERTIFICATE '{}': {}",
                    cert_path, e
                )
            })?;
            builder = builder.identity(identity);
        }
    }

    Ok(builder.build()?)
}

/// Fetch a single JIRA issue by key and return its markdown representation
pub fn fetch_single_issue(issue_key: &str) -> Result<String, Box<dyn Error>> {
    // Validate issue key format before using it
    if !issue_key
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(format!("Invalid JIRA issue key format: {}", issue_key).into());
    }

    let server = env::var("JIRA_SERVER").map_err(|_| "JIRA_SERVER environment variable not set")?;
    let token = api_token()?;

    let client = build_jira_client()?;

    let url = format!(
        "{}/rest/api/2/issue/{}?fields=key,summary,status,issuetype,priority,assignee,reporter,created,updated,resolutiondate,labels,description,comment",
        server.trim_end_matches('/'),
        issue_key
    );

    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/json")
        .send()?;

    if response.status().is_success() {
        let issue: Issue = response.json()?;
        Ok(format_issue_markdown(&issue))
    } else if response.status().as_u16() == 404 {
        Err(format!("Issue '{}' not found", issue_key).into())
    } else {
        Err(format!("JIRA API error: {}", response.status()).into())
    }
}

/// Save pre-fetched markdown to a file for a JIRA issue
pub fn save_issue_markdown(
    issue_key: &str,
    markdown: &str,
    output_dir: &Path,
) -> Result<String, Box<dyn Error>> {
    let jira_dir = output_dir.join("jira");
    fs::create_dir_all(&jira_dir)?;

    let filename = format!("jira/{}.md", issue_key);
    let filepath = output_dir.join(&filename);
    fs::write(&filepath, markdown)?;

    Ok(filepath.to_string_lossy().to_string())
}

/// Import a single JIRA issue by key to a markdown file
pub fn import_single_issue(issue_key: &str, output_dir: &Path) -> Result<String, Box<dyn Error>> {
    let markdown = fetch_single_issue(issue_key)?;
    save_issue_markdown(issue_key, &markdown, output_dir)
}

pub fn import_jira_issues(jql: &str, output_dir: &Path) -> Result<usize, Box<dyn Error>> {
    let server = env::var("JIRA_SERVER").map_err(|_| "JIRA_SERVER environment variable not set")?;
    let token = api_token()?;

    let client = build_jira_client()?;

    let mut start_at = 0i64;
    let max_results = 50i64;
    let mut total_imported = 0;

    let jira_dir = output_dir.join("jira");
    fs::create_dir_all(&jira_dir)?;

    loop {
        let url = format!(
            "{}/rest/api/2/search?jql={}&startAt={}&maxResults={}&fields=key,summary,status,issuetype,priority,assignee,reporter,created,updated,resolutiondate,labels,description,comment",
            server.trim_end_matches('/'),
            urlencoding::encode(jql),
            start_at,
            max_results
        );

        let response = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/json")
            .send()?;

        if !response.status().is_success() {
            return Err(format!("JIRA API error: {}", response.status()).into());
        }

        let data: SearchResponse = response.json()?;

        if data.issues.is_empty() {
            break;
        }

        for issue in &data.issues {
            let markdown = format_issue_markdown(issue);
            let filename = format!("jira/{}.md", issue.key);
            let filepath = output_dir.join(&filename);
            fs::write(&filepath, markdown)?;
            total_imported += 1;
        }

        start_at += max_results;
    }

    Ok(total_imported)
}

/// Format a comment's author/timestamp header, linking the author and the
/// date. `created` is JIRA's ISO 8601 timestamp, e.g.
/// `2024-06-14T09:29:05.000+0000`. Turns
/// "Stefan Harbeck (2024-06-14T09:29:05.000+0000)" into
/// "### [[stefan harbeck]] writes on [[2024-06-14]] at 09:29:05+0000".
fn format_comment_header(author: &str, created: &str) -> String {
    let author_link = format!("[[{}]]", author.to_lowercase());

    let Some((date_part, rest)) = created.split_once('T') else {
        // Unexpected format - fall back to the raw timestamp rather than
        // guessing at a date link.
        return format!("### {} ({})\n\n", author_link, created);
    };

    let (time_with_ms, tz) = match rest.find(['+', '-']) {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None if rest.ends_with('Z') => (&rest[..rest.len() - 1], "Z"),
        None => (rest, ""),
    };
    let time = time_with_ms.split('.').next().unwrap_or(time_with_ms);

    format!(
        "### {} writes on [[{}]] at {}{}\n\n",
        author_link, date_part, time, tz
    )
}

fn format_issue_markdown(issue: &Issue) -> String {
    let mut md = String::new();

    md.push_str("---\n");
    md.push_str(&format!("key: \"{}\"\n", issue.key));
    md.push_str("type: jira\n");

    if let Some(ref status) = issue.fields.status {
        md.push_str(&format!("status: \"{}\"\n", status.name));
    }
    if let Some(ref issuetype) = issue.fields.issuetype {
        md.push_str(&format!("issuetype: \"{}\"\n", issuetype.name));
    }
    if let Some(ref priority) = issue.fields.priority {
        md.push_str(&format!("priority: \"{}\"\n", priority.name));
    }
    if let Some(ref assignee) = issue.fields.assignee {
        md.push_str(&format!("assignee: \"{}\"\n", assignee.display_name));
    }
    if let Some(ref reporter) = issue.fields.reporter {
        md.push_str(&format!("reporter: \"{}\"\n", reporter.display_name));
    }
    if let Some(ref created) = issue.fields.created {
        md.push_str(&format!("jira_created: \"{}\"\n", created));
    }
    if let Some(ref updated) = issue.fields.updated {
        md.push_str(&format!("jira_updated: \"{}\"\n", updated));
    }
    if let Some(ref resolutiondate) = issue.fields.resolutiondate {
        md.push_str(&format!("jira_resolutiondate: \"{}\"\n", resolutiondate));
    }
    if !issue.fields.labels.is_empty() {
        md.push_str("labels:\n");
        for label in &issue.fields.labels {
            md.push_str(&format!("  - \"{}\"\n", label));
        }
    }
    md.push_str("---\n\n");

    md.push_str(&format!("# {} - {}\n\n", issue.key, issue.fields.summary));

    if let Some(ref description) = issue.fields.description {
        if !description.is_empty() {
            md.push_str("## Description\n\n");
            md.push_str(description);
            md.push_str("\n\n");
        }
    }

    if let Some(ref comment_wrapper) = issue.fields.comment {
        if !comment_wrapper.comments.is_empty() {
            md.push_str("## Comments\n\n");
            for comment in &comment_wrapper.comments {
                let author = comment
                    .author
                    .as_ref()
                    .map(|a| a.display_name.as_str())
                    .unwrap_or("Unknown");
                md.push_str(&format_comment_header(author, &comment.created));
                md.push_str(&comment.body);
                md.push_str("\n\n");
            }
        }
    }

    md
}

mod urlencoding {
    pub fn encode(s: &str) -> String {
        s.chars()
            .map(|c| match c {
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
                _ => format!("%{:02X}", c as u8),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_issue_markdown_starts_with_frontmatter_then_title() {
        let issue: Issue = serde_json::from_str(
            r#"{
                "key": "PROJ-123",
                "fields": {
                    "summary": "Fix the thing",
                    "status": {"name": "Open"}
                }
            }"#,
        )
        .unwrap();

        let md = format_issue_markdown(&issue);

        assert!(md.starts_with("---\n"), "must start with frontmatter delimiter");
        let front_end = md.match_indices("---\n").nth(1).unwrap().0;
        let frontmatter = &md[..front_end];
        assert!(frontmatter.contains("key: \"PROJ-123\""));
        assert!(frontmatter.contains("status: \"Open\""));

        let after_frontmatter = &md[front_end + "---\n".len()..];
        let title_pos = after_frontmatter.find("# PROJ-123 - Fix the thing").unwrap();
        // Only blank lines between the closing frontmatter delimiter and the title.
        assert!(after_frontmatter[..title_pos].trim().is_empty());
    }

    #[test]
    fn test_format_comment_header() {
        let header = format_comment_header("Stefan Harbeck", "2024-06-14T09:29:05.000+0000");
        assert_eq!(
            header,
            "### [[stefan harbeck]] writes on [[2024-06-14]] at 09:29:05+0000\n\n"
        );
    }

    #[test]
    fn test_format_comment_header_zulu_time() {
        let header = format_comment_header("Jane Doe", "2024-06-14T09:29:05.000Z");
        assert_eq!(
            header,
            "### [[jane doe]] writes on [[2024-06-14]] at 09:29:05Z\n\n"
        );
    }

    #[test]
    fn test_format_comment_header_unparseable_falls_back() {
        let header = format_comment_header("Jane Doe", "not-a-date");
        assert_eq!(header, "### [[jane doe]] (not-a-date)\n\n");
    }

    #[test]
    fn test_format_issue_markdown_links_comment_author() {
        let issue: Issue = serde_json::from_str(
            r#"{
                "key": "PROJ-1",
                "fields": {
                    "summary": "Something",
                    "comment": {
                        "comments": [{
                            "author": {"displayName": "Stefan Harbeck"},
                            "created": "2024-06-14T09:29:05.000+0000",
                            "body": "Looks good."
                        }]
                    }
                }
            }"#,
        )
        .unwrap();

        let md = format_issue_markdown(&issue);
        assert!(md.contains(
            "### [[stefan harbeck]] writes on [[2024-06-14]] at 09:29:05+0000\n\nLooks good."
        ));
    }
}
