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
complete -c caco -n __fish_use_subcommand -a sessions -d "Show play session history"
complete -c caco -n __fish_use_subcommand -a cache -d "Manage WAD file cache"
complete -c caco -n __fish_use_subcommand -a enrich -d "Re-run enrichment for existing WADs"
complete -c caco -n __fish_use_subcommand -a profile -d "Manage sourceport config profiles"
complete -c caco -n __fish_use_subcommand -a companion -d "Manage companion files"

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
complete -c caco -n "__fish_seen_subcommand_from ls" -l id24 -d "List registered id24 WADs"

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
complete -c caco -n "__fish_seen_subcommand_from ls" -a "complevel:" -d "Filter by complevel"
complete -c caco -n "__fish_seen_subcommand_from ls" -a "config:" -d "Filter by config profile"

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
complete -c caco -n "__fish_seen_subcommand_from modify" -l add-file -d "Add companion file (DEH, music, etc.)" -rF
complete -c caco -n "__fish_seen_subcommand_from modify" -l remove-file -d "Remove companion file" -x
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
complete -c caco -n "__fish_seen_subcommand_from modify" -a "complevel=" -d "Set complevel (int or alias)"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "config=" -d "Set config profile"

# modify clear completions
complete -c caco -n "__fish_seen_subcommand_from modify" -a "!author" -d "Clear author"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "!year" -d "Clear year"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "!description" -d "Clear description"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "!notes" -d "Clear notes"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "!rating" -d "Clear rating"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "!iwad" -d "Clear custom IWAD"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "!sourceport" -d "Clear custom sourceport"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "!complevel" -d "Clear complevel"
complete -c caco -n "__fish_seen_subcommand_from modify" -a "!config" -d "Clear config profile"
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
complete -c caco -n "__fish_seen_subcommand_from trash" -l id24 -d "Remove id24 WAD by name"
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
complete -c caco -n "__fish_seen_subcommand_from play" -s r -l record -d "Record a demo (auto-name or specify name)"
complete -c caco -n "__fish_seen_subcommand_from play" -s c -l complevel -d "Override complevel (int or alias)" -xa "vanilla boom mbf mbf21 2 9 11 21"
complete -c caco -n "__fish_seen_subcommand_from play" -s C -l config -d "Sourceport config profile name"
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

# =============================================================================
# sessions command
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from sessions" -l plain -d "TSV output for scripting"
complete -c caco -n "__fish_seen_subcommand_from sessions" -s y -l yes -d "Auto-select first match"
complete -c caco -n "__fish_seen_subcommand_from sessions" -xa "(__caco_wads)"
complete -c caco -n "__fish_seen_subcommand_from sessions" -a "id:" -d "Filter by ID"
complete -c caco -n "__fish_seen_subcommand_from sessions" -a "title:" -d "Filter by title"
complete -c caco -n "__fish_seen_subcommand_from sessions" -a "author:" -d "Filter by author"
complete -c caco -n "__fish_seen_subcommand_from sessions" -a "tag:" -d "Filter by tag"
complete -c caco -n "__fish_seen_subcommand_from sessions" -a "status:" -d "Filter by status"

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

# =============================================================================
# profile subcommands
# =============================================================================
complete -c caco -n "__fish_seen_subcommand_from profile; and not __fish_seen_subcommand_from ls create edit cp rm path" -a ls -d "List config profiles"
complete -c caco -n "__fish_seen_subcommand_from profile; and not __fish_seen_subcommand_from ls create edit cp rm path" -a create -d "Create a config profile"
complete -c caco -n "__fish_seen_subcommand_from profile; and not __fish_seen_subcommand_from ls create edit cp rm path" -a edit -d "Edit a config profile"
complete -c caco -n "__fish_seen_subcommand_from profile; and not __fish_seen_subcommand_from ls create edit cp rm path" -a cp -d "Copy a config profile"
complete -c caco -n "__fish_seen_subcommand_from profile; and not __fish_seen_subcommand_from ls create edit cp rm path" -a rm -d "Delete a config profile"
complete -c caco -n "__fish_seen_subcommand_from profile; and not __fish_seen_subcommand_from ls create edit cp rm path" -a path -d "Print profile path"

