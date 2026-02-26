# Fish completions for caco

# Disable file completions by default
complete -c caco -f

# Helper function to get WAD IDs and titles
function __caco_wads
    caco list 2>/dev/null | tail -n +4 | head -n -1 | awk '{print $2"\t"$3}'
end

# Helper function to get tags
function __caco_tags
    caco tag list --plain 2>/dev/null | tail -n +2 | awk -F'\t' '{print $1}'
end

# Global options
complete -c caco -n __fish_use_subcommand -l tui -d "Launch TUI interface"
complete -c caco -n __fish_use_subcommand -l gui -d "Launch GUI interface (requires PySide6)"

# Main commands
complete -c caco -n __fish_use_subcommand -a list -d "List WADs in your library"
complete -c caco -n __fish_use_subcommand -a info -d "Show details about a WAD"
complete -c caco -n __fish_use_subcommand -a update -d "Update a WAD's metadata"
complete -c caco -n __fish_use_subcommand -a delete -d "Delete a WAD from the library"
complete -c caco -n __fish_use_subcommand -a play -d "Play a WAD"
complete -c caco -n __fish_use_subcommand -a import -d "Import WADs from various sources"
complete -c caco -n __fish_use_subcommand -a tag -d "Manage tags"
complete -c caco -n __fish_use_subcommand -a config -d "View or set configuration"
complete -c caco -n __fish_use_subcommand -a random -d "Pick a random WAD (prints ID)"
complete -c caco -n __fish_use_subcommand -a completions -d "Generate shell completions"
complete -c caco -n __fish_use_subcommand -a stats -d "Show library statistics"
complete -c caco -n __fish_use_subcommand -a beaten -d "Manage WAD completion records"
complete -c caco -n __fish_use_subcommand -a restore -d "Restore a deleted WAD"
complete -c caco -n __fish_use_subcommand -a link -d "Link a local file to a WAD"
complete -c caco -n __fish_use_subcommand -a cache -d "Manage WAD file cache"
complete -c caco -n __fish_use_subcommand -a iwad -d "Manage IWAD registry"

# iwad subcommands
complete -c caco -n "__fish_seen_subcommand_from iwad; and not __fish_seen_subcommand_from list add remove scan" -a list -d "List registered IWADs"
complete -c caco -n "__fish_seen_subcommand_from iwad; and not __fish_seen_subcommand_from list add remove scan" -a add -d "Register an IWAD file"
complete -c caco -n "__fish_seen_subcommand_from iwad; and not __fish_seen_subcommand_from list add remove scan" -a remove -d "Unregister an IWAD"
complete -c caco -n "__fish_seen_subcommand_from iwad; and not __fish_seen_subcommand_from list add remove scan" -a scan -d "Scan for known IWADs"

# iwad list options
complete -c caco -n "__fish_seen_subcommand_from iwad; and __fish_seen_subcommand_from list" -l plain -d "Output as TSV"

# iwad add options
complete -c caco -n "__fish_seen_subcommand_from iwad; and __fish_seen_subcommand_from add" -l family -d "Override auto-detected family"
complete -c caco -n "__fish_seen_subcommand_from iwad; and __fish_seen_subcommand_from add" -l variant -d "Override auto-detected variant"
complete -c caco -n "__fish_seen_subcommand_from iwad; and __fish_seen_subcommand_from add" -rF

# iwad remove - complete registered IWAD families and variants
complete -c caco -n "__fish_seen_subcommand_from iwad; and __fish_seen_subcommand_from remove" -xa "(caco iwad list --plain 2>/dev/null | tail -n +2 | awk -F'\t' '{print \$1}' | sort -u)"

# iwad scan options
complete -c caco -n "__fish_seen_subcommand_from iwad; and __fish_seen_subcommand_from scan" -l dir -d "Directory to scan" -rF
complete -c caco -n "__fish_seen_subcommand_from iwad; and __fish_seen_subcommand_from scan" -s y -l yes -d "Register all without prompting"

