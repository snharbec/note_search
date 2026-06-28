use chrono::Local;
use std::fs;
use std::path::{Path, PathBuf};

pub fn create_note(
    note_type: &str,
    text: &str,
    note_dir: &Path,
    timestamp: bool,
    as_todo: bool,
) -> Result<PathBuf, String> {
    if note_type != "daily" {
        return Err(format!("Unsupported note type: {}", note_type));
    }

    let now = Local::now();
    let year = now.format("%Y").to_string();
    let month_short = now.format("%b").to_string(); // e.g., Jun
    let date_str = now.format("%Y-%m-%d").to_string();
    let filename = format!("{}.md", date_str);
    let note_path = note_dir
        .join("daily")
        .join(&year)
        .join(&month_short)
        .join(&filename);

    if !note_path.exists() {
        let content = render_template("daily", note_dir, &now)?;
        fs::create_dir_all(note_path.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::write(&note_path, content).map_err(|e| e.to_string())?;
    }

    let existing = fs::read_to_string(&note_path).map_err(|e| e.to_string())?;
    let updated = append_to_yournal(&existing, text, timestamp, as_todo, &now);

    fs::write(&note_path, updated).map_err(|e| e.to_string())?;

    Ok(note_path)
}

fn render_template(
    template_name: &str,
    note_dir: &Path,
    now: &chrono::DateTime<Local>,
) -> Result<String, String> {
    // Look for template in the library directory first
    if let Ok(home_dir) = std::env::var("HOME") {
        let lib_template_path = Path::new(&home_dir)
            .join(".local/share/note_search/templates")
            .join(format!("{}.md", template_name));
        if lib_template_path.exists() {
            let template = fs::read_to_string(&lib_template_path).map_err(|e| e.to_string())?;
            return Ok(replace_placeholders(&template, now));
        }
    }

    // Look for template in the note directory ($NOTE_SEARCH_DIR/templates/)
    let note_dir_template_path = note_dir
        .join("templates")
        .join(format!("{}.md", template_name));
    if note_dir_template_path.exists() {
        let template = fs::read_to_string(&note_dir_template_path).map_err(|e| e.to_string())?;
        return Ok(replace_placeholders(&template, now));
    }

    Err(format!(
        "Template not found: {}",
        format!("{}.md", template_name)
    ))
}

fn replace_placeholders(template: &str, now: &chrono::DateTime<Local>) -> String {
    template
        .replace("{{date}}", &now.format("%Y-%m-%d").to_string())
        .replace("{{time}}", &now.format("%H:%M").to_string())
        .replace("{{date_human}}", &now.format("%A, %B %d, %Y").to_string())
}

fn append_to_yournal(
    content: &str,
    text: &str,
    timestamp: bool,
    as_todo: bool,
    now: &chrono::DateTime<Local>,
) -> String {
    let heading = "## Yournal";

    let prefix = if timestamp {
        format!("[{}] ", now.format("%H:%M"))
    } else {
        String::new()
    };

    let body = if as_todo {
        format!("- [ ] {}{}", prefix, text)
    } else {
        format!("- {}{}", prefix, text)
    };

    // Check if the exact entry already exists
    let existing_lines: Vec<&str> = content.lines().collect();
    if existing_lines.contains(&body.as_str()) {
        return content.to_string();
    }

    if let Some(idx) = content.find(heading) {
        let after_heading_idx = idx + heading.len();
        let (before, rest) = content.split_at(after_heading_idx);
        let rest = rest.trim_start_matches('\n');
        let mut new_content = String::from(before);
        new_content.push('\n');
        new_content.push_str(&body);
        new_content.push('\n');
        if !rest.is_empty() {
            new_content.push_str(rest);
        }
        new_content
    } else {
        let mut new_content = content.trim_end().to_string();
        new_content.push_str("\n\n## Yournal\n");
        new_content.push_str(&body);
        new_content.push('\n');
        new_content
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_render_template_daily() {
        let now = Local.with_ymd_and_hms(2026, 5, 19, 15, 11, 0).unwrap();
        // Template is searched in the note directory
        let result = render_template("nonexistent", Path::new("."), &now);
        assert!(result.is_err());
    }

    #[test]
    fn test_append_to_yournal_existing_heading() {
        let content = "---\ntype: daily\n---\n\n# Title\n\n## Yournal\n";
        let now = Local.with_ymd_and_hms(2026, 5, 19, 15, 11, 0).unwrap();
        let updated = append_to_yournal(content, "My new entry", false, false, &now);
        assert!(updated.contains("## Yournal\n- My new entry"));
    }

    #[test]
    fn test_append_to_yournal_missing_heading() {
        let content = "---\ntype: daily\n---\n\n# Title\n";
        let now = Local.with_ymd_and_hms(2026, 5, 19, 15, 11, 0).unwrap();
        let updated = append_to_yournal(content, "My new entry", false, false, &now);
        assert!(updated.contains("## Yournal\n- My new entry"));
    }

    #[test]
    fn test_append_to_yournal_with_existing_entries() {
        let content = "---\ntype: daily\n---\n\n# Title\n\n## Yournal\n- First entry\n";
        let now = Local.with_ymd_and_hms(2026, 5, 19, 15, 11, 0).unwrap();
        let updated = append_to_yournal(content, "Second entry", false, false, &now);
        assert!(updated.contains("- First entry"));
        assert!(updated.contains("- Second entry"));
    }

    #[test]
    fn test_append_to_yournal_duplicate_entry() {
        let content = "---\ntype: daily\n---\n\n# Title\n\n## Yournal\n- Existing entry\n";
        let now = Local.with_ymd_and_hms(2026, 5, 19, 15, 11, 0).unwrap();
        let updated = append_to_yournal(content, "Existing entry", false, false, &now);
        assert_eq!(updated, content);
        // Make sure the entry wasn't duplicated
        let count = updated.matches("- Existing entry").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_append_to_yournal_with_timestamp() {
        let content = "---\ntype: daily\n---\n\n# Title\n\n## Yournal\n";
        let now = Local.with_ymd_and_hms(2026, 5, 19, 15, 11, 0).unwrap();
        let updated = append_to_yournal(content, "Timestamped entry", true, false, &now);
        assert!(updated.contains("- [15:11] Timestamped entry"));
    }

    #[test]
    fn test_append_to_yournal_as_todo() {
        let content = "---\ntype: daily\n---\n\n# Title\n\n## Yournal\n";
        let now = Local.with_ymd_and_hms(2026, 5, 19, 15, 11, 0).unwrap();
        let updated = append_to_yournal(content, "Todo entry", false, true, &now);
        assert!(updated.contains("- [ ] Todo entry"));
    }

    #[test]
    fn test_append_to_yournal_as_todo_with_timestamp() {
        let content = "---\ntype: daily\n---\n\n# Title\n\n## Yournal\n";
        let now = Local.with_ymd_and_hms(2026, 5, 19, 15, 11, 0).unwrap();
        let updated = append_to_yournal(content, "Todo with time", true, true, &now);
        assert!(updated.contains("- [ ] [15:11] Todo with time"));
    }
}
