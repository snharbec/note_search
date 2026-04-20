use reqwest::blocking::Client;
use serde::Deserialize;
use std::env;
use std::error::Error;
use std::fs;
use std::path::Path;

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

pub fn import_jira_issues(jql: &str, output_dir: &Path) -> Result<usize, Box<dyn Error>> {
    let server = env::var("JIRA_SERVER").map_err(|_| "JIRA_SERVER environment variable not set")?;
    let key = env::var("JIRA_KEY").map_err(|_| "JIRA_KEY environment variable not set")?;

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

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
            .header("Authorization", format!("Bearer {}", key))
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
    md.push_str(&format!("type: jira\n"));
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
