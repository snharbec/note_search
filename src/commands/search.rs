use crate::attribute_pair::AttributePair;
use crate::commands::args::{CommonSearchArgs, TodoSearchArgs};
use crate::database_service::DatabaseService;
use crate::search_criteria::{
    DateComparison, DateRange, DueDateCriteria, SearchCriteria, SortOrder, normalize_date,
};
use std::collections::HashSet;
use std::env;
use std::process;

pub fn handle_todos_search(args: &TodoSearchArgs, database: &str) {
    let mut criteria = build_todo_criteria(args, database);

    // Set base path for absolute path resolution using NOTE_SEARCH_DIR
    if criteria.absolute_path {
        let note_dir = env::var("NOTE_SEARCH_DIR").unwrap_or_else(|_| ".".to_string());
        criteria.note_dir = note_dir.clone();
        criteria.base_path = note_dir;
    }

    let database_service = DatabaseService::new(&criteria.database_path);

    match database_service.search_todos(&criteria) {
        Ok(results) => {
            if results.is_empty() {
                println!("No matching todos found.");
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

pub fn handle_notes_search(args: &CommonSearchArgs, database: &str) {
    let mut criteria = build_note_criteria(args, database);

    // Set base path for absolute path resolution using NOTE_SEARCH_DIR
    if criteria.absolute_path {
        let note_dir = env::var("NOTE_SEARCH_DIR").unwrap_or_else(|_| ".".to_string());
        criteria.note_dir = note_dir.clone();
        criteria.base_path = note_dir;
    }

    let database_service = DatabaseService::new(&criteria.database_path);

    match database_service.search_notes(&criteria) {
        Ok(results) => {
            if results.is_empty() {
                println!("No matching notes found.");
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

pub fn build_todo_criteria(args: &TodoSearchArgs, database: &str) -> SearchCriteria {
    let mut criteria = SearchCriteria {
        database_path: database.to_string(),
        output_format: args.common.format.clone(),
        list_only: args.common.list,
        absolute_path: args.common.absolute_path,
        ..Default::default()
    };

    if let Some(tags_str) = &args.common.tags {
        criteria.tags = parse_comma_separated(tags_str);
    }

    if let Some(links_str) = &args.common.links {
        criteria.links = parse_comma_separated(links_str);
    }

    if let Some(attrs_str) = &args.common.attributes {
        criteria.attributes = parse_key_value_pairs(attrs_str);
    }

    criteria.text = args.common.text.clone();
    criteria.priority = args.priority.clone();

    // Handle due date options
    if let Some(date) = &args.due_date_eq {
        criteria.due_date = Some(DueDateCriteria {
            date: normalize_date(date),
            comparison: DateComparison::Equal,
        });
    } else if let Some(date) = &args.due_date_gt {
        criteria.due_date = Some(DueDateCriteria {
            date: normalize_date(date),
            comparison: DateComparison::GreaterThan,
        });
    } else if let Some(date) = &args.due_date {
        criteria.due_date = Some(DueDateCriteria {
            date: normalize_date(date),
            comparison: DateComparison::LessThan,
        });
    }

    // Handle date range options
    if let Some(date_range_str) = &args.common.date_range {
        if let Some(date_range) = DateRange::parse(date_range_str) {
            criteria.date_range = Some(date_range);
        } else {
            eprintln!(
                "Warning: Invalid date range '{}'. Expected: today, yesterday, this_week, last_week, this_month, last_month, this_year, last_year",
                date_range_str
            );
        }
    }

    // Handle custom start/end dates
    criteria.created_start = args.common.start_date.clone();
    criteria.created_end = args.common.end_date.clone();

    // Handle body search
    criteria.search_body = args.common.search_body.clone();

    if args.open {
        criteria.open = Some(true);
    } else if args.closed {
        criteria.open = Some(false);
    }

    // Handle sort order - todos can sort by due_date and priority
    if let Some(sort_str) = &args.common.sort {
        criteria.sort_order = parse_todo_sort_order(sort_str);
    }

    criteria
}

pub fn build_note_criteria(args: &CommonSearchArgs, database: &str) -> SearchCriteria {
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

    if let Some(attrs_str) = &args.attributes {
        criteria.attributes = parse_key_value_pairs(attrs_str);
    }

    criteria.text = args.text.clone();

    // Handle date range options
    if let Some(date_range_str) = &args.date_range {
        if let Some(date_range) = DateRange::parse(date_range_str) {
            criteria.date_range = Some(date_range);
        } else {
            eprintln!(
                "Warning: Invalid date range '{}'. Expected: today, yesterday, this_week, last_week, this_month, last_month, this_year, last_year",
                date_range_str
            );
        }
    }

    // Handle custom start/end dates
    criteria.created_start = args.start_date.clone();
    criteria.created_end = args.end_date.clone();

    // Handle body search
    criteria.search_body = args.search_body.clone();

    // Handle sort order - notes cannot sort by due_date or priority
    if let Some(sort_str) = &args.sort {
        criteria.sort_order = parse_note_sort_order(sort_str);
    }

    criteria
}

pub fn parse_todo_sort_order(input: &str) -> Option<SortOrder> {
    let input = input.trim().to_lowercase();

    if input.starts_with("attr:") {
        let attr_name = input[5..].trim().to_string();
        if !attr_name.is_empty() {
            return Some(SortOrder::Attr(attr_name));
        }
    }

    match input.as_str() {
        "due_date" => Some(SortOrder::DueDate),
        "priority" => Some(SortOrder::Priority),
        "filename" => Some(SortOrder::Filename),
        "modified" => Some(SortOrder::Modified),
        "text" => Some(SortOrder::Text),
        _ => {
            eprintln!("Warning: Unknown sort order '{}'. Using default.", input);
            None
        }
    }
}

pub fn parse_note_sort_order(input: &str) -> Option<SortOrder> {
    let input = input.trim().to_lowercase();

    if input.starts_with("attr:") {
        let attr_name = input[5..].trim().to_string();
        if !attr_name.is_empty() {
            return Some(SortOrder::Attr(attr_name));
        }
    }

    match input.as_str() {
        "filename" => Some(SortOrder::Filename),
        "modified" => Some(SortOrder::Modified),
        "created" => Some(SortOrder::Created),
        "text" => Some(SortOrder::Text),
        "due_date" | "priority" => {
            eprintln!("Warning: Cannot sort notes by '{}'. Notes don't have due dates or priorities. Use 'filename', 'modified', 'created', or 'attr:ATTRIBUTE' instead.", input);
            None
        }
        _ => {
            eprintln!("Warning: Unknown sort order '{}'. Using default.", input);
            None
        }
    }
}

pub fn parse_comma_separated(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn parse_key_value_pairs(input: &str) -> Vec<AttributePair> {
    input
        .split(',')
        .filter_map(|pair| {
            let parts: Vec<&str> = pair.splitn(2, '=').collect();
            if parts.len() == 2 {
                let key = parts[0].trim().to_string();
                let value = parts[1].trim().to_string();
                if !key.is_empty() && !value.is_empty() {
                    return Some(AttributePair::new(&key, &value));
                }
            }
            None
        })
        .collect()
}