complete -c caco -n "__fish_seen_subcommand_from profile; and __fish_seen_subcommand_from ls" -s p -l sourceport -d "Sourceport to list profiles for"
complete -c caco -n "__fish_seen_subcommand_from profile; and __fish_seen_subcommand_from create" -s p -l sourceport -d "Sourceport (defaults to configured)"
complete -c caco -n "__fish_seen_subcommand_from profile; and __fish_seen_subcommand_from create" -l from -d "Copy from existing profile"
complete -c caco -n "__fish_seen_subcommand_from profile; and __fish_seen_subcommand_from edit" -s p -l sourceport -d "Sourceport (defaults to configured)"
complete -c caco -n "__fish_seen_subcommand_from profile; and __fish_seen_subcommand_from cp" -s p -l sourceport -d "Sourceport (defaults to configured)"
complete -c caco -n "__fish_seen_subcommand_from profile; and __fish_seen_subcommand_from rm" -s p -l sourceport -d "Sourceport (defaults to configured)"
complete -c caco -n "__fish_seen_subcommand_from profile; and __fish_seen_subcommand_from rm" -s y -l yes -d "Skip confirmation"
complete -c caco -n "__fish_seen_subcommand_from profile; and __fish_seen_subcommand_from path" -s p -l sourceport -d "Sourceport (defaults to configured)"

# =============================================================================
# saves subcommands
# =============================================================================
complete -c caco -n __fish_use_subcommand -a saves -d "Manage WAD save files and backups"

complete -c caco -n "__fish_seen_subcommand_from saves; and not __fish_seen_subcommand_from list backup restore clean backups" -a list -d "List save files for a WAD"
complete -c caco -n "__fish_seen_subcommand_from saves; and not __fish_seen_subcommand_from list backup restore clean backups" -a backup -d "Backup a WAD's data directory"
complete -c caco -n "__fish_seen_subcommand_from saves; and not __fish_seen_subcommand_from list backup restore clean backups" -a restore -d "Restore from a backup"
complete -c caco -n "__fish_seen_subcommand_from saves; and not __fish_seen_subcommand_from list backup restore clean backups" -a clean -d "Delete save files (keep stats)"
complete -c caco -n "__fish_seen_subcommand_from saves; and not __fish_seen_subcommand_from list backup restore clean backups" -a backups -d "List backup files"

complete -c caco -n "__fish_seen_subcommand_from saves; and __fish_seen_subcommand_from list" -l plain -d "Output as TSV"
complete -c caco -n "__fish_seen_subcommand_from saves; and __fish_seen_subcommand_from list" -s y -l yes -d "Auto-select first match"
complete -c caco -n "__fish_seen_subcommand_from saves; and __fish_seen_subcommand_from backup" -s y -l yes -d "Auto-select first match"
complete -c caco -n "__fish_seen_subcommand_from saves; and __fish_seen_subcommand_from restore" -s y -l yes -d "Skip confirmation"
complete -c caco -n "__fish_seen_subcommand_from saves; and __fish_seen_subcommand_from clean" -l dry-run -d "Show what would be deleted"
complete -c caco -n "__fish_seen_subcommand_from saves; and __fish_seen_subcommand_from clean" -s y -l yes -d "Skip confirmation"
complete -c caco -n "__fish_seen_subcommand_from saves; and __fish_seen_subcommand_from backups" -l plain -d "Output as TSV"
complete -c caco -n "__fish_seen_subcommand_from saves; and __fish_seen_subcommand_from backups" -s y -l yes -d "Auto-select first match"

complete -c caco -n "__fish_seen_subcommand_from saves; and __fish_seen_subcommand_from list backup restore clean backups" -xa "(__caco_wads)"

# =============================================================================
# demos subcommands
# =============================================================================
complete -c caco -n __fish_use_subcommand -a demos -d "Manage WAD demo recordings"

complete -c caco -n "__fish_seen_subcommand_from demos; and not __fish_seen_subcommand_from list play clean" -a list -d "List demo files for a WAD"
complete -c caco -n "__fish_seen_subcommand_from demos; and not __fish_seen_subcommand_from list play clean" -a play -d "Play back a recorded demo"
complete -c caco -n "__fish_seen_subcommand_from demos; and not __fish_seen_subcommand_from list play clean" -a clean -d "Delete demo files"

complete -c caco -n "__fish_seen_subcommand_from demos; and __fish_seen_subcommand_from list" -l plain -d "Output as TSV"
complete -c caco -n "__fish_seen_subcommand_from demos; and __fish_seen_subcommand_from list" -s y -l yes -d "Auto-select first match"
complete -c caco -n "__fish_seen_subcommand_from demos; and __fish_seen_subcommand_from play" -s p -l sourceport -d "Sourceport to use" -rF
complete -c caco -n "__fish_seen_subcommand_from demos; and __fish_seen_subcommand_from play" -s y -l yes -d "Auto-select first match"
complete -c caco -n "__fish_seen_subcommand_from demos; and __fish_seen_subcommand_from clean" -l dry-run -d "Show what would be deleted"
complete -c caco -n "__fish_seen_subcommand_from demos; and __fish_seen_subcommand_from clean" -s y -l yes -d "Skip confirmation"

complete -c caco -n "__fish_seen_subcommand_from demos; and __fish_seen_subcommand_from list play clean" -xa "(__caco_wads)"
