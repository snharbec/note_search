# Lua note_search plugin

A Neovim plugin for organizing notes by entity types: People, Projects, Companies, and Departments.

## Features

- **Entity-based organization**: Create and manage notes for People, Projects, Companies, and Departments
- **Daily journals**: Automatic daily note creation with date-based folder structure
- **Quick search and creation**: Fuzzy search to find or create new entities
- **Smart link insertion**: Insert wiki-style links to entities directly while typing
- **Template support**: Customizable templates for different note types
- **Subtypes**: Create meeting notes, task notes, and general notes for each entity
- **Backlinks**: Find all notes that reference the current note
- **Flexible search**: Multiple search methods including tag, body, and TODO search

## Installation

``` lua
-- Using lazy.nvim
{
  "snharbec/note_search",
  lazy = false,
  ft = "markdown",
  dependencies = {
    "folke/snacks.nvim",
  },
  opts = {
    notes_dir = vim.fn.expand(vim.env.NOTE_SEARCH_DIR or "~/.local/share/notes"),
    templates_dir = vim.fn.expand(vim.env.NOTE_SEARCH_DIR or "~/.local/share/notes") .. "/templates",
    find_command = "fd",
    keymap_group = "<leader>n",
    insert_group = "/",
  },
}
```

**Required dependencies:**

- `folke/snacks.nvim` - Picker interface
- `fd` or `fzf` - File search (fd recommended)

## Configuration

``` lua
require("note-type").setup({
  -- Directories
  notes_dir = "~/.local/share/notes",
  templates_dir = "~/.local/share/notes/templates",
  
  -- Keybindings
  keymap_group = "<leader>n",  -- Normal mode prefix
  insert_group = "/",          -- Insert mode trigger
  
  -- Search
  find_command = "fd",         -- Or "fzf"
  
  -- Note types
  types = {
    daily = {
      name = "daily",
      dir = "daily",
      subdir = "%Y/%b",
      template = "daily.md",
      day_prefix = true,
      create = "t",
      link = "t",
    },
    note = {
      name = "note",
      dir = "note",
      template = "note.md",
      create = "n",
      link = "N",
    },
    person = {
      name = "person",
      dir = "person",
      template = "person.md",
      has_subtypes = true,
      create = "e",
      link = "p",
    },
    project = {
      name = "project",
      dir = "project",
      template = "project.md",
      has_subtypes = true,
      create = "p",
      link = "p",
    },
    company = {
      name = "company",
      dir = "company",
      template = "company.md",
      has_subtypes = true,
      create = "c",
      link = "c",
    },
    department = {
      name = "department",
      dir = "department",
      template = "department.md",
      has_subtypes = true,
      create = "a",
      link = "d",
    },
  },
  
  -- Subtypes for entities
  subtypes = {
    Meeting = {
      template = "meeting.md",
      add_type = true,
      day_prefix = true,
    },
    Note = {
      template = "note.md",
      add_type = true,
      day_prefix = true,
    },
    Task = {
      template = "task.md",
      add_type = true,
      day_prefix = false,
    },
  },
})
```

## Keybindings

### Normal Mode

**Open Notes:**

- `<leader>nt` - Open today's daily note
- `<leader>nn` - Open any note picker
- `<leader>Nnc` - Open company picker
- `<leader>Nnp` - Open project picker
- `<leader>Nne` - Open person picker
- `<leader>Nnd` - Open department picker
- `<leader>Nnt` - Open daily note picker

**Search and Create Entities:**

- `<leader>nP` - Project search/create (Alt-e to create new)
- `<leader>nE` - Person search/create (Alt-e to create new)
- `<leader>nC` - Company search/create (Alt-e to create new)
- `<leader>nA` - Department search/create (Alt-e to create new)

**Create Notes for Entities:**

- `<leader>np` - Create note for project
- `<leader>ne` - Create note for person
- `<leader>nc` - Create note for company
- `<leader>na` - Create note for department

**Advanced Search:**

- `<leader>nss` - Search by attributes
- `<leader>ns.` - Repeat last search
- `<leader>nst` - Search by tag
- `<leader>nsl` - Live fuzzy search
- `<leader>nst` - Search note body
- `<leader>nsx` - Search TODOs

### Insert Mode

**Link Insertion (press `.` then):**

- `.p` - Insert link to project `[[project_name]]`
- `.e` - Insert link to person `[[person_name]]`
- `.c` - Insert link to company `[[company_name]]`
- `.d` - Insert link to department `[[department_name]]`
- `.t` - Insert link to daily note
- `.f` - Insert link to last file
- `.b` - Insert selected text as code block

## Commands

```
:NoteType <type>              -- Search/create entity
:NoteTypeNote <type> [subtype] -- Create note for entity
:NoteTypeInsertLink <type>    -- Insert link to entity
:NoteTypeInsertFile           -- Insert link to last file
:NoteTypeInsertBlock          -- Insert selection as code block
:NoteBacklinks                -- Show notes linking to current file
```

## Note Organization

```
~/.local/share/notes/
├── daily/
│   └── 2026/
│       └── Apr/
│           └── 2026-04-05.md
├── note/
│   └── random_ideas.md
├── person/
│   └── john_doe/
│       ├── 2026-04-05-meeting.md
│       └── task-follow_up.md
├── project/
│   └── website_redesign/
│       └── 2026-04-05-meeting.md
├── company/
│   └── acme_corp/
│       └── 2026-04-05-note.md
├── department/
│   └── engineering/
│       └── 2026-04-05-meeting.md
└── templates/
    ├── daily.md
    ├── note.md
    ├── person.md
    ├── project.md
    ├── company.md
    ├── department.md
    ├── meeting.md
    └── task.md
```

## Templates

Use the following variables in your templates:

| Variable        | Description                        |
| --------------- | ---------------------------------- |
| `{{date}}`      | Full date: "Monday, April 5, 2026" |
| `{{time}}`      | Current time: "14:30:00"           |
| `{{today}}`     | Today's date: "2026-04-05"         |
| `{{tomorrow}}`  | Tomorrow's date: "2026-04-05"      |
| `{{yesterday}}` | Yesterday's date: "2026-04-05"     |
| `{{month}}`     | Month name: "April"                |
| `{{year}}`      | Year: "2026"                       |
| `{{title}}`     | Note title                         |
| `{{name}}`      | Lowercase name without spaces      |
| `{{type}}`      | Entity type                        |
| `{{ref}}`       | Entity reference with underscores  |
| `{{refname}}`   | Entity reference with spaces       |
| `{{cursor}}`    | Cursor position marker             |

## Quick Start

1. Install the plugin and dependencies
2. Create your templates directory
3. Press `<leader>nt` to open today's daily note
4. Press `<leader>nP` to search/create a project
5. Use `.` in insert mode to add links to entities

## Tips

- Use `:NoteBacklinks` to find related notes
- Customize `keymap_group` and `insert_group` to change keybindings
- Set `NOTE_SEARCH_DIR` environment variable to override note directories
