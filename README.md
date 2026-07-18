# Note Search

All about notes. This packages is split into two pieces

1. A markdown parser called note_search which is written in Rust
2. Some LUA code to create and search for notes within NeoVim.

## note_search Parser

### Features

- **Import**: Parse markdown files and store todos, tags, links, and frontmatter in a SQLite database
- **Search**: Query the database using various filters (tags, links, text, priority, due date, date ranges, etc.)

### Usage

```
note_search [OPTIONS] [COMMAND]
```

#### Commands

- `todos`: Search for todo entries in the database
  - See [Todo Search Options](#todo-search-options) below
- `notes`: Search for notes (documents) in the database
  - See [Note Search Options](#note-search-options) below
- `import`: Import markdown files into the database
  - `-i, --input <PATH>`: Input directory containing markdown files (optional if `NOTE_SEARCH_DIR` is set)
  - `-o, --output <PATH>`: Output database path (optional, defaults to -d value)
  - `--watch`: Watch mode for continuous monitoring
  - `--interval <SECONDS>`: Watch interval in seconds (default: 60)
  - `--browser-history`: Also create a browser history note after import
  - `--browser-history-interval <SECONDS>`: Browser history refresh interval in watch mode (default: 28800 / 8h)
- `clear`: Clear all data from the database
  - `--yes`: Confirm clearing without interactive prompt
- `values`: List unique values for a specific field from the database
  - `<FIELD>`: Field to list values for (priority, due_date, link, tag, attr:ATTRIBUTE)
- `attributes`: List all known attribute names from the database
  - See [Listing Attribute Names](#listing-attribute-names) below
- `backlinks`: List documents that link to a given markdown file
  - `<FILENAME>`: Filename to find backlinks for (e.g., `note_search backlinks myfile.md`)
  - `--markdown`: Output backlinks as markdown links instead of plain filenames
- `list-names`: List all note names (filenames without path and extension)
  - See [Listing Note Names](#listing-note-names) below
- `info`: Display all information for a given document
  - `<FILENAME>`: Filename (or filename suffix) of the document to show
  - See [Document Info](#document-info) below
- `agenda`: Generate an agenda view of projects and their open todos
  - See [Agenda View](#agenda-view) below
- `convert`: Convert a web page or document to a markdown note
  - `<SOURCE>`: URL or file path to convert
  - `-o, --output <PATH>`: Output directory (optional if `NOTE_SEARCH_DIR` is set)
  - See [Converting Documents to Notes](#converting-documents-to-notes) below
- `linker`: Link project and person names in notes to their wiki links
  - `<SUBDIR>`: Subdirectory within the note root directory to process
  - See [Linking Entity Names](#linking-entity-names) below
- `jira`: Import JIRA issues as markdown notes
  - `[JQL]`: JQL query to filter issues (defaults to issues assigned to current user)
  - `-o, --output <PATH>`: Output directory (optional if `NOTE_SEARCH_DIR` is set)
- `jira-issue`: Fetch a single JIRA issue as markdown
  - `<ISSUE_KEY>`: Issue key to fetch (e.g., `PROJ-123`)
  - `-o, --output <PATH>`: Output directory for saving the issue
  - `-p, --print`: Print to stdout instead of saving to file
  - See [Fetching a Single JIRA Issue](#fetching-a-single-jira-issue) below
- `browser-history`: Import browser history from Safari, Vivaldi, and Firefox
  - `[DATE]`: Date to fetch history for (YYYY-MM-DD, defaults to today)
  - `-n, --days <N>`: Number of days to include (default: 1)
  - `-o, --note-dir <PATH>`: Output directory for the note (defaults to `NOTE_SEARCH_DIR/web/`)
  - `-t, --use-timestamp`: Use last timestamp from previous run (overrides `--days`)
  - See [Browser History Import](#browser-history-import) below
- `web`: Start the JSON API + dashboard web server
  - `-p, --port <PORT>`: Port to serve on (default: 3000)
  - `--watch`: Watch mode, continuously re-import the note directory while serving
  - See [Web Server](#web-server) below
- `create-note`: Dynamically create or append to a note (see [Create Note](#create-note) below)
- `help`: Show help for a subcommand

#### Todo Search Options

- `-d, --database <PATH>`: Specify the database file to use (default: ./note.sqlite)
- `--tags <tag1,tag2,...>`: Search for todos with specified tags (all must match)
- `--links <link1,link2,...>`: Search for todos with specified links (all must match)
- `--attributes <key1=value1,key2=value2,...>`: Search for todos with specific attribute values in the header fields
- `--text <search_text>`: Search for todos containing the specified text
- `--search-body <search_text>`: Search for text in the note body (case-insensitive)
- `--priority <priority>`: Search for todos with the specified priority
- `--due-date <YYYYMMDD>`: Search for todos due on or before the specified date
- `--due-date-eq <YYYYMMDD>`: Search for todos due exactly on the specified date
- `--due-date-gt <YYYYMMDD>`: Search for todos due on or after the specified date
- `--open`: Search for open todos only
- `--closed`: Search for closed todos only
- `--date-range <RANGE>`: Search for todos created in a date range (today, yesterday, this_week, last_week, this_month, last_month, this_year, last_year)
- `--start-date <YYYYMMDD>`: Search for todos created on or after this date
- `--end-date <YYYYMMDD>`: Search for todos created on or before this date
- `--format <FORMAT>`: Configure output format using placeholders
- `--sort <FIELD>`: Sort results by field (due_date, priority, filename, modified, attr:ATTRIBUTE, text)
- `--list`: List only file locations without todo text
- `--absolute-path`: Output absolute paths instead of relative paths

#### Note Search Options

- `-d, --database <PATH>`: Specify the database file to use (default: ./note.sqlite)
- `--tags <tag1,tag2,...>`: Search for notes with specified tags in frontmatter (all must match)
- `--links <link1,link2,...>`: Search for notes with specified links (all must match)
- `--attributes <key1=value1,key2=value2,...>`: Search for notes with specific attribute values in the header fields
- `--text <search_text>`: Search for notes containing the specified text in title or frontmatter
- `--search-body <search_text>`: Search for text in the note body (case-insensitive)
- `--date-range <RANGE>`: Search for notes created in a date range (today, yesterday, this_week, last_week, this_month, last_month, this_year, last_year)
- `--start-date <YYYYMMDD>`: Search for notes created on or after this date
- `--end-date <YYYYMMDD>`: Search for notes created on or before this date
- `--format <FORMAT>`: Configure output format using placeholders (supports: filename, title, todo_count, link_count, links, attr:NAME)
- `--sort <FIELD>`: Sort results by field (filename, modified, attr:ATTRIBUTE, text)
- `--list`: List only file locations without note details
- `--absolute-path`: Output absolute paths instead of relative paths

### Obsidian-like Query Syntax

Both `todos` and `notes` commands support a `--query` flag that accepts an Obsidian-inspired search syntax. When `--query` is provided, it overrides the individual `--tags`, `--links`, `--text`, and `--search-body` flags.

#### Syntax Elements

| Syntax | Meaning | Example |
| -------- | --------- | -------- |
| `word` | Search for text in note title, body, and frontmatter | `meeting` |
| `"quoted words"` | Search for an exact phrase | `"project alpha"` |
| `#tag` | Search for a tag | `#urgent` |
| `[[link]]` | Search for a wiki link reference | `[[ProjectX]]` |
| `@name` | Synonym for `[[name]]` (link search) | `@ProjectX` |
| `[attribute]` | Search for notes where an attribute is defined | `[status]` |
| `[attribute:value]` | Search for notes with a specific attribute value | `[type:meeting]` |
| `(expr OR expr)` | Logical OR between expressions | `(bug OR feature)` |

#### How It Works

- **Implicit AND**: All terms at the same level are combined with AND. A note must match all of them.
- **OR groups**: Use parentheses with `OR` to match any of the alternatives.
- **Nesting**: Parentheses can be nested for complex queries.

#### Examples

```bash
# Simple text search
note_search notes --query "meeting"

# Multiple words (AND) — note must contain all words
note_search notes --query "project alpha review"

# Tags and links
note_search notes --query "#urgent [[ProjectX]]"

# @name as link synonym
note_search notes --query "@ProjectX #active"

# Attribute exists
note_search notes --query "[status]"

# Attribute with specific value
note_search notes --query "[type:meeting]"

# OR grouping
note_search notes --query "(bug OR feature)"

# Mixed AND with OR
note_search notes --query "#urgent (bug OR feature)"

# Complex query with all syntax elements
note_search notes --query "word1 [[note1]] #tag1 [status:draft] (word2 OR word3)"

# Works for todos too
note_search todos --query "[author:John] #action follow"
```

#### Due Date Search Examples

Search for todos due exactly on a specific date:

``` bash
note_search todos --due-date-eq 20260315
```

Search for todos due before or on a date (overdue and due today):

``` bash
note_search todos --due-date 20260315
```

Search for todos due on or after a date (future tasks):

``` bash
note_search todos --due-date-gt 20260315
```

Combine with other filters:

``` bash
# Overdue high priority tasks
note_search todos --due-date 20260314 --priority A --open

# Tasks due tomorrow exactly
note_search todos --due-date-eq 20260316
```

#### Date Range Search

Search for todos based on the note's creation date (from the `created` field in YAML frontmatter):

**Note:** The `created` field should be in `YYYY-MM-DD` format in your markdown frontmatter:

``` markdown
---
title: My Note
created: 2026-03-27
---
```

**Relative Date Ranges (for todos):**

``` bash
# Todos from notes created today
note_search todos --date-range today

# Todos from notes created yesterday
note_search todos --date-range yesterday

# Todos from notes created this week (Monday-Sunday, ISO 8601)
note_search todos --date-range this_week

# Todos from notes created last week
note_search todos --date-range last_week

# Todos from notes created this month
note_search todos --date-range this_month

# Todos from notes created last month
note_search todos --date-range last_month

# Todos from notes created this year
note_search todos --date-range this_year

# Todos from notes created last year
note_search todos --date-range last_year
```

**Relative Date Ranges (for notes):**

``` bash
# Notes created today
note_search notes --date-range today

# Notes created this week
note_search notes --date-range this_week

# Notes created this month
note_search notes --date-range this_month
```

**Custom Date Ranges:**

``` bash
# Todos from notes created on or after a specific date
note_search todos --start-date 20260101

# Todos from notes created on or before a specific date
note_search todos --end-date 20260331

# Todos from notes created within a date range
note_search todos --start-date 20260101 --end-date 20260331

# Notes created within a date range
note_search notes --start-date 20260101 --end-date 20260331
```

**Combine with other filters:**

``` bash
# Recent urgent tasks from notes created this week
note_search todos --date-range this_week --tags urgent --open

# High priority tasks from notes created in January
note_search todos --start-date 20260101 --end-date 20260131 --priority A

# Completed tasks from notes created yesterday
note_search todos --date-range yesterday --closed

# Notes created this week with specific tags
note_search notes --date-range this_week --tags project
```

#### Sorting Results

Sort todo results by various fields:

``` bash
# Sort by due date (earliest first, NULLs last)
note_search todos --open --sort due_date

# Sort by priority (A-Z, NULLs last)
note_search todos --open --sort priority

# Sort by filename
note_search todos --open --sort filename

# Sort by file modification time (most recent first)
note_search todos --open --sort modified

# Sort by todo text alphabetically
note_search todos --open --sort text

# Sort by a document attribute (e.g., author from YAML frontmatter)
note_search todos --open --sort attr:author

# Combine with other filters
note_search todos --tags urgent --sort due_date --open
```

Sort note results by various fields:

``` bash
# Sort by filename
note_search notes --sort filename

# Sort by file modification time (most recent first)
note_search notes --sort modified

# Sort by title alphabetically
note_search notes --sort text

# Sort by a document attribute
note_search notes --sort attr:author
```

#### Clearing the Database

Clear all data from the database (with confirmation prompt):

``` bash
# Clear default database (asks for confirmation)
note_search clear

# Clear specific database (asks for confirmation)
note_search -d /path/to/database.sqlite clear

# Clear without confirmation (--yes flag)
note_search clear --yes

# Combined with database flag
note_search -d my_notes.db clear --yes
```

**Warning**: The `clear` command permanently deletes all markdown data and todo entries from the database. This operation cannot be undone.

#### Listing Unique Values

List all unique values for specific fields in the database:

``` bash
# List all unique priorities
note_search values priority

# List all unique due dates
note_search values due_date

# List all unique tags
note_search values tag

# List all unique links
note_search values link

# List all unique values for a specific attribute (from YAML frontmatter)
note_search values attr:author
note_search values attr:type

# With specific database
note_search -d my_notes.db values priority
```

**Supported fields:**

- `priority` - Unique priority values (A, B, C, etc.)
- `due_date` - Unique due dates (YYYYMMDD format)
- `tag` - Unique tags found in todos
- `link` - Unique links found in todos
- `attr:ATTRIBUTE` - Unique values for a specific document attribute (e.g., `attr:author`, `attr:title`)

#### Listing Attribute Names

List all known YAML frontmatter attribute names found across the database (useful for discovering what you can pass to `values attr:NAME` or `--attributes`):

``` bash
note_search attributes

# With specific database
note_search -d my_notes.db attributes
```

#### Listing Note Names

List all note names (filenames without their directory path or `.md` extension) — handy for autocompletion or piping into other tools:

``` bash
note_search list-names
```

#### Document Info

Show all stored information for a single document (title, frontmatter attributes, todos, links, and backlinks):

``` bash
# Exact filename
note_search info myfile.md

# Matches by suffix if not found exactly (e.g. projects/myfile.md)
note_search info myfile.md
```

If multiple documents match by suffix, the command lists the candidates and asks you to specify the full path.

#### Finding Backlinks

List all documents that link to a specific markdown file:

``` bash
# Find all documents that link to myfile.md
note_search backlinks myfile.md

# With specific database
note_search -d my_notes.db backlinks important_doc.md

# Output as markdown links instead of plain filenames
note_search backlinks myfile.md --markdown
```

This searches for both wiki-style links (`[[filename]]`) and markdown links (`[text](filename.md)`). The command looks for links in:

- Document-level links stored in the markdown_data table
- Todo-level links stored in the todo_entries table

#### Importing JIRA Issues

Import JIRA issues as markdown notes with frontmatter and comments:

``` bash
# Import issues assigned to current user
note_search jira

# Import with custom JQL query
note_search jira "project = PROJ AND status = Open"

# Import to specific output directory
note_search jira "assignee = currentUser()" -o ./my_notes
```

**Prerequisites:**

- Set `JIRA_SERVER` environment variable to your JIRA instance URL
- Set `JIRA_API_TOKEN` environment variable to your JIRA API token (or the legacy `JIRA_KEY`)

**Example setup:**

``` bash
export JIRA_SERVER="https://company.atlassian.net"
export JIRA_API_TOKEN="your-api-token-here"
```

The command creates markdown files in `<output_dir>/jira/` with:

- YAML frontmatter containing issue metadata (key, status, priority, assignee, reporter, labels, dates)
- Issue description
- All comments with author and timestamp

#### Fetching a Single JIRA Issue

Fetch one JIRA issue as markdown without importing a whole directory:

``` bash
# Print to stdout
note_search jira-issue PROJ-123 --print

# Save to a directory
note_search jira-issue PROJ-123 -o ./my_notes

# No output directory and no --print defaults to printing to stdout
note_search jira-issue PROJ-123
```

Uses the same `JIRA_SERVER` / `JIRA_API_TOKEN` (or `JIRA_KEY`) prerequisites as `jira`.

#### Agenda View

Generate a project/department/person/company-centric view of open todos, grouped by note:

``` bash
# Agenda across all project notes (type: project), sorted by due date
note_search agenda

# Agenda for a specific note
note_search agenda myproject.md

# Agenda scoped to persons or companies instead of projects
note_search agenda --persons
note_search agenda --companies
note_search agenda --departments

# Filter like todos: priority, due date, open/closed, tags, text, etc.
note_search agenda --priority A --open

# Hide the summary section
note_search agenda --no-summary
```

The agenda groups open (by default) todos under the project/person/company note that owns them (matched by `type` in YAML frontmatter), and accepts the same filtering flags as [Todo Search Options](#todo-search-options) via `CommonSearchArgs`, plus `--priority`, `--due-date`, `--due-date-eq`, `--due-date-gt`, `--open`, `--closed`.

#### Converting Documents to Notes

Convert a web page, GitHub page, Reddit thread, email, Outlook message, or office document into a markdown note with frontmatter:

``` bash
# Web page
note_search convert https://example.com/article -o ./my_notes

# Reddit discussion (thread + top comments)
note_search convert https://reddit.com/r/rust/comments/xyz -o ./my_notes

# GitHub page
note_search convert https://github.com/owner/repo/issues/1 -o ./my_notes

# Email (.eml) or Outlook message (.msg)
note_search convert ./message.eml -o ./my_notes
note_search convert ./message.msg -o ./my_notes

# Office documents (.docx, .pdf)
note_search convert ./report.docx -o ./my_notes
note_search convert ./report.pdf -o ./my_notes
```

The source type is auto-detected from the URL/extension. If `-o` is omitted, `NOTE_SEARCH_DIR` is used.

#### Linking Entity Names

Scan markdown files under a subdirectory of `NOTE_SEARCH_DIR` and rewrite plain-text mentions of known project/person names (pulled from the database) into `[[wiki links]]`:

``` bash
# Link entity names found in files under NOTE_SEARCH_DIR/projects
note_search linker projects
```

Requires the database to already be populated (`note_search import` first). Names are matched longest-first to avoid partial-match collisions.

#### Browser History Import

Import browser history from Safari, Vivaldi, and Firefox into a markdown note:

``` bash
# Today's history from all supported browsers
note_search browser-history

# Specific date
note_search browser-history 2026-07-15

# Last 7 days
note_search browser-history --days 7

# Continue from the last recorded timestamp instead of a fixed day range
note_search browser-history --use-timestamp

# Custom output directory (defaults to NOTE_SEARCH_DIR/web/)
note_search browser-history --note-dir ./my_notes/web
```

Also runs automatically during `import --watch` when `--browser-history` is passed; see the `import` section above.

#### Web Server

Serve a small dashboard and JSON API over the same database used by the CLI:

``` bash
# Start on the default port (3000)
note_search web

# Custom port
note_search web --port 8080

# Watch mode: keep re-importing NOTE_SEARCH_DIR while serving
note_search web --watch
```

Endpoints:

- `GET /` — single-page dashboard for browsing and searching notes/todos
- `GET /api/search` — JSON search endpoint (`text`, `q` for the Obsidian-like query syntax, `attributes`, `kind`)
- `GET /api/note` — a single note's title, content, and backlinks as JSON
- `GET /api/projects`, `GET /api/persons` — distinct attribute values for UI filters

### Environment Variables

You can set default values using environment variables:

- `NOTE_SEARCH_DATABASE`: Default database path (overridden by `-d` flag)
- `NOTE_SEARCH_DIR`: Default input directory for import (overridden by `--input` flag)
- `NOTE_SEARCH_CONFIG`: Path to the mapping configuration file (overrides the default `~/.config/note_search/config` path)
- `JIRA_SERVER`: URL of the JIRA server (e.g., `https://company.atlassian.net`)
- `JIRA_API_TOKEN`: API token for JIRA authentication (preferred)
- `JIRA_KEY`: Legacy API token for JIRA authentication; used as a fallback when `JIRA_API_TOKEN` is not set
- `JIRA_CA_CERTIFICATE`: Path to a PEM bundle used to verify the JIRA server's host certificate (optional). Added as an additional root certificate.
- `JIRA_HOST_CERTIFICATE`: Path to a PKCS#12 archive (`.p12`/`.pfx`) used as the client identity for mutual TLS (optional). Decrypted with `JIRA_HOST_CERTIFICATE_PASSWORD`.
- `JIRA_HOST_CERTIFICATE_PASSWORD`: Password for the PKCS#12 archive specified by `JIRA_HOST_CERTIFICATE` (required when `JIRA_HOST_CERTIFICATE` is set)

#### Environment Variable Examples

``` bash
# Set defaults in your shell profile
export NOTE_SEARCH_DATABASE="$HOME/.notes/note.sqlite"
export NOTE_SEARCH_DIR="$HOME/notes"

# Set JIRA credentials (required for jira command)
export JIRA_SERVER="https://company.atlassian.net"
export JIRA_API_TOKEN="your-api-token-here"
# (optional) mTLS / custom CA for a JIRA server behind a host certificate
export JIRA_CA_CERTIFICATE="/etc/ssl/jira-ca.pem"
export JIRA_HOST_CERTIFICATE="/etc/ssl/jira-client.p12"
export JIRA_HOST_CERTIFICATE_PASSWORD="your-pkcs12-password"

# Now you can search without specifying -d
note_search --open

# And import without specifying --input
note_search import

# Import JIRA issues
note_search jira

# CLI flags still override environment variables
note_search -d /other/db.sqlite --open  # Uses /other/db.sqlite instead of default
```

### Attribute Mapping Configuration

You can configure `note_search` to automatically map and unify attribute names from your markdown documents. This is useful if you have notes that use different terms (e.g., `participant` vs `participants`) for the same concept and want them stored under a single attribute name.

By default, the configuration is read from `~/.config/note_search/config`. You can override this location using the `NOTE_SEARCH_CONFIG` environment variable.

The file uses the standard INI format:

```ini
[Mapping]
participants=people
participant=people
projects=project
```

With this configuration, whenever `note_search` imports a document containing `participants` or `participant` in its frontmatter or as a markdown heading, those values will be automatically merged into the `people` attribute and the original attribute names will be removed.

For example, if a note has:

```markdown
---
participant: [[Alice]]
participants:
  - [[Bob]]
  - [[Carol]]
---

# People
- [[Dave]]
```

All four names will be combined into a single `people` attribute in the database.

### Tag Hierarchy

Tags support hierarchies using forward slashes (`/`). When a note contains a tag like `#project/alpha`, `note_search` automatically expands it to include all parent tags.

For example, a note containing:

```markdown
This task is related to #project/alpha/feature.
```

Will be automatically tagged with:

- `project`
- `project/alpha`
- `project/alpha/feature`

This allows you to search for a parent tag (e.g., `project`) to find all notes within that category, while still being able to drill down into specific sub-tags when needed.

### Create Note

You can dynamically create or append to notes using the `create-note` command. Currently, the `daily` type is supported, which creates a daily note for the current day (if it doesn't exist) and appends text under the `## Yournal` heading.

```bash
# Append text to today's daily note
note_search create-note -t daily "This is a new entry inside the daily note"
```

The note path defaults to `$NOTE_SEARCH_DIR`. If the daily note does not exist, it will be created using the `daily.md` template. The system searches for templates in the following locations (in order):

1. `~/.local/share/note_search/templates/daily.md`
2. `$NOTE_SEARCH_DIR/templates/daily.md`

The following placeholders are supported in the template:

- `{{date}}` — The current date in `YYYY-MM-DD` format
- `{{time}}` — The current time in `HH:MM` format
- `{{date_human}}` — A human-readable date format (e.g., `Tuesday, May 19, 2026`)

If the `## Yournal` heading is missing, it will be appended to the end of the file along with the new entry.

### Import Markdown Files

Parse a directory of markdown files and store the extracted data:

``` bash
# Import markdown files to default database
note_search import --input ./my_notes

# Import using environment variable (if NOTE_SEARCH_DIR is set)
note_search import

# Import to specific database
note_search -d my_database.sqlite import --input ./my_notes

# Import to a different output database
note_search import --input ./my_notes --output ./other_database.sqlite
```

#### Watch Mode

Monitor a directory continuously and automatically import markdown files as they are added or modified:

``` bash
# Watch directory with default 60-second interval
note_search import --input ./my_notes --watch

# Watch with custom interval (e.g., check every 10 seconds)
note_search import --input ./my_notes --watch --interval 10

# Watch with specific database
note_search -d my_database.sqlite import --input ./my_notes --watch
```

In watch mode:

- Performs an initial import of all existing markdown files
- Monitors the directory at the specified interval for:
  - **New files**: Automatically imported when added to the directory
  - **Modified files**: Re-imported when file content changes (mtime is checked)
- Old content is replaced with new content when files are modified
- Shows timestamps when imports/updates occur
- Press Ctrl+C to stop watching

**Example workflow:**

``` bash
# Start watching your notes directory
note_search import --input ~/notes --watch --interval 30

# In another terminal, add or edit files...
echo "- [ ] New task" >> ~/notes/todo.md
# Watch mode will detect the change and update the database automatically
```

#### Directory Structure

When importing, the tool preserves the directory structure relative to the input directory. Files in subdirectories will have their relative path included in the filename stored in the database.

For example, if your directory structure is:

```
my_notes/
├── todo.md
├── projects/
│   └── ideas.md
└── archive/
    └── 2024/
        └── old_todo.md
```

After importing with `note_search import --input ./my_notes`, the database will contain:

- `todo.md`
- `projects/ideas.md`
- `archive/2024/old_todo.md`

This allows you to search within specific subdirectories:

``` bash
# Search for todos in the projects directory
note_search --text "projects/" --list

# Search for todos in archive
note_search --text "archive/" --open
```

#### Supported Markdown Format

The tool recognizes:

- **YAML frontmatter** (between `---` markers)
- **TODO entries** as checkbox items: `- [ ]` (open) or `- [x]` (closed)
- **Tags**: Extracted from `#tag` format or `tag: TAG` format. Tags must consist only of letters (`A-Z`, `a-z`), German umlauts (`äöüÄÖÜß`), forward slashes (`/`), or underscores (`_`), and the `#` symbol must be at the beginning of the line or preceded by a whitespace character.
- **Priority**: Extracted from `priority: A` format
- **Due dates**: Extracted from `due: 20260101` format (YYYYMMDD)
- **Created dates**: Extracted from `created: 2026-03-27` format (YYYY-MM-DD) for date range filtering
- **Links**: Markdown links `[text](url)` and wiki-style links `[[Page]]`

Example markdown:

``` markdown
---
title: Project Notes
author: John Doe
created: 2026-03-27
---

# My Project

- [ ] Implement feature priority: A due: 20260101 #feature
- [x] Fix bug #bug
- [ ] Review documentation #docs
```

### Search Examples

#### Todo Search Examples

Search for todos with specific tags:

```
note_search todos --tags feature,documentation
```

Search for todos due before a certain date:

```
note_search todos --due-date 20260315
```

Search for todos from notes created today:

```
note_search todos --date-range today
```

Search for todos from notes created this week with specific tags:

```
note_search todos --date-range this_week --tags urgent
```

Search for todos from notes created within a custom date range:

```
note_search todos --start-date 20260101 --end-date 20260331 --open
```

Search for open todos with specific text:

```
note_search todos --open --text "login"
```

Search for todos with specific attributes:

```
note_search todos --attributes type=meeting,author=Stefan_Harbeck
```

Show tags and links in output:

```
note_search todos --tags feature --format "{tags} - {text}"
```

Search for todos in notes mentioning "architecture" in the body:

```
note_search todos --search-body architecture --open
```

Search for todos in notes discussing "API design":

```
note_search todos --search-body "API design" --tags urgent
```

#### Note Search Examples

Search for notes with specific tags in frontmatter:

```
note_search notes --tags project,active
```

Search for notes created today:

```
note_search notes --date-range today
```

Search for notes created this month with specific tags:

```
note_search notes --date-range this_month --tags meeting
```

Search for notes by title text:

```
note_search notes --text "Project Alpha"
```

Search for notes with specific attributes:

```
note_search notes --attributes author=John,status=draft
```

Search for notes mentioning "architecture" in the body:

```
note_search notes --search-body architecture
```

Search for notes discussing "refactoring" in project notes:

```
note_search notes --search-body refactoring --tags project
```

Show title and todo count:

```
note_search notes --format "{title} [{todo_count} todos]"
```

Show specific document attributes in todos:

```
note_search todos --open --format "{attr:author}: {text}"
```

Combine format fields for todos:

```
note_search todos --tags bug --format "{filename}:{line_number} - {priority} {text} [{attr:participant}]"
```

Custom format with all todo fields:

```
note_search todos --format "[{due_date}] {priority}: {text} ({filename}:{line_number})"
```

List only todo file locations (useful for piping to other tools):

```
note_search todos --tags feature --list
```

List only note file locations:

```
note_search notes --tags project --list
```

Note: When using `--list` with `todos`, each file is shown only once, even if it contains multiple matching todos.

### Output Format

The `--format` option allows you to customize the output using placeholders in curly braces:

**For todos:**

- `{filename}` - The filename containing the todo
- `{line_number}` - The line number of the todo
- `{text}` - The todo text
- `{priority}` - The todo priority
- `{due_date}` - The due date
- `{tags}` - The todo tags
- `{links}` - The todo links
- `{attr:NAME}` - A specific attribute from the document header (e.g., `{attr:author}`)

**For notes:**

- `{filename}` - The filename of the note
- `{title}` - The note title
- `{todo_count}` - Number of todos in the note
- `{link_count}` - Number of links in the note
- `{links}` - The note's links
- `{attr:NAME}` - A specific attribute from the document header (e.g., `{attr:author}`)

The format string can include any text and placeholders in any order.

### Requirements

- Rust 1.70 or higher
- Cargo (included with Rust)

### Building and Running

### Development build

``` bash
# Build the project
cargo build

# Search for todos
cargo run -- todos --tags feature

# Search for notes
cargo run -- notes --tags project

# Import markdown files
cargo run -- import --input ./my_notes
```

#### Release build (optimized)

``` bash
# Build optimized release
cargo build --release

# The binary will be in ./target/release/note_search
./target/release/note_search todos --tags feature
./target/release/note_search notes --date-range this_week
./target/release/note_search import --input ./my_notes
```

#### Installation

``` bash
# Install the binary to ~/.cargo/bin (from workspace root)
cargo install --path note_search_cli

# Or from the CLI directory
cd note_search_cli && cargo install --path .

# Now you can use note_search from anywhere
note_search --help
note_search todos --help
note_search notes --help
note_search import --input ./my_notes
```

### Testing

The project includes comprehensive unit tests covering all modules:

``` bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific test module
cargo test search_criteria::tests
cargo test query_builder::tests
cargo test markdown_parser::tests
cargo test attribute_pair::tests
```

Test coverage includes:

- **SearchCriteria**: Default values, has_any_criteria logic
- **QueryBuilder**: SQL query construction with various criteria
- **AttributePair**: Creation, equality, trimming, display
- **MarkdownParser**: Frontmatter extraction, TODO parsing, YAML conversion, database operations

### Database Structure

The tool creates and searches in a SQLite database with two tables:

1. `markdown_data` - Contains document metadata including:
    - `filename` - Relative path from input directory (primary key)
    - `created` - Creation date, from the `created` frontmatter field (YYYYMMDD)
    - `updated` - Last update timestamp
    - `title` - Document title
    - `header_fields` - JSON string of YAML frontmatter
    - `links` - JSON array of document-level links
    - `body` - Full markdown body content (excluding frontmatter)
    - `tags` - JSON array of tags found in the document
    - `todo_count` and `link_count` - Summary counts
2. `todo_entries` - Contains todo items with:
    - `id` - Auto-incrementing primary key
    - `filename` - Reference to markdown_data
    - `closed` - Boolean indicating if todo is done
    - `priority` - Priority (A-Z)
    - `due` - Due date (YYYYMMDD)
    - `text` - Todo text
    - `tags` and `links` - JSON arrays
    - `line_number` - Line in the markdown file
    - `updated` - Last update timestamp

Todo items can have tags and links that are stored either directly in the todo entry or in the document's header fields.

### Project Structure

This is a two-crate Cargo workspace (see `Cargo.toml`):

- `note_search_cli/` - the `note_search` binary crate
  - `src/main.rs` - CLI argument parsing (`clap`) and dispatch into `note_search_core::commands::*`
  - `completions/_note_search` - Zsh completion script
  - `completions/note_search.bash` - Bash completion script
  - `man/note_search.1` - Manual page
- `note_search_core/` - the `note_search` library crate with all parsing, database, and search logic
  - `src/lib.rs` - Library module exports
  - `src/commands/` - One handler module per subcommand (`search.rs`, `import.rs`, `agenda.rs`, `convert.rs`, `linker.rs`, `jira.rs`, `browser_history.rs`, `backlinks.rs`, `metadata.rs`, `list_names.rs`, `info.rs`, `clear.rs`, `create_note.rs`, `mapping.rs`, `args.rs`)
  - `src/markdown_parser.rs` - Markdown parsing, database schema, and import
  - `src/query_parser.rs` - Parser for the Obsidian-like `--query` DSL
  - `src/search_criteria.rs` - Normalized search criteria struct
  - `src/query_builder.rs` - SQL query builder
  - `src/attribute_pair.rs` - Attribute key-value pairs
  - `src/database_service.rs` - Shared database access layer used by both the CLI and the web server
  - `src/jira.rs` - JIRA API integration
  - `src/converter.rs` - Web page / document / email / message to markdown conversion
  - `src/web/` - Embedded `axum` web server (dashboard + JSON API)
- `lua/note_search/` - Neovim plugin (Snacks picker integration); see `NEOVIM.md`
- `Cargo.toml` - Workspace manifest

### Dependencies

Core crate (`note_search_core`):

- `rusqlite` - SQLite database driver with bundled SQLite
- `clap` - Command-line argument parsing with subcommand support
- `chrono` - Date and time handling
- `serde`, `serde_json`, `serde_yaml` - Serialization
- `yaml-rust2` - YAML frontmatter parsing
- `regex` - Pattern matching for todo extraction
- `walkdir` - Directory traversal
- `reqwest` - HTTP client (JIRA API, web page/GitHub/Reddit conversion)
- `scraper`, `html2md` - HTML parsing and HTML-to-markdown conversion
- `docx-rs`, `lopdf` - `.docx` and `.pdf` document conversion
- `mail-parser`, `msg_parser` - `.eml` and Outlook `.msg` conversion
- `url` - URL parsing
- `dirs` - Platform config/data directory lookup
- `strsim` - String similarity (entity-name matching for `linker`)
- `base64` - Encoding (JIRA API auth)
- `tokio`, `axum` - Async runtime and web server
- `ini` - INI parsing for the attribute mapping config

CLI crate (`note_search_cli`) additionally depends on `clap` and `tokio` directly, plus `note_search_core`.

## Workflow Example

1. **Import your notes**:

    ``` bash
    note_search import --input ~/notes
    ```

2. **Search for open todos**:

    ``` bash
    note_search todos --open
    ```

3. **Find high priority todo items**:

    ``` bash
    note_search todos --priority A --open
    ```

4. **Search todos by tags**:

    ``` bash
    note_search todos --tags urgent,bug --open
    ```

5. **Find recent todos**:

    ``` bash
    note_search todos --date-range this_week --open
    ```

6. **Search for notes by tags** (searches in frontmatter):

    ``` bash
    note_search notes --tags project,active
    ```

7. **Find notes created recently**:

    ``` bash
    note_search notes --date-range this_month
    ```

8. **Re-import after making changes** (updates existing entries):

    ``` bash
    note_search import --input ~/notes
    ```

9. **Import JIRA issues** (requires JIRA_SERVER and JIRA_API_TOKEN env vars):

    ``` bash
    note_search jira "assignee = currentUser() AND status != Done"
    ```

### Manual Page

A comprehensive manual page is available in `note_search_cli/man/note_search.1`. To view it:

``` bash
man note_search_cli/man/note_search.1
```

To install the man page system-wide:

``` bash
sudo cp note_search_cli/man/note_search.1 /usr/local/share/man/man1/
sudo mandb  # Update man database
```

### Shell Completions

#### Bash

Bash completion script is available in `note_search_cli/completions/note_search.bash`. To install:

**Option 1: System-wide installation (Linux)**

``` bash
sudo cp note_search_cli/completions/note_search.bash /etc/bash_completion.d/note_search
```

**Option 2: User installation**

``` bash
# Add to your ~/.bashrc:
source /path/to/note_search_cli/completions/note_search.bash
```

**Option 3: Homebrew (macOS)**

``` bash
cp note_search_cli/completions/note_search.bash $(brew --prefix)/etc/bash_completion.d/
```

After installation, restart your shell or run:

``` bash
source ~/.bashrc  # or source your config file
```

#### Zsh

Zsh completion script is available in `note_search_cli/completions/_note_search`. To install:

**Option 1: System-wide installation**

``` bash
sudo cp note_search_cli/completions/_note_search /usr/local/share/zsh/site-functions/
# Or copy to your fpath directory
```

**Option 2: User installation**

``` bash
mkdir -p ~/.zsh/completions
cp note_search_cli/completions/_note_search ~/.zsh/completions/
# Add to your ~/.zshrc:
# fpath+=(~/.zsh/completions)
```

**Option 3: Direct sourcing in .zshrc**

``` bash
# Add to your ~/.zshrc:
source /path/to/note_search_cli/completions/_note_search
```

After installation, restart zsh or run:

``` bash
exec zsh
```

#### Available Completions

Both completion scripts provide:

- Tab completion for: `todos`, `notes`, `import`, `clear`, `values`, `attributes`, `backlinks`, `help` (newer commands — `agenda`, `convert`, `linker`, `jira`, `jira-issue`, `browser-history`, `web`, `list-names`, `info`, `create-note` — aren't wired into the completion scripts yet)
- Smart completions for subcommand-specific options:
  - `--sort` suggests relevant fields (e.g., `due_date` and `priority` only for todos)
  - `--date-range` suggests all date range options (today, yesterday, this_week, etc.)
  - `--priority` suggests A, B, C for todos
- Date suggestions in YYYYMMDD format for date options
- Directory completion for `--input`
- File completion for `--database` (*.sqlite) and markdown files
- Attribute completion suggestions for `--attributes`
