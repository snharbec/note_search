use crate::commands::args::ElementSearchArgs;
use crate::commands::search::parse_comma_separated;
use crate::database_service::DatabaseService;
use crate::search_criteria::{SearchCriteria, SortOrder};
use std::collections::HashSet;
use std::env;
use std::process;

pub fn handle_elements_search(args: &ElementSearchArgs, database: &str) {
    let mut criteria = build_element_criteria(args, database);

    if criteria.absolute_path {
        let note_dir = env::var("NOTE_SEARCH_DIR").unwrap_or_else(|_| ".".to_string());
        criteria.note_dir = note_dir.clone();
        criteria.base_path = note_dir;
    }

    let database_service = DatabaseService::new(&criteria.database_path);

    match database_service.search_elements(&criteria) {
        Ok(results) => {
            if results.is_empty() {
                println!("No matching elements found.");
            } else if criteria.list_only {
                // When listing, show each file only once
                let mut seen_files: HashSet<String> = HashSet::new();
                for result in results {
                    if seen_files.insert(result.filename.clone()) {
                        println!(
                            "{}",
                            result.formatted_string(
                                &criteria.output_format,
                                criteria.absolute_path,
                                &criteria.base_path
                            )
                        );
                    }
                }
            } else {
                for result in results {
                    println!(
                        "{}",
                        result.formatted_string(
                            &criteria.output_format,
                            criteria.absolute_path,
                            &criteria.base_path
                        )
                    );
                }
            }
        }
        Err(e) => {
            eprintln!("Database error: {}", e);
            process::exit(1);
        }
    }
}

pub fn build_element_criteria(args: &ElementSearchArgs, database: &str) -> SearchCriteria {
    let mut criteria = SearchCriteria {
        database_path: database.to_string(),
        output_format: args.format.clone(),
        list_only: args.list,
        absolute_path: args.absolute_path,
        ..Default::default()
    };

    if let Some(tags_str) = &args.tags {
        criteria.tags = parse_comma_separated(tags_str);
    }

    if let Some(links_str) = &args.links {
        criteria.links = parse_comma_separated(links_str);
    }

    criteria.text = args.text.clone();

    if let Some(sort_str) = &args.sort {
        criteria.sort_order = parse_element_sort_order(sort_str);
    }

    criteria
}

pub fn parse_element_sort_order(input: &str) -> Option<SortOrder> {
    let input = input.trim().to_lowercase();

    match input.as_str() {
        "filename" => Some(SortOrder::Filename),
        "modified" => Some(SortOrder::Modified),
        "text" => Some(SortOrder::Text),
        _ => {
            eprintln!(
                "Warning: Unknown sort order '{}'. Use 'filename', 'modified', or 'text'.",
                input
            );
            None
        }
    }
}
