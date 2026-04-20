# Neovim Snacks Picker Integration

This module provides a Neovim integration for the `note_search` tool using the Snacks picker.

## Installation

### Using lazy.nvim

```lua
{
  dir = "/path/to/note_search",  -- Local path to this repo
  dependencies = {
    "folke/snacks.nvim",  -- Required for the picker
  },
  config = function()
    require("note_search").setup({
      note_search_cmd = "note_search",  -- Path to binary
      database_path = "~/notes/note.sqlite",  -- Optional
      note_dir = "~/notes",  -- Optional
    })

    -- Keymaps
    vim.keymap.set("n", "<leader>ns", require("note_search").interactive_search, 
      { desc = "Note Search: Select attribute → value → text" })
    
    vim.keymap.set("n", "<leader>nt", require("note_search").search_by_tag, 
      { desc = "Note Search by Tag" })
    
    vim.keymap.set("n", "<leader>nl", require("note_search").live_search, 
      { desc = "Note Search (Live)" })
    
    vim.keymap.set("n", "<leader>nr", require("note_search").repeat_interactive_search, 
      { desc = "Repeat Last Note Search" })
    
    vim.keymap.set("n", "<leader>nd", function()
      require("note_search").search_notes({ search_body = vim.fn.input("Search in body: ") })
    end, { desc = "Note Search in Body" })
  end,
}
```

### Manual Installation

1. Copy or symlink the `lua/note_search` directory to your Neovim config:
   ```bash
   ln -s /path/to/note_search/lua/note_search ~/.config/nvim/lua/note_search
   ```

2. Add to your config:
   ```lua
   require("note_search").setup({
     note_search_cmd = "note_search",
   })
   ```

## Features

### 1. Interactive Search (`interactive_search()`)

A three-step picker:
1. **Select attribute** (e.g., `project`, `type`, `tags`)
2. **Select value** for that attribute (e.g., `project=NeoVimNote`)
3. **Optional text search** within those results

### 2. Repeat Last Search (`repeat_interactive_search()`)

Quickly re-run your last interactive search with the same attribute, value, and text filter. Useful when you want to go back to your previous search results without navigating through pickers again.

### 3. Tag Search (`search_by_tag()`)

Quick picker to select a tag and see all notes with that tag. Tags are now aggregated from both note frontmatter and todo entries within notes, so searching by tag will find:
- Notes that have the tag in their YAML frontmatter
- Notes that contain todos with that tag

### 4. Live Search (`live_search()`)

Real-time search as you type, with results updating dynamically.

### 5. Backlink Search (`search_backlinks()`)

With the cursor on a `[[link]]` (or any word), searches for all notes that reference that link. Uses the `note_search` CLI `--links` filter.

- If the cursor is inside `[[NeoVimNote]]`, searches for all notes linking to `NeoVimNote`
- Falls back to the word under the cursor if no wiki link is found
- Strips `.md` suffix from the link name automatically

`:NoteSearchBacklinks` command or `<leader>nsB` keymap.

### 6. Direct Search (`search_notes(opts)`)

Programmatic search with options:

```lua
require("note_search").search_notes({
  tags = "project,discuss",
  attributes = "project=NeoVimNote",
  text = "picker",
  search_body = true,  -- Search in note body
  date_range = "this_week",
})
```

### 7. Todo Search (`search_todos(opts)`)

Search todo entries:

```lua
require("note_search").search_todos({
  priority = "A",
  open = true,
  tags = "urgent",
})
```

## API Reference

### Setup Options

- `note_search_cmd`: Path to note_search binary (default: `"note_search"`)
- `database_path`: Path to SQLite database (optional, uses env var or default)
- `note_dir`: Path to notes directory (optional, uses env var or default)

### Functions

- `setup(opts)` - Configure the module
- `search_notes(opts)` - Search notes with filters (automatically uses absolute paths)
- `search_todos(opts)` - Search todos with filters (automatically uses absolute paths)
- `interactive_search()` - Multi-step attribute picker
- `repeat_interactive_search()` - Re-run last interactive search with same parameters
- `search_by_tag()` - Tag selection picker
- `search_backlinks()` - Search backlinks for link under cursor
- `live_search()` - Real-time search picker
- `get_tags()` - Returns list of all tags
- `get_links()` - Returns list of all links
- `get_attribute_names()` - Returns list of all attribute names
- `get_attribute_values(attr_name)` - Returns values for a specific attribute

## Examples

### Search for project notes created this week

```lua
require("note_search").search_notes({
  attributes = "type=project",
  date_range = "this_week",
})
```

### Search meeting notes with specific person

```lua
require("note_search").search_notes({
  attributes = "type=meeting",
  text = "John",
})
```

### Find open high-priority todos

```lua
require("note_search").search_todos({
  priority = "A",
  open = true,
})
```

## CLI Commands

The following `note_search` CLI commands are used by this module:

- `note_search notes` - Search notes
- `note_search todos` - Search todos
- `note_search values <field>` - Get unique values
- `note_search attributes` - Get all attribute names
- `note_search backlinks <filename>` - Get backlinks

See the main README for full CLI documentation.