# list options
complete -c caco -n "__fish_seen_subcommand_from list" -s S -l sort -d "Sort results" -xa "playtime rating created title author last_played year playtime+ rating+ created+ title+ author+ last_played+ year+ playtime- rating- created- title- author- last_played- year-"
complete -c caco -n "__fish_seen_subcommand_from list" -l deleted -d "Show deleted WADs (trash)"
complete -c caco -n "__fish_seen_subcommand_from list" -l json -d "Output as JSON"
complete -c caco -n "__fish_seen_subcommand_from list" -l plain -d "Output as TSV for scripting"

# Query field completions for list
complete -c caco -n "__fish_seen_subcommand_from list" -a "id:" -d "Filter by ID"
complete -c caco -n "__fish_seen_subcommand_from list" -a "title:" -d "Filter by title"
complete -c caco -n "__fish_seen_subcommand_from list" -a "author:" -d "Filter by author"
complete -c caco -n "__fish_seen_subcommand_from list" -a "year:" -d "Filter by year"
complete -c caco -n "__fish_seen_subcommand_from list" -a "filename:" -d "Filter by filename"
complete -c caco -n "__fish_seen_subcommand_from list" -a "tag:" -d "Filter by tag"
complete -c caco -n "__fish_seen_subcommand_from list" -a "status:" -d "Filter by status"
complete -c caco -n "__fish_seen_subcommand_from list" -a "source:" -d "Filter by source"

# Query field completions for info, update, delete, play
complete -c caco -n "__fish_seen_subcommand_from info update delete play" -a "id:" -d "Filter by ID"
complete -c caco -n "__fish_seen_subcommand_from info update delete play" -a "title:" -d "Filter by title"
complete -c caco -n "__fish_seen_subcommand_from info update delete play" -a "author:" -d "Filter by author"
complete -c caco -n "__fish_seen_subcommand_from info update delete play" -a "year:" -d "Filter by year"
complete -c caco -n "__fish_seen_subcommand_from info update delete play" -a "filename:" -d "Filter by filename"
complete -c caco -n "__fish_seen_subcommand_from info update delete play" -a "tag:" -d "Filter by tag"
complete -c caco -n "__fish_seen_subcommand_from info update delete play" -a "status:" -d "Filter by status"
complete -c caco -n "__fish_seen_subcommand_from info update delete play" -a "source:" -d "Filter by source"

# info, update, delete, play - take WAD ID or query
complete -c caco -n "__fish_seen_subcommand_from info update delete play" -xa "(__caco_wads)"

# info options
complete -c caco -n "__fish_seen_subcommand_from info" -l json -d "Output as JSON"
complete -c caco -n "__fish_seen_subcommand_from info" -l plain -d "Output as key=value for scripting"

# update options
complete -c caco -n "__fish_seen_subcommand_from update" -s s -l status -d "Set status" -xa "to-play backlog playing finished abandoned awaiting-update"
complete -c caco -n "__fish_seen_subcommand_from update" -s r -l rating -d "Set rating (1-5)" -xa "1 2 3 4 5"
complete -c caco -n "__fish_seen_subcommand_from update" -s n -l notes -d "Set notes"
complete -c caco -n "__fish_seen_subcommand_from update" -l iwad -d "Custom IWAD path" -rF
complete -c caco -n "__fish_seen_subcommand_from update" -l clear-iwad -d "Clear custom IWAD"
complete -c caco -n "__fish_seen_subcommand_from update" -l sourceport -d "Custom sourceport" -rF
complete -c caco -n "__fish_seen_subcommand_from update" -l clear-sourceport -d "Clear custom sourceport"
complete -c caco -n "__fish_seen_subcommand_from update" -l args -d "Custom arguments"
complete -c caco -n "__fish_seen_subcommand_from update" -l clear-args -d "Clear custom arguments"
complete -c caco -n "__fish_seen_subcommand_from update" -l idgames-id -d "Set idgames file ID for downloading"
complete -c caco -n "__fish_seen_subcommand_from update" -l clear-idgames-id -d "Clear idgames file ID"
complete -c caco -n "__fish_seen_subcommand_from update" -s y -l yes -d "Skip confirmation for multi-WAD updates"

