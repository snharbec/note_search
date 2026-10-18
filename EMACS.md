# Emacs Integration

`emacs/note-search.el` is the Emacs port of the Neovim plugin in `lua/note_search`. It calls the
`note_search` binary and uses [consult](https://github.com/minad/consult) for pickers with live
preview. Evil is optional (keys below assume Evil with `SPC` leader and `,` local leader).

## Installation

Emacs 30+ can install it straight from GitHub:

```elisp
(use-package note-search
  :vc (:url "https://github.com/snharbec/note_search" :lisp-dir "emacs" :rev :newest)
  :demand t
  :config (note-search-mode 1))
```

Update with `M-x package-vc-upgrade`. `:rev :newest` takes the latest commit (the repo has no
release tags). From a local checkout instead, replace `:vc ...` with
`:ensure nil :load-path "/path/to/note_search/emacs"`.

`note-search-mode` turns on `note-search-markdown-mode` in every markdown buffer. Bind the commands
yourself (see the suggested keys below).

The notes directory is `note-search-notes-directory`, else `$NOTE_SEARCH_DIR`, else
`~/.local/share/notes`. The database is `note-search-database`, else whatever `note_search` uses
(`$NOTE_SEARCH_DATABASE`). On macOS GUI Emacs, copy these variables from the shell with
`exec-path-from-shell`.

## Commands and suggested keys

These mirror the Neovim plugin's `<leader>n` keys.

| Key | Command | What it does |
| --- | --- | --- |
| `SPC n t` | `note-search-daily` | Today's daily note |
| `SPC n T` | `note-search-day-note` | Daily note for a date |
| `SPC n n` | `note-search-find-note` | Find any note |
| `SPC n N c/p/e/d/t` | `note-search-find-company` ... | Find a company/project/person/department/daily note |
| `SPC n P/E/C/R` | `note-search-create-project` ... | Open a project/person/company/department, or create it (`M-e` takes the typed name) |
| `SPC n p/e/c/r` | `note-search-project-note` ... | New Meeting/Note/Task note for an entity |
| `SPC n s a` | `note-search-interactive-search` | Attribute → value → optional text |
| `SPC n s .` | `note-search-repeat-search` | Repeat last attribute search |
| `SPC n s s` | `note-search-find-by-attributes` | One or more attribute → value filters → open a matching note |
| `SPC n s t` | `note-search-search-by-tag` | Notes with a tag |
| `SPC n s l` | `note-search-live-search` | Search as you type |
| `SPC n L` | `note-search-list` | All notes, newest first; narrow with `@link #tag word` (all must match) |
| `SPC n S` | `note-search-segments` | Same for heading sections (segments); opens at the heading |
| `SPC n s b` | `note-search-search-body` | Text in note bodies |
| `SPC n s B` | `note-search-search-backlinks` | Notes linking to the `[[link]]` at point |
| `SPC n s R` | `note-search-references` | Notes linking to the current note |
| `SPC n s x` | `note-search-interactive-todo` | Open todos by attribute → value → text |
| `SPC n s r/d/w/4` | `note-search-search-recent` ... | Notes created recently / today / this week / last 4 weeks |
| `SPC n s f` | `note-search-search-files` | Find a file in the notes directory |
| `SPC n l` | `note-search-replace-links` | Turn note names in the buffer into `[[links]]` |
| `SPC n A` | `note-search-agenda` | Agenda buffer (`C-u` hides the summary) |
| `SPC n a` | `note-search-agenda-current` | Agenda for the current note |
| `SPC n g` | `note-search-agenda-jump` | Agenda: open the todo on this line |
| `SPC n D` | `note-search-agenda-done` | Agenda: mark the todo on this line done |
| `SPC n q` | `note-search-quick-capture` | Capture buffer; saving (`:w`, `C-s`, `C-c C-c`) appends it to today's daily note |
| `SPC n j` | `note-search-document-to-link` | URL or JIRA key at point → download as note, replace with `[[link]]` |
| `SPC n x` | `note-search-exec-toggle` | Run ```` ```exec ```` blocks, or remove their output |

In `note-search-list` and `note-search-segments`, typing after `@` completes note names and typing
after `#` completes tags (with Corfu the popup appears on its own; otherwise press `C-M-i`). A
name that `@` cannot hold, such as one with spaces, is inserted as `[[name]]`.

Without a default key: `note-search-backlink-search` (attribute → value → note → notes linking to
it) and `note-search-browser-history` (import browser history).

### Inserting links (markdown buffers)

| Normal (`, n …`) | Insert (`/…`) | Inserts a link to |
| --- | --- | --- |
| `, n p` | `/p` | a project |
| `, n e` | `/e` | a person |
| `, n c` | `/c` | a company |
| `, n D` | `/D` | a department |
| `, n t` | `/t` | a daily note |
| `, n n` | `/n` | any note |
| `, n d` | `/d` | a note changed today |
| `, n w` | `/w` | a note changed this week |
| `, n 4` | `/4` | a note changed in the last 4 weeks |
| `, n f` | `/f` | the file a typed note was started from |
| `, n b` | `/b` | the lines selected when a typed note was started (as a code block) |

In insert state, `/` waits `note-search-insert-timeout` (0.6 s) for the next key; any other key
(or no key) types a plain `/`. Typing `@` followed by part of a note name offers completion that
inserts `[[name]]`.

### On save

- Generated exec output is removed before saving (as in Neovim).
- An `updated:` line in the YAML frontmatter is set to the current time
  (`note-search-frontmatter-update`, `note-search-frontmatter-format`).

## Options

`M-x customize-group RET note-search` lists them: note types and subtypes, templates directory,
date format, `fd` command, exec auto-run (`note-search-exec-auto-execute`, off by default), the
insert trigger key and its key table.

## Tests

```bash
emacs -Q --batch --eval '(package-initialize)' -L emacs -l emacs/note-search-test.el -f ert-run-tests-batch-and-exit
```
