use std::env;
use std::fs;
use std::path::Path;
use std::process;

pub fn handle_linker(database: &str, subdir: &str) {
    use rusqlite::Connection;
    use walkdir::WalkDir;

    let note_dir = env::var("NOTE_SEARCH_DIR").unwrap_or_else(|_| ".".to_string());
    let target_dir = Path::new(&note_dir).join(subdir);

    if !target_dir.exists() {
        eprintln!("Error: Directory '{}' does not exist", target_dir.display());
        process::exit(1);
    }

    // Query database for project and person names
    let db_path = Path::new(database);
    if !db_path.exists() {
        eprintln!(
            "Error: Database '{}' not found. Run import first.",
            database
        );
        process::exit(1);
    }

    let conn = match Connection::open(db_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error opening database: {}", e);
            process::exit(1);
        }
    };

    // Get note names for projects and persons
    let entity_names = match get_entity_names(&conn) {
        Ok(names) => names,
        Err(e) => {
            eprintln!("Error querying database: {}", e);
            process::exit(1);
        }
    };

    if entity_names.is_empty() {
        println!("No projects or persons found in database.");
        return;
    }

    eprintln!(
        "Found {} entities (projects/persons) to link",
        entity_names.len()
    );

    // Sort names by length (longest first) to avoid partial matches
    let mut sorted_names = entity_names;
    sorted_names.sort_by(|a, b| b.len().cmp(&a.len()));

    // Process all .md files in the target directory
    let mut total_replacements = 0;
    let mut files_modified = 0;

    for entry in WalkDir::new(&target_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "md") {
            match process_file_for_links(path, &sorted_names) {
                Ok(count) => {
                    if count > 0 {
                        files_modified += 1;
                        total_replacements += count;
                    }
                }
                Err(e) => {
                    eprintln!("Error processing {}: {}", path.display(), e);
                }
            }
        }
    }

    println!(
        "Linked {} replacements across {} files in {}",
        total_replacements,
        files_modified,
        target_dir.display()
    );
}

pub fn get_entity_names(
    conn: &rusqlite::Connection,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut names = Vec::new();

    let query = "SELECT DISTINCT filename FROM markdown_data \
                 WHERE json_extract(header_fields, '$.type') IN ('project', 'person')";

    let mut stmt = conn.prepare(query)?;
    let rows = stmt.query_map([], |row| Ok(row.get::<_, String>(0)?))?;

    for row in rows {
        let filename = row?;
        let name = Path::new(&filename)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| filename.clone());
        names.push(name);
    }

    Ok(names)
}

pub fn process_file_for_links(
    path: &Path,
    entity_names: &[String],
) -> Result<usize, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().collect();
    let mut new_lines = Vec::new();
    let mut total_replacements = 0;

    for line in &lines {
        let (new_line, count) = replace_entity_names_in_line(line, entity_names);
        new_lines.push(new_line);
        total_replacements += count;
    }

    if total_replacements > 0 {
        let new_content = new_lines.join("\n");
        let final_content = if content.ends_with('\n') && !new_content.ends_with('\n') {
            format!("{}\n", new_content)
        } else {
            new_content
        };
        fs::write(path, final_content)?;
    }

    Ok(total_replacements)
}

pub fn replace_entity_names_in_line(line: &str, entity_names: &[String]) -> (String, usize) {
    let mut result = line.to_string();
    let mut total_count = 0;

    for note_name in entity_names {
        let (new_result, count) = link_replacements(&result, note_name);
        result = new_result;
        total_count += count;
    }

    (result, total_count)
}

pub fn link_replacements(text: &str, note_name: &str) -> (String, usize) {
    let pattern = build_entity_pattern(note_name);
    let re = match regex::Regex::new(&pattern) {
        Ok(re) => re,
        Err(_) => return (text.to_string(), 0),
    };

    let mut result = String::new();
    let mut last_end = 0;
    let mut count = 0;

    for cap in re.find_iter(text) {
        let start = cap.start();
        let end = cap.end();

        let boundary_before = start == 0
            || (!text.as_bytes()[start - 1].is_ascii_alphanumeric()
                && text.as_bytes()[start - 1] != b'_'
                && text.as_bytes()[start - 1] != b'-');

        let boundary_after = end == text.len()
            || (!text.as_bytes()[end].is_ascii_alphanumeric()
                && text.as_bytes()[end] != b'_'
                && text.as_bytes()[end] != b'-');

        if !boundary_before || !boundary_after {
            continue;
        }

        if is_inside_wiki_link(text, start) {
            continue;
        }

        result.push_str(&text[last_end..start]);
        result.push_str(&format!("[[{}]]", note_name));
        last_end = end;
        count += 1;
    }

    if count > 0 {
        result.push_str(&text[last_end..]);
        (result, count)
    } else {
        (text.to_string(), 0)
    }
}

