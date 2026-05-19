# Fish completions for caco

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

function __caco_profiles
    caco _complete profiles 2>/dev/null
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
complete -c caco -n __fish_use_subcommand -a enrich -d "Re-run enrichment for existing WADs"
complete -c caco -n __fish_use_subcommand -a companion -d "Manage companion files"
complete -c caco -n __fish_use_subcommand -a gc -d "Garbage collect finished/abandoned WAD data"
complete -c caco -n __fish_use_subcommand -a collection -d "Manage smart collections"
complete -c caco -n __fish_use_subcommand -a profile -d "Manage sourceport config profiles"
complete -c caco -n __fish_use_subcommand -a saves -d "Manage save files"
complete -c caco -n __fish_use_subcommand -a demos -d "Manage demo files"
complete -c caco -n __fish_use_subcommand -a sessions -d "Show play sessions"

# =============================================================================
# companion subcommands
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from companion; and not __fish_seen_subcommand_from add rm enable disable ls" -a add -d "Add a companion file"
complete -c caco -n "__fish_seen_subcommand_from companion; and not __fish_seen_subcommand_from add rm enable disable ls" -a rm -d "Remove a companion file"
complete -c caco -n "__fish_seen_subcommand_from companion; and not __fish_seen_subcommand_from add rm enable disable ls" -a enable -d "Enable a companion file"
complete -c caco -n "__fish_seen_subcommand_from companion; and not __fish_seen_subcommand_from add rm enable disable ls" -a disable -d "Disable a companion file"
complete -c caco -n "__fish_seen_subcommand_from companion; and not __fish_seen_subcommand_from add rm enable disable ls" -a ls -d "List companion files"

complete -c caco -n "__fish_seen_subcommand_from companion; and __fish_seen_subcommand_from add" -xa "(__caco_wads)"
complete -c caco -n "__fish_seen_subcommand_from companion; and __fish_seen_subcommand_from rm" -xa "(__caco_wads)"
complete -c caco -n "__fish_seen_subcommand_from companion; and __fish_seen_subcommand_from rm" -s y -l yes -d "Skip confirmation"
complete -c caco -n "__fish_seen_subcommand_from companion; and __fish_seen_subcommand_from enable" -xa "(__caco_wads)"
complete -c caco -n "__fish_seen_subcommand_from companion; and __fish_seen_subcommand_from disable" -xa "(__caco_wads)"
complete -c caco -n "__fish_seen_subcommand_from companion; and __fish_seen_subcommand_from ls" -xa "(__caco_wads)"
complete -c caco -n "__fish_seen_subcommand_from companion; and __fish_seen_subcommand_from ls" -l plain -d "Plain TSV output"

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
complete -c caco -n "__fish_seen_subcommand_from ls" -a "play:" -d "Filter by play state"
complete -c caco -n "__fish_seen_subcommand_from ls" -a "intent:" -d "Filter by intent"
complete -c caco -n "__fish_seen_subcommand_from ls" -a "avail:" -d "Filter by availability"
complete -c caco -n "__fish_seen_subcommand_from ls" -a "source:" -d "Filter by source"
complete -c caco -n "__fish_seen_subcommand_from ls" -a "iwad:" -d "Filter by IWAD"

# ls inline sort completions
complete -c caco -n "__fish_seen_subcommand_from ls" -a "id+ id- playtime+ playtime- rating+ rating- created+ created- title+ title- author+ author- last_played+ last_played- year+ year-" -d "Sort"

# =============================================================================
# info command
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from info" -s o -l output -d "Output format" -xa "json plain"
complete -c caco -n "__fish_seen_subcommand_from info" -l levelstats -d "Show per-map statistics"
complete -c caco -n "__fish_seen_subcommand_from info" -l completions -d "List completion records with IDs"
complete -c caco -n "__fish_seen_subcommand_from info" -s b -d "Target completion by timestamp"
complete -c caco -n "__fish_seen_subcommand_from info" -l live -d "Show only live stats"
complete -c caco -n "__fish_seen_subcommand_from info" -l plain -d "TSV output for stats"
complete -c caco -n "__fish_seen_subcommand_from info" -xa "(__caco_wads)"
complete -c caco -n "__fish_seen_subcommand_from info" -a "id:" -d "Filter by ID"
complete -c caco -n "__fish_seen_subcommand_from info" -a "title:" -d "Filter by title"
complete -c caco -n "__fish_seen_subcommand_from info" -a "author:" -d "Filter by author"
complete -c caco -n "__fish_seen_subcommand_from info" -a "tag:" -d "Filter by tag"
complete -c caco -n "__fish_seen_subcommand_from info" -a "status:" -d "Filter by status"
complete -c caco -n "__fish_seen_subcommand_from info" -a "play:" -d "Filter by play state"
complete -c caco -n "__fish_seen_subcommand_from info" -a "intent:" -d "Filter by intent"

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
complete -c caco -n "__fish_seen_subcommand_from modify" -l completion -d "Target completion by ID"
complete -c caco -n "__fish_seen_subcommand_from modify" -xa "(__caco_wads)"