# delete options
complete -c caco -n "__fish_seen_subcommand_from delete" -s y -l yes -d "Skip confirmation prompt"

# play options
complete -c caco -n "__fish_seen_subcommand_from play" -s p -l sourceport -d "Sourceport to use" -rF

# import options (unified command with source flags)
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

# tag subcommands
complete -c caco -n "__fish_seen_subcommand_from tag; and not __fish_seen_subcommand_from add remove list" -a add -d "Add tags to a WAD"
complete -c caco -n "__fish_seen_subcommand_from tag; and not __fish_seen_subcommand_from add remove list" -a remove -d "Remove tags from a WAD"
complete -c caco -n "__fish_seen_subcommand_from tag; and not __fish_seen_subcommand_from add remove list" -a list -d "List all tags"

# tag add/remove - take WAD ID/query then tags
complete -c caco -n "__fish_seen_subcommand_from tag; and __fish_seen_subcommand_from add remove" -xa "(__caco_wads)"
complete -c caco -n "__fish_seen_subcommand_from tag; and __fish_seen_subcommand_from add remove" -a "id:" -d "Filter by ID"
complete -c caco -n "__fish_seen_subcommand_from tag; and __fish_seen_subcommand_from add remove" -a "title:" -d "Filter by title"
complete -c caco -n "__fish_seen_subcommand_from tag; and __fish_seen_subcommand_from add remove" -a "author:" -d "Filter by author"
complete -c caco -n "__fish_seen_subcommand_from tag; and __fish_seen_subcommand_from add remove" -a "filename:" -d "Filter by filename"
complete -c caco -n "__fish_seen_subcommand_from tag; and __fish_seen_subcommand_from add remove" -a "tag:" -d "Filter by tag"
complete -c caco -n "__fish_seen_subcommand_from tag; and __fish_seen_subcommand_from add remove" -a "status:" -d "Filter by status"
complete -c caco -n "__fish_seen_subcommand_from tag; and __fish_seen_subcommand_from add remove" -s y -l yes -d "Skip confirmation for multi-WAD updates"

# tag list options
complete -c caco -n "__fish_seen_subcommand_from tag; and __fish_seen_subcommand_from list" -l plain -d "Output as TSV for scripting"

# random command options and query field completions
complete -c caco -n "__fish_seen_subcommand_from random" -l info -d "Print ID, title, and author"
complete -c caco -n "__fish_seen_subcommand_from random" -a "id:" -d "Filter by ID"
complete -c caco -n "__fish_seen_subcommand_from random" -a "title:" -d "Filter by title"
complete -c caco -n "__fish_seen_subcommand_from random" -a "author:" -d "Filter by author"
complete -c caco -n "__fish_seen_subcommand_from random" -a "status:" -d "Filter by status"
complete -c caco -n "__fish_seen_subcommand_from random" -a "tag:" -d "Filter by tag"
complete -c caco -n "__fish_seen_subcommand_from random" -a "source:" -d "Filter by source"

# cache subcommands
complete -c caco -n "__fish_seen_subcommand_from cache; and not __fish_seen_subcommand_from list clear prune" -a list -d "List cached files"
complete -c caco -n "__fish_seen_subcommand_from cache; and not __fish_seen_subcommand_from list clear prune" -a clear -d "Remove cached files"
complete -c caco -n "__fish_seen_subcommand_from cache; and not __fish_seen_subcommand_from list clear prune" -a prune -d "Remove orphaned files"

# cache list options
complete -c caco -n "__fish_seen_subcommand_from cache; and __fish_seen_subcommand_from list" -l plain -d "Output as TSV"
complete -c caco -n "__fish_seen_subcommand_from cache; and __fish_seen_subcommand_from list" -l orphans -d "Show orphaned files"

# cache clear options
complete -c caco -n "__fish_seen_subcommand_from cache; and __fish_seen_subcommand_from clear" -l all -d "Clear entire cache"
complete -c caco -n "__fish_seen_subcommand_from cache; and __fish_seen_subcommand_from clear" -l dry-run -d "Show what would be deleted"
complete -c caco -n "__fish_seen_subcommand_from cache; and __fish_seen_subcommand_from clear" -s y -l yes -d "Skip confirmation"

