use crate::converter::convert_document;
use crate::markdown_parser::extract_frontmatter;
use std::path::Path;
use std::process;

/// Re-run `convert_document` against every already-converted `type: document`
/// note under `input`, using whatever the original source file resolves to
/// on disk, and overwrite the note's body in place. Frontmatter (including
/// `created`, `source`, and any fields the user added by hand) is kept
/// verbatim - only the markdown body is replaced.
pub fn handle_reconvert(input: &str) {
    let input_path = Path::new(input);

    if !input_path.exists() {
        eprintln!("Error: Input directory '{}' does not exist", input);
        process::exit(1);
    }
    if !input_path.is_dir() {
        eprintln!("Error: Input path '{}' is not a directory", input);
        process::exit(1);
    }

    println!("Reconverting documents under '{}'...", input);

    let mut reconverted = 0;
    let mut unchanged = 0;
    let mut skipped = 0;
    let mut failed = 0;

    for entry in walkdir::WalkDir::new(input_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|ext| ext == "md"))
    {
        let note_path = entry.path();

        let content = match std::fs::read_to_string(note_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let Some((frontmatter, old_body, line_count)) = extract_frontmatter(&content) else {
            continue;
        };

        let Ok(yaml_docs) = yaml_rust2::YamlLoader::load_from_str(&frontmatter) else {
            continue;
        };
        let Some(yaml) = yaml_docs.first() else {
            continue;
        };

        if yaml["type"].as_str() != Some("document") {
            continue;
        }
        let Some(source) = yaml["source"].as_str() else {
            continue;
        };

        match resolve_source_path(note_path, source) {
            Some(source_path) => match convert_document(&source_path) {
                Ok((new_body, _metadata)) => {
                    if new_body == old_body {
                        unchanged += 1;
                        continue;
                    }

                    let lines: Vec<&str> = content.lines().collect();
                    let frontmatter_block = lines[..line_count].join("\n");
                    let new_content = format!("{}\n\n{}", frontmatter_block, new_body);

                    match std::fs::write(note_path, new_content) {
                        Ok(()) => {
                            println!("Reconverted: {}", note_path.display());
                            reconverted += 1;
                        }
                        Err(e) => {
                            eprintln!("Failed to write {}: {}", note_path.display(), e);
                            failed += 1;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to reconvert {}: {}", note_path.display(), e);
                    failed += 1;
                }
            },
            None => {
                eprintln!(
                    "Skipping {}: source '{}' not found",
                    note_path.display(),
                    source
                );
                skipped += 1;
            }
        }
    }

    println!(
        "Done. {} reconverted, {} unchanged, {} skipped (source not found), {} failed.",
        reconverted, unchanged, skipped, failed
    );
}

/// Find the original source file for a `type: document` note. Prefers the
/// copy `create_note` keeps alongside the note (same directory, same
/// basename as `source`) over the recorded `source` path itself, since that
/// path is only ever resolved relative to wherever the original `convert`
/// invocation happened to run from and may no longer point anywhere useful.
fn resolve_source_path(note_path: &Path, source: &str) -> Option<std::path::PathBuf> {
    let note_dir = note_path.parent().unwrap_or_else(|| Path::new("."));
    if let Some(name) = Path::new(source).file_name() {
        let sibling = note_dir.join(name);
        if sibling.exists() {
            return Some(sibling);
        }
    }

    let direct = Path::new(source);
    if direct.exists() {
        return Some(direct.to_path_buf());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_resolve_source_path_prefers_sibling() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let note_path = temp_dir.path().join("note.md");
        fs::write(&note_path, "note")?;
        fs::write(temp_dir.path().join("original.docx"), "docx bytes")?;

        // "source" points at a path that no longer exists anywhere; the
        // sibling copy alongside the note should still be found by basename.
        let resolved = resolve_source_path(&note_path, "/nowhere/original.docx");
        assert_eq!(resolved, Some(temp_dir.path().join("original.docx")));

        Ok(())
    }

    #[test]
    fn test_resolve_source_path_falls_back_to_direct() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let note_path = temp_dir.path().join("note.md");
        fs::write(&note_path, "note")?;

        let elsewhere_dir = TempDir::new()?;
        let elsewhere_file = elsewhere_dir.path().join("original.docx");
        fs::write(&elsewhere_file, "docx bytes")?;

        let resolved = resolve_source_path(&note_path, elsewhere_file.to_str().unwrap());
        assert_eq!(resolved, Some(elsewhere_file));

        Ok(())
    }

    #[test]
    fn test_resolve_source_path_none_when_neither_exists() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let note_path = temp_dir.path().join("note.md");
        fs::write(&note_path, "note")?;

        assert_eq!(resolve_source_path(&note_path, "/nowhere/original.docx"), None);

        Ok(())
    }

    #[test]
    fn test_handle_reconvert_updates_body_preserves_frontmatter() -> Result<(), Box<dyn std::error::Error>>
    {
        use docx_rs::{Docx, Paragraph, Run};

        let vault = TempDir::new()?;
        let docs_dir = vault.path().join("documents");
        fs::create_dir_all(&docs_dir)?;

        let docx_path = docs_dir.join("original.docx");
        let docx = Docx::new().add_paragraph(
            Paragraph::new()
                .style("Heading1")
                .add_run(Run::new().add_text("Fresh Title")),
        );
        let file = fs::File::create(&docx_path)?;
        docx.build().pack(file)?;

        let note_path = docs_dir.join("document-2026-01-01-original.md");
        let original_note = "---\ntype: document\nsource: \"original.docx\"\ncustom_field: keep-me\n---\n\nSTALE BODY FROM BEFORE THE HEADING FIX\n";
        fs::write(&note_path, original_note)?;

        handle_reconvert(vault.path().to_str().unwrap());

        let updated = fs::read_to_string(&note_path)?;
        assert!(updated.contains("custom_field: keep-me"));
        assert!(updated.contains("source: \"original.docx\""));
        assert!(updated.contains("# Fresh Title"));
        assert!(!updated.contains("STALE BODY FROM BEFORE THE HEADING FIX"));

        Ok(())
    }

    #[test]
    fn test_handle_reconvert_skips_non_document_notes() -> Result<(), Box<dyn std::error::Error>> {
        let vault = TempDir::new()?;
        let note_path = vault.path().join("note.md");
        let content = "---\ntype: web\nsource: \"https://example.com\"\n---\n\nUnchanged.\n";
        fs::write(&note_path, content)?;

        handle_reconvert(vault.path().to_str().unwrap());

        assert_eq!(fs::read_to_string(&note_path)?, content);

        Ok(())
    }
}