# modify field=value completions
complete -c caco -n "__fish_seen_subcommand_from modify" -a "status=" -d "Set status"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "play=" -d "Set play state"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "intent=" -d "Set intent"
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

# modify per-completion edits — use `completion.<id>.<field>=<value>`
complete -c caco -n "__fish_seen_subcommand_from modify" -a "completion." -d "Edit a completion (completion.<id>.notes/date/stats=...)"

# modify query fields
complete -c caco -n "__fish_seen_subcommand_from modify" -a "id:" -d "Filter by ID"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "title:" -d "Filter by title"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "author:" -d "Filter by author"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "tag:" -d "Filter by tag"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "status:" -d "Filter by status"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "play:" -d "Filter by play state"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "intent:" -d "Filter by intent"

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
complete -c caco -n "__fish_seen_subcommand_from trash" -a "play:" -d "Filter by play state"
complete -c caco -n "__fish_seen_subcommand_from trash" -a "intent:" -d "Filter by intent"

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
complete -c caco -n "__fish_seen_subcommand_from play" -a "play:" -d "Filter by play state"
complete -c caco -n "__fish_seen_subcommand_from play" -a "intent:" -d "Filter by intent"

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
complete -c caco -n "__fish_seen_subcommand_from random" -a "play:" -d "Filter by play state"
complete -c caco -n "__fish_seen_subcommand_from random" -a "intent:" -d "Filter by intent"
complete -c caco -n "__fish_seen_subcommand_from random" -a "tag:" -d "Filter by tag"
complete -c caco -n "__fish_seen_subcommand_from random" -a "source:" -d "Filter by source"

# =============================================================================
# stats command
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from stats" -s p -l period -d "Group by period" -xa "month year"
complete -c caco -n "__fish_seen_subcommand_from stats" -s n -l limit -d "Number of periods"
complete -c caco -n "__fish_seen_subcommand_from stats" -s o -l output -d "Output format" -xa "plain json table"

# =============================================================================
# cache subcommands
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from cache; and not __fish_seen_subcommand_from list clear prune" -a list -d "List cached files"
complete -c caco -n "__fish_seen_subcommand_from cache; and not __fish_seen_subcommand_from list clear prune" -a clear -d "Remove cached files"
complete -c caco -n "__fish_seen_subcommand_from cache; and not __fish_seen_subcommand_from list clear prune" -a prune -d "Remove orphaned files"

complete -c caco -n "__fish_seen_subcommand_from cache; and __fish_seen_subcommand_from list" -s o -l output -d "Output format" -xa "plain json table"
complete -c caco -n "__fish_seen_subcommand_from cache; and __fish_seen_subcommand_from list" -l orphans -d "Show orphaned files"
complete -c caco -n "__fish_seen_subcommand_from cache; and __fish_seen_subcommand_from clear" -l all -d "Clear entire cache"
complete -c caco -n "__fish_seen_subcommand_from cache; and __fish_seen_subcommand_from clear" -l dry-run -d "Show what would be deleted"
complete -c caco -n "__fish_seen_subcommand_from cache; and __fish_seen_subcommand_from clear" -s y -l yes -d "Skip confirmation"
complete -c caco -n "__fish_seen_subcommand_from cache; and __fish_seen_subcommand_from prune" -l dry-run -d "Show what would be deleted"
complete -c caco -n "__fish_seen_subcommand_from cache; and __fish_seen_subcommand_from prune" -s y -l yes -d "Skip confirmation"

# =============================================================================
# enrich command
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from enrich" -l complevel -d "Only enrich WADs with missing complevel"
complete -c caco -n "__fish_seen_subcommand_from enrich" -l dry-run -d "Preview changes"
complete -c caco -n "__fish_seen_subcommand_from enrich" -xa "(__caco_wads)"
complete -c caco -n "__fish_seen_subcommand_from enrich" -a "id:" -d "Filter by ID"
complete -c caco -n "__fish_seen_subcommand_from enrich" -a "title:" -d "Filter by title"
complete -c caco -n "__fish_seen_subcommand_from enrich" -a "author:" -d "Filter by author"
complete -c caco -n "__fish_seen_subcommand_from enrich" -a "tag:" -d "Filter by tag"
complete -c caco -n "__fish_seen_subcommand_from enrich" -a "status:" -d "Filter by status"
complete -c caco -n "__fish_seen_subcommand_from enrich" -a "play:" -d "Filter by play state"
complete -c caco -n "__fish_seen_subcommand_from enrich" -a "intent:" -d "Filter by intent"

