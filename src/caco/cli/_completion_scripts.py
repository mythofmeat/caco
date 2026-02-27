"""Hand-crafted shell completion scripts for caco.

Each script uses `caco _complete <context>` for dynamic data (WADs, tags,
IWADs, sourceports, etc.) rather than Click's generic completion mechanism.
"""

FISH_SCRIPT = r"""# Fish completions for caco

# Disable file completions by default
complete -c caco -f

# Helper functions using caco _complete for fast, purpose-built data
function __caco_wads
    caco _complete wads 2>/dev/null
end

function __caco_tags
    caco _complete tags 2>/dev/null
end

function __caco_iwads
    caco _complete iwads 2>/dev/null
end

function __caco_sourceports
    caco _complete sourceports 2>/dev/null
end

# Global options
complete -c caco -n __fish_use_subcommand -l tui -d "Launch TUI interface"
complete -c caco -n __fish_use_subcommand -l gui -d "Launch GUI interface (requires PySide6)"

# Main commands
complete -c caco -n __fish_use_subcommand -a ls -d "List WADs in your library"
complete -c caco -n __fish_use_subcommand -a info -d "Show details about a WAD"
complete -c caco -n __fish_use_subcommand -a modify -d "Modify WAD metadata"
complete -c caco -n __fish_use_subcommand -a trash -d "Manage trash and removals"
complete -c caco -n __fish_use_subcommand -a play -d "Play a WAD"
complete -c caco -n __fish_use_subcommand -a import -d "Import WADs from various sources"
complete -c caco -n __fish_use_subcommand -a config -d "View or edit configuration"
complete -c caco -n __fish_use_subcommand -a random -d "Pick a random WAD (prints ID)"
complete -c caco -n __fish_use_subcommand -a completions -d "Generate shell completions"
complete -c caco -n __fish_use_subcommand -a stats -d "Show library statistics"
complete -c caco -n __fish_use_subcommand -a cache -d "Manage WAD file cache"

# =============================================================================
# ls command
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from ls" -s o -l output -d "Output format" -xa "json plain"
complete -c caco -n "__fish_seen_subcommand_from ls" -l tags -d "List all tags with counts"
complete -c caco -n "__fish_seen_subcommand_from ls" -l iwad -d "List registered IWADs"

# ls query field completions
complete -c caco -n "__fish_seen_subcommand_from ls" -a "id:" -d "Filter by ID"
complete -c caco -n "__fish_seen_subcommand_from ls" -a "title:" -d "Filter by title"
complete -c caco -n "__fish_seen_subcommand_from ls" -a "author:" -d "Filter by author"
complete -c caco -n "__fish_seen_subcommand_from ls" -a "year:" -d "Filter by year"
complete -c caco -n "__fish_seen_subcommand_from ls" -a "filename:" -d "Filter by filename"
complete -c caco -n "__fish_seen_subcommand_from ls" -a "tag:" -d "Filter by tag"
complete -c caco -n "__fish_seen_subcommand_from ls" -a "status:" -d "Filter by status"
complete -c caco -n "__fish_seen_subcommand_from ls" -a "source:" -d "Filter by source"
complete -c caco -n "__fish_seen_subcommand_from ls" -a "iwad:" -d "Filter by IWAD"

# ls inline sort completions
complete -c caco -n "__fish_seen_subcommand_from ls" -a "id+ id- playtime+ playtime- rating+ rating- created+ created- title+ title- author+ author- last_played+ last_played- year+ year-" -d "Sort"

# =============================================================================
# info command
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from info" -s o -l output -d "Output format" -xa "json plain"
complete -c caco -n "__fish_seen_subcommand_from info" -l levelstats -d "Show per-map statistics"
complete -c caco -n "__fish_seen_subcommand_from info" -s b -d "Target completion by timestamp"
complete -c caco -n "__fish_seen_subcommand_from info" -l live -d "Show only live stats"
complete -c caco -n "__fish_seen_subcommand_from info" -l plain -d "TSV output for stats"
complete -c caco -n "__fish_seen_subcommand_from info" -xa "(__caco_wads)"
complete -c caco -n "__fish_seen_subcommand_from info" -a "id:" -d "Filter by ID"
complete -c caco -n "__fish_seen_subcommand_from info" -a "title:" -d "Filter by title"
complete -c caco -n "__fish_seen_subcommand_from info" -a "author:" -d "Filter by author"
complete -c caco -n "__fish_seen_subcommand_from info" -a "tag:" -d "Filter by tag"
complete -c caco -n "__fish_seen_subcommand_from info" -a "status:" -d "Filter by status"

# =============================================================================
# modify command
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from modify" -s y -l yes -d "Skip confirmation"
complete -c caco -n "__fish_seen_subcommand_from modify" -l dry-run -d "Preview changes"
complete -c caco -n "__fish_seen_subcommand_from modify" -l link -d "Link a local file" -rF
complete -c caco -n "__fish_seen_subcommand_from modify" -l notes -d "Notes for beaten+N"
complete -c caco -n "__fish_seen_subcommand_from modify" -s s -l stats-file -d "Stats file for beaten+N or attach" -rF
complete -c caco -n "__fish_seen_subcommand_from modify" -l date -d "Backdate completion (ISO)"
complete -c caco -n "__fish_seen_subcommand_from modify" -s b -d "Target completion by timestamp"
complete -c caco -n "__fish_seen_subcommand_from modify" -xa "(__caco_wads)"

# modify field=value completions
complete -c caco -n "__fish_seen_subcommand_from modify" -a "status=" -d "Set status"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "rating=" -d "Set rating (1-5)"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "title=" -d "Set title"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "author=" -d "Set author"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "year=" -d "Set year"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "notes=" -d "Set notes"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "tag=" -d "Add tag"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "iwad=" -d "Set custom IWAD"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "sourceport=" -d "Set custom sourceport"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "idgames-id=" -d "Set idgames ID"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "description=" -d "Set description"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "args=" -d "Set custom args"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "version=" -d "Set version"

# modify clear completions
complete -c caco -n "__fish_seen_subcommand_from modify" -a "!author" -d "Clear author"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "!year" -d "Clear year"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "!description" -d "Clear description"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "!notes" -d "Clear notes"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "!rating" -d "Clear rating"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "!iwad" -d "Clear custom IWAD"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "!sourceport" -d "Clear custom sourceport"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "!tag" -d "Remove all tags"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "!tag:" -d "Remove tags matching pattern"

# modify beaten completions
complete -c caco -n "__fish_seen_subcommand_from modify" -a "beaten+" -d "Add completion(s)"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "beaten-" -d "Remove completion(s)"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "beaten=" -d "Set completion count"

# modify query fields
complete -c caco -n "__fish_seen_subcommand_from modify" -a "id:" -d "Filter by ID"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "title:" -d "Filter by title"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "author:" -d "Filter by author"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "tag:" -d "Filter by tag"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "status:" -d "Filter by status"

# =============================================================================
# trash command
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from trash" -l list -d "Show trashed WADs"
complete -c caco -n "__fish_seen_subcommand_from trash" -l purge -d "Permanently delete"
complete -c caco -n "__fish_seen_subcommand_from trash" -l restore -d "Restore from trash"
complete -c caco -n "__fish_seen_subcommand_from trash" -l iwad -d "Remove IWAD (FAMILY or FAMILY/VARIANT)" -xa "(__caco_iwads)"
complete -c caco -n "__fish_seen_subcommand_from trash" -s y -l yes -d "Skip confirmation"
complete -c caco -n "__fish_seen_subcommand_from trash" -l dry-run -d "Preview changes"
complete -c caco -n "__fish_seen_subcommand_from trash" -s o -l output -d "Output format (with --list)" -xa "json plain"
complete -c caco -n "__fish_seen_subcommand_from trash" -xa "(__caco_wads)"
complete -c caco -n "__fish_seen_subcommand_from trash" -a "id:" -d "Filter by ID"
complete -c caco -n "__fish_seen_subcommand_from trash" -a "title:" -d "Filter by title"
complete -c caco -n "__fish_seen_subcommand_from trash" -a "status:" -d "Filter by status"

# =============================================================================
# play command
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from play" -s p -l sourceport -d "Sourceport to use" -xa "(__caco_sourceports)"
complete -c caco -n "__fish_seen_subcommand_from play" -s 1 -l first -d "Auto-select first match"
complete -c caco -n "__fish_seen_subcommand_from play" -l iwad -d "Play IWAD directly (e.g., doom2)" -xa "(__caco_iwads)"
complete -c caco -n "__fish_seen_subcommand_from play" -xa "(__caco_wads)"
complete -c caco -n "__fish_seen_subcommand_from play" -a "id:" -d "Filter by ID"
complete -c caco -n "__fish_seen_subcommand_from play" -a "title:" -d "Filter by title"
complete -c caco -n "__fish_seen_subcommand_from play" -a "author:" -d "Filter by author"
complete -c caco -n "__fish_seen_subcommand_from play" -a "tag:" -d "Filter by tag"
complete -c caco -n "__fish_seen_subcommand_from play" -a "status:" -d "Filter by status"

# =============================================================================
# import command
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from import" -l idgames -d "Force idgames source"
complete -c caco -n "__fish_seen_subcommand_from import" -l doomwiki -d "Force Doom Wiki source"
complete -c caco -n "__fish_seen_subcommand_from import" -l doomworld -d "Force Doomworld forum source"
complete -c caco -n "__fish_seen_subcommand_from import" -l local -d "Force local file import"
complete -c caco -n "__fish_seen_subcommand_from import" -l url -d "Import from URL (value is download URL)"
complete -c caco -n "__fish_seen_subcommand_from import" -s t -l title -d "Override title"
complete -c caco -n "__fish_seen_subcommand_from import" -s a -l author -d "Author name"
complete -c caco -n "__fish_seen_subcommand_from import" -l year -d "Year released"
complete -c caco -n "__fish_seen_subcommand_from import" -l tag -d "Add tag" -xa "(__caco_tags)"
complete -c caco -n "__fish_seen_subcommand_from import" -s f -l force -d "Import even if duplicate exists"
complete -c caco -n "__fish_seen_subcommand_from import" -s m -l multi -d "Allow multi-select (requires fzf)"
complete -c caco -n "__fish_seen_subcommand_from import" -s d -l description -d "Description (for --url imports)"
complete -c caco -n "__fish_seen_subcommand_from import" -s s -l smart -d "Use LLM for metadata extraction"
complete -c caco -n "__fish_seen_subcommand_from import" -l llm-backend -d "LLM backend" -xa "claude-code openrouter anthropic openai"
complete -c caco -n "__fish_seen_subcommand_from import" -l llm-model -d "Model override for API backends"

# =============================================================================
# config command
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from config" -s e -l edit -d "Open config in editor"

# =============================================================================
# completions command
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from completions" -a "bash fish zsh" -d "Shell type"
complete -c caco -n "__fish_seen_subcommand_from completions" -l install -d "Install completions to config"

# =============================================================================
# random command
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from random" -l info -d "Print ID, title, and author"
complete -c caco -n "__fish_seen_subcommand_from random" -a "id:" -d "Filter by ID"
complete -c caco -n "__fish_seen_subcommand_from random" -a "title:" -d "Filter by title"
complete -c caco -n "__fish_seen_subcommand_from random" -a "author:" -d "Filter by author"
complete -c caco -n "__fish_seen_subcommand_from random" -a "status:" -d "Filter by status"
complete -c caco -n "__fish_seen_subcommand_from random" -a "tag:" -d "Filter by tag"
complete -c caco -n "__fish_seen_subcommand_from random" -a "source:" -d "Filter by source"

# =============================================================================
# stats command
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from stats" -s p -l period -d "Group by period" -xa "month year"
complete -c caco -n "__fish_seen_subcommand_from stats" -s n -l limit -d "Number of periods"
complete -c caco -n "__fish_seen_subcommand_from stats" -l plain -d "Key=value output"

# =============================================================================
# cache subcommands
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from cache; and not __fish_seen_subcommand_from list clear prune" -a list -d "List cached files"
complete -c caco -n "__fish_seen_subcommand_from cache; and not __fish_seen_subcommand_from list clear prune" -a clear -d "Remove cached files"
complete -c caco -n "__fish_seen_subcommand_from cache; and not __fish_seen_subcommand_from list clear prune" -a prune -d "Remove orphaned files"

complete -c caco -n "__fish_seen_subcommand_from cache; and __fish_seen_subcommand_from list" -l plain -d "Output as TSV"
complete -c caco -n "__fish_seen_subcommand_from cache; and __fish_seen_subcommand_from list" -l orphans -d "Show orphaned files"
complete -c caco -n "__fish_seen_subcommand_from cache; and __fish_seen_subcommand_from clear" -l all -d "Clear entire cache"
complete -c caco -n "__fish_seen_subcommand_from cache; and __fish_seen_subcommand_from clear" -l dry-run -d "Show what would be deleted"
complete -c caco -n "__fish_seen_subcommand_from cache; and __fish_seen_subcommand_from clear" -s y -l yes -d "Skip confirmation"
complete -c caco -n "__fish_seen_subcommand_from cache; and __fish_seen_subcommand_from prune" -l dry-run -d "Show what would be deleted"
complete -c caco -n "__fish_seen_subcommand_from cache; and __fish_seen_subcommand_from prune" -s y -l yes -d "Skip confirmation"

"""