pub fn build_entity_pattern(name: &str) -> String {
    let mut pattern = String::from("(?:");

    for ch in name.chars() {
        if ch == ' ' || ch == '_' || ch == '-' {
            pattern.push_str("[_\\s-]");
        } else if ch.is_ascii_alphabetic() {
            let lower = ch.to_ascii_lowercase();
            let upper = ch.to_ascii_uppercase();
            pattern.push_str(&format!("[{}{}]", lower, upper));
        } else {
            pattern.push_str(&regex::escape(&ch.to_string()));
        }
    }

    pattern.push(')');
    pattern
}

pub fn is_inside_wiki_link(text: &str, pos: usize) -> bool {
    let before = &text[..pos];
    if let Some(open_pos) = before.rfind("[[") {
        let between = &text[open_pos + 2..pos];
        if !between.contains("]]") {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod linker_tests {
    use super::*;

    #[test]
    fn test_build_entity_pattern_simple() {
        let pattern = build_entity_pattern("Alpha");
        let re = regex::Regex::new(&pattern).unwrap();
        assert!(re.is_match("Alpha"));
        assert!(re.is_match("alpha"));
        assert!(re.is_match("ALPHA"));
    }

    #[test]
    fn test_build_entity_pattern_with_spaces() {
        let pattern = build_entity_pattern("Project Alpha");
        let re = regex::Regex::new(&pattern).unwrap();
        assert!(re.is_match("Project Alpha"));
        assert!(re.is_match("Project_Alpha"));
        assert!(re.is_match("Project-Alpha"));
        assert!(re.is_match("project alpha"));
    }

    #[test]
    fn test_link_replacements_basic() {
        let (result, count) = link_replacements("Working on Project Alpha today", "Project Alpha");
        assert_eq!(count, 1);
        assert_eq!(result, "Working on [[Project Alpha]] today");
    }

    #[test]
    fn test_link_replacements_case_insensitive() {
        let (result, count) = link_replacements("Working on project alpha today", "Project Alpha");
        assert_eq!(count, 1);
        assert_eq!(result, "Working on [[Project Alpha]] today");
    }

    #[test]
    fn test_link_replacements_underscore_match() {
        let (result, count) = link_replacements("Working on Project_Alpha today", "Project Alpha");
        assert_eq!(count, 1);
        assert_eq!(result, "Working on [[Project Alpha]] today");
    }

    #[test]
    fn test_link_replacements_skip_already_linked() {
        let (result, count) =
            link_replacements("See [[Project Alpha]] for details", "Project Alpha");
        assert_eq!(count, 0);
        assert_eq!(result, "See [[Project Alpha]] for details");
    }

    #[test]
    fn test_link_replacements_word_boundary() {
        let (result, count) = link_replacements("The Alpha release is here", "Alpha");
        assert_eq!(count, 1);
        assert_eq!(result, "The [[Alpha]] release is here");
    }

    #[test]
    fn test_link_replacements_no_partial_match() {
        let (result, count) = link_replacements("AlphaCentauri is a star", "Alpha");
        assert_eq!(count, 0);
        assert_eq!(result, "AlphaCentauri is a star");
    }

    #[test]
    fn test_link_replacements_no_prefix_match() {
        let (result, count) = link_replacements("The SubAlpha project", "Alpha");
        assert_eq!(count, 0);
        assert_eq!(result, "The SubAlpha project");
    }

    #[test]
    fn test_link_replacements_multiple_occurrences() {
        let (result, count) = link_replacements("Alpha and more Alpha here", "Alpha");
        assert_eq!(count, 2);
        assert_eq!(result, "[[Alpha]] and more [[Alpha]] here");
    }

    #[test]
    fn test_replace_entity_names_in_line_multiple_entities() {
        let names = vec!["Project Alpha".to_string(), "Jane Smith".to_string()];
        let (result, count) =
            replace_entity_names_in_line("Project Alpha assigned to Jane Smith", &names);
        assert_eq!(count, 2);
        assert_eq!(result, "[[Project Alpha]] assigned to [[Jane Smith]]");
    }

    #[test]
    fn test_is_inside_wiki_link() {
        assert!(is_inside_wiki_link("See [[Project Alpha", 10));
        assert!(!is_inside_wiki_link("See [[Project]] Alpha", 17));
        assert!(!is_inside_wiki_link("Project Alpha is cool", 0));
    }
}