# =============================================================================
# gc command
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from gc" -l dry-run -d "Preview what would be cleaned"
complete -c caco -n "__fish_seen_subcommand_from gc" -s y -l yes -d "Skip confirmation prompts"
complete -c caco -n "__fish_seen_subcommand_from gc" -l keep-data -d "Don't delete data directories"
complete -c caco -n "__fish_seen_subcommand_from gc" -l keep-cache -d "Don't delete cached WAD files"
complete -c caco -n "__fish_seen_subcommand_from gc" -l keep-saves -d "Preserve save files in data dirs"
complete -c caco -n "__fish_seen_subcommand_from gc" -l keep-demos -d "Preserve demo files in data dirs"
complete -c caco -n "__fish_seen_subcommand_from gc" -l keep-companions -d "Don't delete companion files"
complete -c caco -n "__fish_seen_subcommand_from gc" -l orphans-only -d "Only clean orphaned dirs/backups"
complete -c caco -n "__fish_seen_subcommand_from gc" -l ignore -d "Mark WAD(s) as GC-ignored" -xa "(__caco_wads)"
complete -c caco -n "__fish_seen_subcommand_from gc" -l unignore -d "Remove GC-ignore from WAD(s)" -xa "(__caco_wads)"

# =============================================================================
# collection subcommands
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from collection; and not __fish_seen_subcommand_from add rm ls run" -a add -d "Create a smart collection"
complete -c caco -n "__fish_seen_subcommand_from collection; and not __fish_seen_subcommand_from add rm ls run" -a rm -d "Delete a smart collection"
complete -c caco -n "__fish_seen_subcommand_from collection; and not __fish_seen_subcommand_from add rm ls run" -a ls -d "List smart collections"
complete -c caco -n "__fish_seen_subcommand_from collection; and not __fish_seen_subcommand_from add rm ls run" -a run -d "Run a smart collection"

complete -c caco -n "__fish_seen_subcommand_from collection; and __fish_seen_subcommand_from add" -l sort -d "Sort field"
complete -c caco -n "__fish_seen_subcommand_from collection; and __fish_seen_subcommand_from add" -l desc -d "Sort descending"
complete -c caco -n "__fish_seen_subcommand_from collection; and __fish_seen_subcommand_from run" -s o -l output -d "Output format" -xa "json plain"
complete -c caco -n "__fish_seen_subcommand_from collection; and __fish_seen_subcommand_from ls" -s o -l output -d "Output format" -xa "json plain"

# =============================================================================
# profile subcommands
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from profile; and not __fish_seen_subcommand_from ls create edit cp rm path" -a ls -d "List config profiles"
complete -c caco -n "__fish_seen_subcommand_from profile; and not __fish_seen_subcommand_from ls create edit cp rm path" -a create -d "Create a new profile"
complete -c caco -n "__fish_seen_subcommand_from profile; and not __fish_seen_subcommand_from ls create edit cp rm path" -a edit -d "Open profile in editor"
complete -c caco -n "__fish_seen_subcommand_from profile; and not __fish_seen_subcommand_from ls create edit cp rm path" -a cp -d "Copy a profile"
complete -c caco -n "__fish_seen_subcommand_from profile; and not __fish_seen_subcommand_from ls create edit cp rm path" -a rm -d "Delete a profile"
complete -c caco -n "__fish_seen_subcommand_from profile; and not __fish_seen_subcommand_from ls create edit cp rm path" -a path -d "Print path to profile file"

