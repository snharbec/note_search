use std::path::Path;
use std::process;

pub fn handle_jira_import(jql: &str, output_dir: &str) {
    let output_path = Path::new(output_dir);

    if !output_path.exists() {
        eprintln!("Error: Output directory '{}' does not exist", output_dir);
        process::exit(1);
    }

    println!("Importing JIRA issues with JQL: {}", jql);

    match crate::jira::import_jira_issues(jql, output_path) {
        Ok(count) => {
            println!(
                "Successfully imported {} JIRA issues to {}/jira/",
                count, output_dir
            );
        }
        Err(e) => {
            eprintln!("Error importing JIRA issues: {}", e);
            process::exit(1);
        }
    }
}

pub fn handle_jira_single_issue(issue_key: &str, output_dir: Option<&str>, print: bool) {
    // First, fetch the issue markdown
    let markdown = match crate::jira::fetch_single_issue(issue_key) {
        Ok(md) => md,
        Err(e) => {
            eprintln!("Error fetching JIRA issue '{}': {}", issue_key, e);
            process::exit(1);
        }
    };

    // If --print flag is set or no output directory is provided, print to stdout
    if print || output_dir.is_none() {
        println!("{}", markdown);
    }

    // If an output directory is provided, save to file
    if let Some(dir) = output_dir {
        let output_path = Path::new(dir);

        if !output_path.exists() {
            eprintln!("Error: Output directory '{}' does not exist", dir);
            process::exit(1);
        }

        match crate::jira::save_issue_markdown(issue_key, &markdown, output_path) {
            Ok(filepath) => {
                if print {
                    // If we also printed to stdout, just mention the file was saved
                    eprintln!("Saved to: {}", filepath);
                } else {
                    // Otherwise, just print the filepath
                    println!("{}", filepath);
                }
            }
            Err(e) => {
                eprintln!("Error saving JIRA issue '{}': {}", issue_key, e);
                process::exit(1);
            }
        }
    }
}