# cache prune options
complete -c caco -n "__fish_seen_subcommand_from cache; and __fish_seen_subcommand_from prune" -l dry-run -d "Show what would be deleted"
complete -c caco -n "__fish_seen_subcommand_from cache; and __fish_seen_subcommand_from prune" -s y -l yes -d "Skip confirmation"

# config keys
complete -c caco -n "__fish_seen_subcommand_from config" -xa "sourceport iwad cache_dir download_mirror sourceport_args"

# completions command
complete -c caco -n "__fish_seen_subcommand_from completions" -a "bash fish zsh" -d "Shell type"
complete -c caco -n "__fish_seen_subcommand_from completions" -l install -d "Install completions to config"

# stats command options
complete -c caco -n "__fish_seen_subcommand_from stats; and not __fish_seen_subcommand_from list add remove set export" -s p -l period -d "Group by period" -xa "month year"
complete -c caco -n "__fish_seen_subcommand_from stats; and not __fish_seen_subcommand_from list add remove set export" -s n -l limit -d "Number of periods"
complete -c caco -n "__fish_seen_subcommand_from stats; and not __fish_seen_subcommand_from list add remove set export" -l plain -d "Key=value output"

# beaten subcommands
complete -c caco -n "__fish_seen_subcommand_from beaten; and not __fish_seen_subcommand_from list add attach remove set stats export" -a list -d "List completion records"
complete -c caco -n "__fish_seen_subcommand_from beaten; and not __fish_seen_subcommand_from list add attach remove set stats export" -a add -d "Add a completion record"
complete -c caco -n "__fish_seen_subcommand_from beaten; and not __fish_seen_subcommand_from list add attach remove set stats export" -a attach -d "Attach stats to existing completion"
complete -c caco -n "__fish_seen_subcommand_from beaten; and not __fish_seen_subcommand_from list add attach remove set stats export" -a remove -d "Remove a completion record"
complete -c caco -n "__fish_seen_subcommand_from beaten; and not __fish_seen_subcommand_from list add attach remove set stats export" -a set -d "Set completion count"
complete -c caco -n "__fish_seen_subcommand_from beaten; and not __fish_seen_subcommand_from list add attach remove set stats export" -a stats -d "Show per-map statistics"
complete -c caco -n "__fish_seen_subcommand_from beaten; and not __fish_seen_subcommand_from list add attach remove set stats export" -a export -d "Export stats to file"

# beaten add options
complete -c caco -n "__fish_seen_subcommand_from beaten; and __fish_seen_subcommand_from add" -s n -l notes -d "Notes for this completion"
complete -c caco -n "__fish_seen_subcommand_from beaten; and __fish_seen_subcommand_from add" -s s -l stats-file -d "Import stats from file" -rF
complete -c caco -n "__fish_seen_subcommand_from beaten; and __fish_seen_subcommand_from add" -s y -l yes -d "Auto-select first match"

# beaten attach options
complete -c caco -n "__fish_seen_subcommand_from beaten; and __fish_seen_subcommand_from attach" -s s -l stats-file -d "Stats file to attach" -rF
complete -c caco -n "__fish_seen_subcommand_from beaten; and __fish_seen_subcommand_from attach" -s y -l yes -d "Auto-select first match"

# beaten list/remove/set/stats/export options
complete -c caco -n "__fish_seen_subcommand_from beaten; and __fish_seen_subcommand_from list remove set stats export" -s y -l yes -d "Auto-select first match"
complete -c caco -n "__fish_seen_subcommand_from beaten; and __fish_seen_subcommand_from stats" -l plain -d "TSV output for scripting"
complete -c caco -n "__fish_seen_subcommand_from beaten; and __fish_seen_subcommand_from export" -s o -l output -d "Write to file" -rF

# beaten subcommand WAD query completions
complete -c caco -n "__fish_seen_subcommand_from beaten; and __fish_seen_subcommand_from list add attach remove set stats export" -xa "(__caco_wads)"