complete -c caco -n "__fish_seen_subcommand_from profile; and __fish_seen_subcommand_from ls" -s p -l sourceport -d "Filter by sourceport" -xa "(__caco_sourceports)"
complete -c caco -n "__fish_seen_subcommand_from profile; and __fish_seen_subcommand_from create" -s p -l sourceport -d "Sourceport" -xa "(__caco_sourceports)"
complete -c caco -n "__fish_seen_subcommand_from profile; and __fish_seen_subcommand_from create" -l from -d "Copy from existing profile" -xa "(__caco_profiles)"
complete -c caco -n "__fish_seen_subcommand_from profile; and __fish_seen_subcommand_from edit" -s p -l sourceport -d "Sourceport" -xa "(__caco_sourceports)"
complete -c caco -n "__fish_seen_subcommand_from profile; and __fish_seen_subcommand_from edit" -xa "(__caco_profiles)"
complete -c caco -n "__fish_seen_subcommand_from profile; and __fish_seen_subcommand_from cp" -s p -l sourceport -d "Sourceport" -xa "(__caco_sourceports)"
complete -c caco -n "__fish_seen_subcommand_from profile; and __fish_seen_subcommand_from cp" -xa "(__caco_profiles)"
complete -c caco -n "__fish_seen_subcommand_from profile; and __fish_seen_subcommand_from rm" -s p -l sourceport -d "Sourceport" -xa "(__caco_sourceports)"
complete -c caco -n "__fish_seen_subcommand_from profile; and __fish_seen_subcommand_from rm" -s y -l yes -d "Skip confirmation"
complete -c caco -n "__fish_seen_subcommand_from profile; and __fish_seen_subcommand_from rm" -xa "(__caco_profiles)"
complete -c caco -n "__fish_seen_subcommand_from profile; and __fish_seen_subcommand_from path" -s p -l sourceport -d "Sourceport" -xa "(__caco_sourceports)"
complete -c caco -n "__fish_seen_subcommand_from profile; and __fish_seen_subcommand_from path" -xa "(__caco_profiles)"

# =============================================================================
# saves subcommands
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from saves; and not __fish_seen_subcommand_from list backup restore clean backups" -a list -d "List save files"
complete -c caco -n "__fish_seen_subcommand_from saves; and not __fish_seen_subcommand_from list backup restore clean backups" -a backup -d "Backup save files"
complete -c caco -n "__fish_seen_subcommand_from saves; and not __fish_seen_subcommand_from list backup restore clean backups" -a restore -d "Restore save files"
complete -c caco -n "__fish_seen_subcommand_from saves; and not __fish_seen_subcommand_from list backup restore clean backups" -a clean -d "Clean save files"
complete -c caco -n "__fish_seen_subcommand_from saves; and not __fish_seen_subcommand_from list backup restore clean backups" -a backups -d "List backups"

complete -c caco -n "__fish_seen_subcommand_from saves; and __fish_seen_subcommand_from list" -xa "(__caco_wads)"
complete -c caco -n "__fish_seen_subcommand_from saves; and __fish_seen_subcommand_from list" -s o -l output -d "Output format" -xa "plain json table"
complete -c caco -n "__fish_seen_subcommand_from saves; and __fish_seen_subcommand_from backup" -xa "(__caco_wads)"
complete -c caco -n "__fish_seen_subcommand_from saves; and __fish_seen_subcommand_from restore" -xa "(__caco_wads)"
complete -c caco -n "__fish_seen_subcommand_from saves; and __fish_seen_subcommand_from clean" -xa "(__caco_wads)"
complete -c caco -n "__fish_seen_subcommand_from saves; and __fish_seen_subcommand_from clean" -s y -l yes -d "Skip confirmation"
complete -c caco -n "__fish_seen_subcommand_from saves; and __fish_seen_subcommand_from backups" -xa "(__caco_wads)"
complete -c caco -n "__fish_seen_subcommand_from saves; and __fish_seen_subcommand_from backups" -s o -l output -d "Output format" -xa "plain json table"

# =============================================================================
# demos subcommands
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from demos; and not __fish_seen_subcommand_from list play clean" -a list -d "List demo files"
complete -c caco -n "__fish_seen_subcommand_from demos; and not __fish_seen_subcommand_from list play clean" -a play -d "Play a demo"
complete -c caco -n "__fish_seen_subcommand_from demos; and not __fish_seen_subcommand_from list play clean" -a clean -d "Clean demo files"

complete -c caco -n "__fish_seen_subcommand_from demos; and __fish_seen_subcommand_from list" -xa "(__caco_wads)"
complete -c caco -n "__fish_seen_subcommand_from demos; and __fish_seen_subcommand_from list" -s o -l output -d "Output format" -xa "plain json table"
complete -c caco -n "__fish_seen_subcommand_from demos; and __fish_seen_subcommand_from play" -xa "(__caco_wads)"
complete -c caco -n "__fish_seen_subcommand_from demos; and __fish_seen_subcommand_from clean" -xa "(__caco_wads)"
complete -c caco -n "__fish_seen_subcommand_from demos; and __fish_seen_subcommand_from clean" -s y -l yes -d "Skip confirmation"

# =============================================================================
# sessions command
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from sessions" -xa "(__caco_wads)"
complete -c caco -n "__fish_seen_subcommand_from sessions" -l plain -d "Plain TSV output"
