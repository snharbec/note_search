use crate::converter::{
    convert_document, convert_email, convert_msg, convert_reddit_discussion, convert_web_page,
    create_note, is_reddit_url, is_url,
};
use std::path::Path;
use std::process;

pub fn handle_convert(source: &str, output_dir: &str) {
    let output_path = Path::new(output_dir);

    // Ensure output directory exists
    if !output_path.exists() {
        eprintln!("Error: Output directory '{}' does not exist", output_dir);
        process::exit(1);
    }

    let is_eml = Path::new(source)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("eml"));
    let is_msg = Path::new(source)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("msg"));

    let result = if is_reddit_url(source) {
        println!("Converting Reddit discussion: {}", source);
        convert_reddit_discussion(source)
    } else if is_url(source) {
        println!("Converting web page: {}", source);
        convert_web_page(source)
    } else if is_eml {
        let source_path = Path::new(source);
        if !source_path.exists() {
            eprintln!("Error: Source file '{}' does not exist", source);
            process::exit(1);
        }
        println!("Converting email: {}", source);
        convert_email(source_path)
    } else if is_msg {
        let source_path = Path::new(source);
        if !source_path.exists() {
            eprintln!("Error: Source file '{}' does not exist", source);
            process::exit(1);
        }
        println!("Converting Outlook message: {}", source);
        convert_msg(source_path)
    } else {
        let source_path = Path::new(source);
        if !source_path.exists() {
            eprintln!("Error: Source file '{}' does not exist", source);
            process::exit(1);
        }
        println!("Converting document: {}", source);
        convert_document(source_path)
    };

    match result {
        Ok((content, metadata)) => match create_note(&content, &metadata, output_path) {
            Ok(file_path) => {
                println!(
                    "Successfully created note: {} (type: {})",
                    file_path.display(),
                    metadata.note_type
                );
                if let Some(title) = metadata.title {
                    println!("Title: {}", title);
                }
            }
            Err(e) => {
                eprintln!("Error creating note: {}", e);
                process::exit(1);
            }
        },
        Err(e) => {
            eprintln!("Error converting source: {}", e);
            process::exit(1);
        }
    }
}
