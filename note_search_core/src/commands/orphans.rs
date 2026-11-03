use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process;

/// List notes that no other note links to (by filename or frontmatter
/// alias) - they can only be found by searching for words they contain.
pub fn handle_orphans(database: &str, untagged: bool, absolute_path: bool) {
    let db_path = Path::new(database);
    crate::commands::require_db_exists(db_path, database);

    let result = crate::commands::open_db_with_schema(db_path)
        .and_then(|conn| find_orphans(&conn, untagged).map_err(Into::into));
    match result {
        Ok(orphans) => {
            let base = std::env::var("NOTE_SEARCH_DIR").unwrap_or_else(|_| ".".to_string());
            for filename in orphans {
                if absolute_path {
                    println!("{}", Path::new(&base).join(&filename).display());
                } else {
                    println!("{}", filename);
                }
            }
        }
        Err(e) => {
            eprintln!("Error finding orphans: {}", e);
            process::exit(1);
        }
    }
}

/// Lowercase, `_` as space, and reduce a path/link to its basename without
/// `.md`, so `[[Projects/Foo_Bar.md]]`, `[[foo bar]]` and `Projects/Foo Bar.md`
/// all compare equal.
fn normalize(name: &str) -> String {
    let base = name.rsplit('/').next().unwrap_or(name);
    let base = base.strip_suffix(".md").unwrap_or(base);
    base.to_lowercase().replace('_', " ")
}

/// Notes without incoming links from other notes, sorted by filename. With
/// `untagged`, only those that also have no tags.
pub fn find_orphans(conn: &rusqlite::Connection, untagged: bool) -> rusqlite::Result<Vec<String>> {
    // link name -> notes that link to it
    let mut linkers: HashMap<String, HashSet<String>> = HashMap::new();
    let mut stmt = conn.prepare("SELECT filename, link FROM note_links")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (filename, link) = row?;
        linkers
            .entry(normalize(&link))
            .or_default()
            .insert(filename);
    }

    let mut aliases: HashMap<String, Vec<String>> = HashMap::new();
    let mut stmt = conn.prepare("SELECT filename, alias FROM note_aliases")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (filename, alias) = row?;
        aliases.entry(filename).or_default().push(normalize(&alias));
    }

    let tagged: HashSet<String> = conn
        .prepare("SELECT DISTINCT filename FROM note_tags")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;

    let filenames: Vec<String> = conn
        .prepare("SELECT filename FROM markdown_data ORDER BY filename")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;

    Ok(filenames
        .into_iter()
        .filter(|filename| {
            let mut names = vec![normalize(filename)];
            names.extend(aliases.get(filename).cloned().unwrap_or_default());
            let linked = names.iter().any(|name| {
                linkers
                    .get(name)
                    .is_some_and(|sources| sources.iter().any(|s| s != filename))
            });
            !linked && !(untagged && tagged.contains(filename))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_orphans_counts_links_by_name_path_and_alias() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::markdown_parser::init_database_schema(&conn).unwrap();
        for f in [
            "a.md",
            "projects/Foo_Bar.md",
            "self.md",
            "aliased.md",
            "lonely.md",
            "tagged.md",
        ] {
            conn.execute("INSERT INTO markdown_data (filename) VALUES (?1)", [f])
                .unwrap();
        }
        for (f, l) in [
            ("a.md", "foo bar"),
            ("self.md", "self"),
            ("a.md", "Nickname"),
        ] {
            conn.execute(
                "INSERT INTO note_links (filename, link) VALUES (?1, ?2)",
                [f, l],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO note_aliases VALUES ('aliased.md', 'nickname')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO note_tags VALUES ('tagged.md', 'x')", [])
            .unwrap();

        assert_eq!(
            find_orphans(&conn, false).unwrap(),
            vec!["a.md", "lonely.md", "self.md", "tagged.md"]
        );
        assert_eq!(
            find_orphans(&conn, true).unwrap(),
            vec!["a.md", "lonely.md", "self.md"]
        );
    }
}