BASH_SCRIPT = r"""# Bash completions for caco
# Install: caco completions --install
# Or: source <(caco completions bash)

# ---------------------------------------------------------------------------
# Dynamic data helpers (call caco _complete for live data)
# ---------------------------------------------------------------------------

_caco_wads() {
    local IFS=$'\n'
    COMPREPLY+=($(compgen -W "$(caco _complete wads 2>/dev/null | cut -f1)" -- "$cur"))
}

_caco_tags() {
    local IFS=$'\n'
    COMPREPLY+=($(compgen -W "$(caco _complete tags 2>/dev/null)" -- "$cur"))
}

_caco_iwads() {
    local IFS=$'\n'
    COMPREPLY+=($(compgen -W "$(caco _complete iwads 2>/dev/null)" -- "$cur"))
}

_caco_sourceports() {
    local IFS=$'\n'
    COMPREPLY+=($(compgen -W "$(caco _complete sourceports 2>/dev/null)" -- "$cur"))
}

_caco_query_fields() {
    COMPREPLY+=($(compgen -W "id: title: author: year: filename: tag: status: source: iwad:" -- "$cur"))
}

_caco_sort_fields() {
    local IFS=$'\n'
    COMPREPLY+=($(compgen -W "$(caco _complete sort-fields 2>/dev/null)" -- "$cur"))
}

_caco_modify_fields() {
    local IFS=$'\n'
    COMPREPLY+=($(compgen -W "$(caco _complete modify-fields 2>/dev/null)" -- "$cur"))
}

_caco_filedir() {
    if type _filedir &>/dev/null; then _filedir; else COMPREPLY=($(compgen -f -- "$cur")); fi
}

# ---------------------------------------------------------------------------
# Main completion function
# ---------------------------------------------------------------------------

_caco() {
    local cur prev words cword
    if type _init_completion &>/dev/null; then
        _init_completion || return
    else
        COMPREPLY=()
        cur="${COMP_WORDS[COMP_CWORD]}"
        prev="${COMP_WORDS[COMP_CWORD-1]}"
        words=("${COMP_WORDS[@]}")
        cword=$COMP_CWORD
    fi

    # Find the subcommand and sub-subcommand
    local cmd="" subcmd=""
    local i
    for ((i = 1; i < cword; i++)); do
        case "${words[i]}" in
            -*)
                # Skip options that take arguments
                case "${words[i]}" in
                    -o|--output|-p|--sourceport|--period|-n|--limit|--notes|\
                    --iwad|--tag|--url|--llm-backend|--llm-model|\
                    -t|--title|-a|--author|--year|-d|--description|\
                    -s|--stats-file|--link)
                        ((i++))
                        ;;
                esac
                ;;
            *)
                if [[ -z "$cmd" ]]; then
                    cmd="${words[i]}"
                elif [[ -z "$subcmd" ]]; then
                    subcmd="${words[i]}"
                fi
                ;;
        esac
    done

    # Top-level: complete commands or global options
    if [[ -z "$cmd" ]]; then
        if [[ "$cur" == -* ]]; then
            COMPREPLY=($(compgen -W "--tui --gui --help" -- "$cur"))
        else
            COMPREPLY=($(compgen -W "ls info modify trash play import config random completions stats cache" -- "$cur"))
        fi
        return
    fi

    case "$cmd" in
        ls)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-o --output --tags --iwad --help" -- "$cur"))
            elif [[ "$prev" == -o || "$prev" == --output ]]; then
                COMPREPLY=($(compgen -W "json plain" -- "$cur"))
            else
                _caco_query_fields
                _caco_sort_fields
            fi
            ;;
        info)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-o --output --levelstats -b --live --plain --help" -- "$cur"))
            elif [[ "$prev" == -o || "$prev" == --output ]]; then
                COMPREPLY=($(compgen -W "json plain" -- "$cur"))
            else
                _caco_wads
                _caco_query_fields
            fi
            ;;
        modify)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-y --yes --dry-run --link --notes -s --stats-file --date -b --help" -- "$cur"))
            elif [[ "$prev" == --link || "$prev" == -s || "$prev" == --stats-file ]]; then
                _caco_filedir
            else
                _caco_wads
                _caco_query_fields
                _caco_modify_fields
            fi
            ;;
        trash)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "--list --purge --restore --iwad -y --yes --dry-run -o --output --help" -- "$cur"))
            elif [[ "$prev" == --iwad ]]; then
                _caco_iwads
            elif [[ "$prev" == -o || "$prev" == --output ]]; then
                COMPREPLY=($(compgen -W "json plain" -- "$cur"))
            else
                _caco_wads
                _caco_query_fields
            fi
            ;;
        play)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-p --sourceport -1 --first --iwad --help" -- "$cur"))
            elif [[ "$prev" == -p || "$prev" == --sourceport ]]; then
                _caco_sourceports
            elif [[ "$prev" == --iwad ]]; then
                _caco_iwads
            else
                _caco_wads
                _caco_query_fields
            fi
            ;;
        import)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "--idgames --doomwiki --doomworld --local --url -t --title -a --author --year --tag -f --force -m --multi -d --description -s --smart --llm-backend --llm-model --help" -- "$cur"))
            elif [[ "$prev" == --tag ]]; then
                _caco_tags
            elif [[ "$prev" == --llm-backend ]]; then
                COMPREPLY=($(compgen -W "claude-code openrouter anthropic openai" -- "$cur"))
            fi
            ;;
        config)
            COMPREPLY=($(compgen -W "-e --edit --help" -- "$cur"))
            ;;
        completions)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "--install --help" -- "$cur"))
            else
                COMPREPLY=($(compgen -W "bash fish zsh" -- "$cur"))
            fi
            ;;
        random)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "--info --help" -- "$cur"))
            else
                _caco_query_fields
            fi
            ;;
        stats)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-p --period -n --limit --plain --help" -- "$cur"))
            elif [[ "$prev" == -p || "$prev" == --period ]]; then
                COMPREPLY=($(compgen -W "month year" -- "$cur"))
            fi
            ;;
        cache)
            if [[ -z "$subcmd" ]]; then
                COMPREPLY=($(compgen -W "list clear prune" -- "$cur"))
            else
                case "$subcmd" in
                    list)
                        COMPREPLY=($(compgen -W "--plain --orphans --help" -- "$cur"))
                        ;;
                    clear)
                        COMPREPLY=($(compgen -W "--all --dry-run -y --yes --help" -- "$cur"))
                        ;;
                    prune)
                        COMPREPLY=($(compgen -W "--dry-run -y --yes --help" -- "$cur"))
                        ;;
                esac
            fi
            ;;
    esac
}

complete -o default -F _caco caco
"""

