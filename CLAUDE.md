# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`note_search` is a Rust CLI (+ small embedded web UI + Neovim/Lua integration) that parses a
directory of markdown notes into a SQLite database and provides rich search over notes, todos,
and JIRA-imported issues. It's a two-crate Cargo workspace:

- `note_search_core` — library crate (`note_search`) with all parsing, DB, and query logic.
- `note_search_cli` — thin binary crate (`note_search`) that only does `clap` argument parsing
  and dispatches to `note_search_core::commands::*`.
- `lua/note_search/` — Neovim plugin (Snacks picker integration) that shells out to the built
  `note_search` binary. See `NEOVIM.md` for its API.

The full CLI reference (every subcommand and flag) lives in `README.md` — check it for
user-facing behavior before re-deriving it from source. Note: README's "Project Structure"
section describes an older single-crate `src/` layout; the actual layout is the two-crate
workspace described below.

## Common Commands

```bash
# Build
cargo build
cargo build --release          # binary at ./target/release/note_search

# Run in development
cargo run -- todos --tags feature
cargo run -- notes --query "#urgent [[ProjectX]]"
cargo run -- import --input ./my_notes

# Test
cargo test                             # all tests (unit tests live inline in note_search_core modules)
cargo test -- --nocapture
cargo test search_criteria::tests      # single module, e.g. also query_builder::tests, markdown_parser::tests, attribute_pair::tests

# Install locally
cargo install --path note_search_cli
```

There is no lint/format CI config in the repo; use standard `cargo fmt` / `cargo clippy` if asked
to clean up code, but don't assume either is wired into a pipeline.

## Architecture

### Command dispatch

`note_search_cli/src/main.rs` defines the entire `clap` CLI surface (`Cli`, `Commands` enum) and
does nothing else — every subcommand match arm immediately calls into a handler in
`note_search_core::commands::<name>` (e.g. `commands::search::handle_todos_search`,
`commands::import::handle_import`, `commands::agenda::handle_agenda`). When adding a new
subcommand: add the `clap` variant in `main.rs`, then add the handler module under
`note_search_core/src/commands/`.

Shared search flags (`--tags`, `--links`, `--query`, `--date-range`, `--sort`, etc.) live in
`commands/args.rs` as `CommonSearchArgs`, flattened into `TodoSearchArgs` and reused by `Notes`
and `Agenda`.

### Search/query pipeline

Three layers, used identically by the CLI and the web server:

1. **`query_parser.rs`** — hand-written recursive-descent parser (`Parser`, `parse_query`) that
   turns the Obsidian-like `--query` DSL (`word`, `#tag`, `[[link]]`, `[attr:value]`,
   `(a OR b)`) into a `QueryExpr` AST.
2. **`search_criteria.rs`** — `SearchCriteria` struct is the normalized set of filters (tags,
   links, attributes, text, date range, sort, etc.) built either from a `QueryExpr` or directly
   from individual CLI flags. `DateRange` handles relative ranges (`today`, `this_week`, ...) by
   converting to concrete `YYYYMMDD` bounds.
3. **`query_builder.rs`** — `QueryBuilder` turns `SearchCriteria` into parameterized SQL
   (`build_query`) against the two SQLite tables below.

`database_service.rs`'s `DatabaseService` is the single data-access layer: it opens the SQLite
connection, calls `QueryBuilder`, and maps rows to `NoteResult`/`TodoResult` (both `Serialize`).
Both `commands/search.rs` (CLI) and `web/mod.rs` (JSON API) call `DatabaseService` directly — this
is the layer to extend if you're adding a new filter/sort field, rather than duplicating SQL in
each caller.

### Database schema

Two tables, created/migrated in `markdown_parser.rs::init_database_schema`:

- `markdown_data` — one row per note: `filename` (PK, path relative to the import root),
  `created`, `updated`, `title`, `todo_count`, `link_count`, `header_fields` (YAML frontmatter as
  JSON), `links` (JSON), `body`, `tags`.
- `todo_entries` — one row per todo checkbox item: `id`, `filename` (FK), `closed`, `priority`,
  `due`, `text`, `tags`, `links` (JSON), `line_number`, `updated`.

Some columns (`markdown_data.created`/`.tags`, `todo_entries.updated`) were added later via
best-effort `ALTER TABLE` in `init_database_schema` for backward compatibility with existing
databases — don't assume the schema is fully defined by the initial `CREATE TABLE` statements
alone.

### Import path

`markdown_parser.rs` does the actual parsing (frontmatter, todos, tags, links, tag-hierarchy
expansion) and writes to SQLite (`write_markdown_data_to_sqlite_with_conn`,
`update_files_in_db`). `commands/import.rs` wraps this for one-shot and `--watch` (polling)
imports. `commands/mapping.rs` applies the INI-based attribute-name unification config
(`~/.config/note_search/config`, or `NOTE_SEARCH_CONFIG`) during import.

### Other integrations

- `jira.rs` / `commands/jira.rs` — JIRA REST import (single issue or JQL query) as markdown with
  frontmatter, including optional mTLS (`JIRA_HOST_CERTIFICATE`) and custom CA
  (`JIRA_CA_CERTIFICATE`) support.
- `converter.rs` / `commands/convert.rs` — converts a URL (web page, Reddit thread) or local file
  (`.docx`, `.pdf`, `.eml`, `.msg`) into a markdown note.
- `commands/browser_history.rs` — reads Safari/Vivaldi/Firefox history DBs and emits a markdown
  note.
- `commands/linker.rs` — rewrites plain-text project/person names in notes into `[[wiki links]]`.
- `web/mod.rs` — small `axum` server (`start_server`): serves one embedded HTML page (`GET /`) and
  a JSON API (`/api/search`, `/api/note`, `/api/projects`, `/api/persons`) over the same
  `DatabaseService` the CLI uses. Supports `--watch` to keep the DB in sync while serving.
- `lua/note_search/` — Neovim front-end; calls the compiled binary as a subprocess and parses its
  output. Not part of the Cargo workspace; edited/tested independently of `cargo test`.

---

## Behavioral Guidelines

Behavioral guidelines to reduce common LLM coding mistakes. Merge with project-specific instructions as needed.

**Tradeoff:** These guidelines bias toward caution over speed. For trivial tasks, use judgment.

### 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

### 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

### 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:
- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

### 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:
```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

---

**These guidelines are working if:** fewer unnecessary changes in diffs, fewer rewrites due to overcomplication, and clarifying questions come before implementation rather than after mistakes.
