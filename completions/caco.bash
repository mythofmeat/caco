# Bash completions for caco
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
                    -s|--stats-file|--link|-b|--date)
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
            COMPREPLY=($(compgen -W "ls info modify trash play import config random completions stats cache enrich companion" -- "$cur"))
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
        enrich)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "--complevel --dry-run --help" -- "$cur"))
            else
                _caco_wads
                _caco_query_fields
            fi
            ;;
        companion)
            if [[ -z "$subcmd" ]]; then
                COMPREPLY=($(compgen -W "add rm enable disable ls" -- "$cur"))
            else
                case "$subcmd" in
                    add)
                        _caco_wads
                        _caco_filedir
                        ;;
                    rm)
                        if [[ "$cur" == -* ]]; then
                            COMPREPLY=($(compgen -W "-y --yes --help" -- "$cur"))
                        else
                            _caco_wads
                        fi
                        ;;
                    enable|disable)
                        _caco_wads
                        ;;
                    ls)
                        if [[ "$cur" == -* ]]; then
                            COMPREPLY=($(compgen -W "--plain --help" -- "$cur"))
                        else
                            _caco_wads
                        fi
                        ;;
                esac
            fi
            ;;
    esac
}

complete -o default -F _caco caco