ZSH_SCRIPT = r"""#compdef caco

# Zsh completions for caco
# Install: caco completions --install
# Or place in a directory in your $fpath

# ---------------------------------------------------------------------------
# Dynamic data helpers (call caco _complete for live data)
# ---------------------------------------------------------------------------

__caco_wads() {
    local -a wads
    local id title
    while IFS=$'\t' read -r id title; do
        [[ -n "$id" ]] && wads+=("${id}:${title//:/\\:}")
    done < <(caco _complete wads 2>/dev/null)
    _describe 'WAD' wads
}

__caco_tags() {
    local -a tags
    tags=("${(@f)$(caco _complete tags 2>/dev/null)}")
    compadd -a tags
}

__caco_iwads() {
    local -a iwads
    iwads=("${(@f)$(caco _complete iwads 2>/dev/null)}")
    compadd -a iwads
}

__caco_sourceports() {
    local -a ports
    ports=("${(@f)$(caco _complete sourceports 2>/dev/null)}")
    compadd -a ports
}

# ---------------------------------------------------------------------------
# Static completion helpers
# ---------------------------------------------------------------------------

__caco_query_fields() {
    local -a fields
    fields=(
        'id\::Filter by ID'
        'title\::Filter by title'
        'author\::Filter by author'
        'year\::Filter by year'
        'filename\::Filter by filename'
        'tag\::Filter by tag'
        'status\::Filter by status'
        'source\::Filter by source'
        'iwad\::Filter by IWAD'
    )
    _describe 'query field' fields
}

__caco_sort_fields() {
    local -a fields
    fields=("${(@f)$(caco _complete sort-fields 2>/dev/null)}")
    compadd -a fields
}

__caco_modify_fields() {
    local -a fields
    fields=("${(@f)$(caco _complete modify-fields 2>/dev/null)}")
    compadd -a fields
}

# Combined completion actions for _arguments specs
__caco_query_or_sort() { __caco_query_fields; __caco_sort_fields; }
__caco_wads_or_query() { __caco_wads; __caco_query_fields; }
__caco_wads_query_modify() { __caco_wads; __caco_query_fields; __caco_modify_fields; }

# ---------------------------------------------------------------------------
# Per-command completions
# ---------------------------------------------------------------------------

_caco_ls() {
    _arguments \
        '(-o --output)'{-o,--output}'[Output format]:format:(json plain)' \
        '--tags[List all tags with counts]' \
        '--iwad[List registered IWADs]' \
        '--help[Show help]' \
        '*:query:__caco_query_or_sort'
}

_caco_info() {
    _arguments \
        '(-o --output)'{-o,--output}'[Output format]:format:(json plain)' \
        '--levelstats[Show per-map statistics]' \
        '-b[Target completion by timestamp]:timestamp:' \
        '--live[Show only live stats]' \
        '--plain[TSV output for stats]' \
        '--help[Show help]' \
        '*:query:__caco_wads_or_query'
}

_caco_modify() {
    _arguments \
        '(-y --yes)'{-y,--yes}'[Skip confirmation]' \
        '--dry-run[Preview changes]' \
        '--link[Link a local file]:file:_files' \
        '--notes[Notes for beaten+N]:notes:' \
        '(-s --stats-file)'{-s,--stats-file}'[Stats file for beaten+N or attach]:file:_files' \
        '--date[Backdate completion (ISO)]:date:' \
        '-b[Target completion by timestamp]:timestamp:' \
        '--help[Show help]' \
        '*:query:__caco_wads_query_modify'
}

_caco_trash() {
    _arguments \
        '--list[Show trashed WADs]' \
        '--purge[Permanently delete]' \
        '--restore[Restore from trash]' \
        '--iwad[Remove IWAD]:iwad:__caco_iwads' \
        '(-y --yes)'{-y,--yes}'[Skip confirmation]' \
        '--dry-run[Preview changes]' \
        '(-o --output)'{-o,--output}'[Output format]:format:(json plain)' \
        '--help[Show help]' \
        '*:query:__caco_wads_or_query'
}

_caco_play() {
    _arguments \
        '(-p --sourceport)'{-p,--sourceport}'[Sourceport to use]:sourceport:__caco_sourceports' \
        '(-1 --first)'{-1,--first}'[Auto-select first match]' \
        '--iwad[Play IWAD directly]:iwad:__caco_iwads' \
        '--help[Show help]' \
        '*:query:__caco_wads_or_query'
}

_caco_import() {
    _arguments \
        '--idgames[Force idgames source]' \
        '--doomwiki[Force Doom Wiki source]' \
        '--doomworld[Force Doomworld forum source]' \
        '--local[Force local file import]' \
        '--url[Import from URL]:url:' \
        '(-t --title)'{-t,--title}'[Override title]:title:' \
        '(-a --author)'{-a,--author}'[Author name]:author:' \
        '--year[Year released]:year:' \
        '*--tag[Add tag]:tag:__caco_tags' \
        '(-f --force)'{-f,--force}'[Import even if duplicate exists]' \
        '(-m --multi)'{-m,--multi}'[Allow multi-select]' \
        '(-d --description)'{-d,--description}'[Description]:description:' \
        '(-s --smart)'{-s,--smart}'[Use LLM for metadata extraction]' \
        '--llm-backend[LLM backend]:backend:(claude-code openrouter anthropic openai)' \
        '--llm-model[Model override]:model:' \
        '--help[Show help]' \
        '*:source:'
}

_caco_config() {
    _arguments \
        '(-e --edit)'{-e,--edit}'[Open config in editor]' \
        '--help[Show help]'
}

_caco_completions() {
    _arguments \
        '--install[Install completions to shell config]' \
        '--help[Show help]' \
        '1:shell:(bash fish zsh)'
}

_caco_random() {
    _arguments \
        '--info[Print ID, title, and author]' \
        '--help[Show help]' \
        '*:query:__caco_query_fields'
}

_caco_stats() {
    _arguments \
        '(-p --period)'{-p,--period}'[Group by period]:period:(month year)' \
        '(-n --limit)'{-n,--limit}'[Number of periods]:limit:' \
        '--plain[Key=value output]' \
        '--help[Show help]'
}

_caco_cache() {
    local -a subcmds
    subcmds=(
        'list:List cached files'
        'clear:Remove cached files'
        'prune:Remove orphaned files'
    )

    if (( CURRENT == 2 )); then
        _describe 'cache command' subcmds
        return
    fi

    local subcmd="${words[2]}"
    (( CURRENT-- ))
    shift words

    case "$subcmd" in
        list)
            _arguments \
                '--plain[Output as TSV]' \
                '--orphans[Show orphaned files]' \
                '--help[Show help]'
            ;;
        clear)
            _arguments \
                '--all[Clear entire cache]' \
                '--dry-run[Show what would be deleted]' \
                '(-y --yes)'{-y,--yes}'[Skip confirmation]' \
                '--help[Show help]'
            ;;
        prune)
            _arguments \
                '--dry-run[Show what would be deleted]' \
                '(-y --yes)'{-y,--yes}'[Skip confirmation]' \
                '--help[Show help]'
            ;;
    esac
}

# ---------------------------------------------------------------------------
# Main dispatcher
# ---------------------------------------------------------------------------

_caco() {
    local state

    local -a commands
    commands=(
        'ls:List WADs in your library'
        'info:Show details about a WAD'
        'modify:Modify WAD metadata'
        'trash:Manage trash and removals'
        'play:Play a WAD'
        'import:Import WADs from various sources'
        'config:View or edit configuration'
        'random:Pick a random WAD'
        'completions:Generate shell completions'
        'stats:Show library statistics'
        'cache:Manage WAD file cache'
    )

    if (( CURRENT == 2 )); then
        _describe 'caco command' commands
        local -a options
        options=(
            '--tui:Launch TUI interface'
            '--gui:Launch GUI interface'
            '--help:Show help'
        )
        _describe 'option' options
        return
    fi

    local cmd="${words[2]}"
    (( CURRENT-- ))
    shift words

    case "$cmd" in
        ls) _caco_ls ;;
        info) _caco_info ;;
        modify) _caco_modify ;;
        trash) _caco_trash ;;
        play) _caco_play ;;
        import) _caco_import ;;
        config) _caco_config ;;
        completions) _caco_completions ;;
        random) _caco_random ;;
        stats) _caco_stats ;;
        cache) _caco_cache ;;
    esac
}

_caco "$@"
"""
