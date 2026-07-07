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
            let pem = fs::read(&ca_path)
                .map_err(|e| format!("Failed to read JIRA_CA_CERTIFICATE '{}': {}", ca_path, e))?;
            let cert = Certificate::from_pem(&pem)
                .map_err(|e| format!("Failed to parse JIRA_CA_CERTIFICATE '{}': {}", ca_path, e))?;
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

fn format_issue_markdown(issue: &Issue) -> String {
    let mut md = String::new();

    md.push_str(&format!("# {} - {}\n\n", issue.key, issue.fields.summary));

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
                md.push_str(&format!("### {} ({})\n\n", author, comment.created));
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
