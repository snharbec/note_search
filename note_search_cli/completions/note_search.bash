#!/bin/bash
# Bash completion script for note_search
# 
# Installation:
#   1. Copy this file to /etc/bash_completion.d/note_search
#   2. Or source it in your ~/.bashrc: source /path/to/note_search.bash
#   3. On macOS with Homebrew: copy to $(brew --prefix)/etc/bash_completion.d/

_note_search() {
    local cur prev words cword
    _init_completion || return

    # Get the command or subcommand being completed
    local cmd="note_search"
    local subcmd=""
    
    # Check if we're completing a subcommand
    if [ ${#words[@]} -ge 3 ]; then
        subcmd="${words[2]}"
    fi

    # Main command options (only when no subcommand)
    if [ -z "$subcmd" ] || [ ${#words[@]} -eq 2 ]; then
        # Complete subcommands
        local subcommands="todos notes import clear values attributes backlinks help"
        COMPREPLY=( $(compgen -W "${subcommands}" -- "$cur") )
        
        # Also suggest global options
        local global_options="-d --database -h --help -V --version"
        COMPREPLY+=( $(compgen -W "${global_options}" -- "$cur") )
        
        return 0
    fi

    # Subcommand-specific completions
    case "$subcmd" in
        todos)
            _note_search_todos
            ;;
        notes)
            _note_search_notes
            ;;
        import)
            _note_search_import
            ;;
        clear)
            _note_search_clear
            ;;
        values)
            _note_search_values
            ;;
        attributes)
            _note_search_attributes
            ;;
        backlinks)
            _note_search_backlinks
            ;;
        help)
            _note_search_help
            ;;
        *)
            # Default: no completion
            ;;
    esac
}

_note_search_todos() {
    local options="
        --tags
        --links
        --attributes
        --text
        --search-body
        --priority
        --due-date
        --due-date-eq
        --due-date-gt
        --open
        --closed
        --date-range
        --start-date
        --end-date
        --format
        --sort
        --list
        --absolute-path
        -d
        --database
        -h
        --help
    "
    
    local cur="${COMP_WORDS[COMP_CWORD]}"
    local prev="${COMP_WORDS[COMP_CWORD-1]}"
    
    case "$prev" in
        --priority)
            # Suggest priority values
            COMPREPLY=( $(compgen -W "A B C" -- "$cur") )
            return 0
            ;;
        --date-range)
            # Suggest date range options
            local ranges="today yesterday this_week last_week this_month last_month this_year last_year"
            COMPREPLY=( $(compgen -W "$ranges" -- "$cur") )
            return 0
            ;;
        --start-date|--end-date|--due-date|--due-date-eq|--due-date-gt)
            # Suggest today's date
            local today=$(date +%Y%m%d)
            COMPREPLY=( $(compgen -W "$today" -- "$cur") )
            return 0
            ;;
        --sort)
            # Suggest sort fields
            local fields="due_date priority filename modified text attr:"
            COMPREPLY=( $(compgen -W "$fields" -- "$cur") )
            return 0
            ;;
        --tags)
            # Suggest common tags
            local tags="feature bug urgent documentation enhancement todo"
            COMPREPLY=( $(compgen -W "$tags" -- "$cur") )
            return 0
            ;;
        -d|--database)
            # File completion for database
            _filedir sqlite
            return 0
            ;;
    esac
    
    # Default: complete options
    COMPREPLY=( $(compgen -W "$options" -- "$cur") )
}

_note_search_notes() {
    local options="
        --tags
        --links
        --attributes
        --text
        --search-body
        --date-range
        --start-date
        --end-date
        --format
        --sort
        --list
        --absolute-path
        -d
        --database
        -h
        --help
    "
    
    local cur="${COMP_WORDS[COMP_CWORD]}"
    local prev="${COMP_WORDS[COMP_CWORD-1]}"
    
    case "$prev" in
        --date-range)
            # Suggest date range options
            local ranges="today yesterday this_week last_week this_month last_month this_year last_year"
            COMPREPLY=( $(compgen -W "$ranges" -- "$cur") )
            return 0
            ;;
        --start-date|--end-date)
            # Suggest today's date
            local today=$(date +%Y%m%d)
            COMPREPLY=( $(compgen -W "$today" -- "$cur") )
            return 0
            ;;
        --sort)
            # Suggest sort fields (notes cannot sort by due_date or priority)
            local fields="filename modified text attr:"
            COMPREPLY=( $(compgen -W "$fields" -- "$cur") )
            return 0
            ;;
        --tags)
            # Suggest common tags
            local tags="feature bug urgent documentation enhancement todo project active"
            COMPREPLY=( $(compgen -W "$tags" -- "$cur") )
            return 0
            ;;
        -d|--database)
            # File completion for database
            _filedir sqlite
            return 0
            ;;
    esac
    
    # Default: complete options
    COMPREPLY=( $(compgen -W "$options" -- "$cur") )
}

_note_search_import() {
    local options="
        -i
        --input
        -o
        --output
        --watch
        --interval
        -h
        --help
    "
    
    local cur="${COMP_WORDS[COMP_CWORD]}"
    local prev="${COMP_WORDS[COMP_CWORD-1]}"
    
    case "$prev" in
        -i|--input)
            # Directory completion
            _filedir -d
            return 0
            ;;
        -o|--output)
            # File completion for database
            _filedir sqlite
            return 0
            ;;
        --interval)
            # Suggest common intervals
            COMPREPLY=( $(compgen -W "10 30 60 300" -- "$cur") )
            return 0
            ;;
    esac
    
    COMPREPLY=( $(compgen -W "$options" -- "$cur") )
}

_note_search_clear() {
    local options="
        --yes
        -h
        --help
    "
    
    COMPREPLY=( $(compgen -W "$options" -- "$cur") )
}

_note_search_attributes() {
    local options="
        -d
        --database
        -h
        --help
    "
    
    local cur="${COMP_WORDS[COMP_CWORD]}"
    local prev="${COMP_WORDS[COMP_CWORD-1]}"
    
    case "$prev" in
        -d|--database)
            _filedir sqlite
            return 0
            ;;
    esac
    
    COMPREPLY=( $(compgen -W "$options" -- "$cur") )
}

_note_search_values() {
    local options="
        -d
        --database
        -h
        --help
    "
    
    local cur="${COMP_WORDS[COMP_CWORD]}"
    local prev="${COMP_WORDS[COMP_CWORD-1]}"
    
    # First positional argument: field
    if [ $COMP_CWORD -eq 3 ]; then
        local fields="priority due_date tag link attr:"
        COMPREPLY=( $(compgen -W "$fields" -- "$cur") )
        return 0
    fi
    
    case "$prev" in
        -d|--database)
            _filedir sqlite
            return 0
            ;;
    esac
    
    COMPREPLY=( $(compgen -W "$options" -- "$cur") )
}

_note_search_backlinks() {
    local options="
        -d
        --database
        -h
        --help
    "
    
    local cur="${COMP_WORDS[COMP_CWORD]}"
    local prev="${COMP_WORDS[COMP_CWORD-1]}"
    
    # First positional argument: filename
    if [ $COMP_CWORD -eq 3 ]; then
        _filedir md
        return 0
    fi
    
    case "$prev" in
        -d|--database)
            _filedir sqlite
            return 0
            ;;
    esac
    
    COMPREPLY=( $(compgen -W "$options" -- "$cur") )
}

_note_search_help() {
    local commands="todos notes import clear values attributes backlinks"
    COMPREPLY=( $(compgen -W "$commands" -- "$cur") )
}

# Register the completion function
complete -F _note_search note_search
