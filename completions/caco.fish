# Fish completions for caco

# Disable file completions by default
complete -c caco -f

# Helper function to get WAD IDs and titles
function __caco_wads
    caco list 2>/dev/null | tail -n +4 | head -n -1 | awk '{print $2"\t"$3}'
end

# Helper function to get tags
function __caco_tags
    caco tag list 2>/dev/null
end

# Global options
complete -c caco -n __fish_use_subcommand -l tui -d "Launch TUI interface"

# Main commands
complete -c caco -n __fish_use_subcommand -a list -d "List WADs in your library"
complete -c caco -n __fish_use_subcommand -a info -d "Show details about a WAD"
complete -c caco -n __fish_use_subcommand -a update -d "Update a WAD's metadata"
complete -c caco -n __fish_use_subcommand -a delete -d "Delete a WAD from the library"
complete -c caco -n __fish_use_subcommand -a play -d "Play a WAD"
complete -c caco -n __fish_use_subcommand -a import -d "Import WADs from various sources"
complete -c caco -n __fish_use_subcommand -a tag -d "Manage tags"
complete -c caco -n __fish_use_subcommand -a map -d "Manage map completions"
complete -c caco -n __fish_use_subcommand -a config -d "View or set configuration"
complete -c caco -n __fish_use_subcommand -a completions -d "Generate shell completions"

# list options
complete -c caco -n "__fish_seen_subcommand_from list" -s s -l status -d "Filter by status" -xa "to-play backlog playing finished abandoned"
complete -c caco -n "__fish_seen_subcommand_from list" -s t -l tag -d "Filter by tag" -xa "(__caco_tags)"
complete -c caco -n "__fish_seen_subcommand_from list" -l source -d "Filter by source" -xa "idgames doomwiki doomworld url local"
complete -c caco -n "__fish_seen_subcommand_from list" -s S -l sort -d "Sort results" -xa "playtime rating created title author last_played year -playtime -rating -created -title -author -last_played -year"
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
complete -c caco -n "__fish_seen_subcommand_from info" -l plain -d "Output as key=value for scripting"

# update options
complete -c caco -n "__fish_seen_subcommand_from update" -s s -l status -d "Set status" -xa "to-play backlog playing finished abandoned"
complete -c caco -n "__fish_seen_subcommand_from update" -s r -l rating -d "Set rating (1-5)" -xa "1 2 3 4 5"
complete -c caco -n "__fish_seen_subcommand_from update" -s n -l notes -d "Set notes"
complete -c caco -n "__fish_seen_subcommand_from update" -l iwad -d "Custom IWAD path" -rF
complete -c caco -n "__fish_seen_subcommand_from update" -l clear-iwad -d "Clear custom IWAD"
complete -c caco -n "__fish_seen_subcommand_from update" -l sourceport -d "Custom sourceport" -rF
complete -c caco -n "__fish_seen_subcommand_from update" -l clear-sourceport -d "Clear custom sourceport"
complete -c caco -n "__fish_seen_subcommand_from update" -l args -d "Custom arguments"
complete -c caco -n "__fish_seen_subcommand_from update" -l clear-args -d "Clear custom arguments"
complete -c caco -n "__fish_seen_subcommand_from update" -s y -l yes -d "Skip confirmation for multi-WAD updates"

# delete options
complete -c caco -n "__fish_seen_subcommand_from delete" -s y -l yes -d "Skip confirmation prompt"

# play options
complete -c caco -n "__fish_seen_subcommand_from play" -s p -l sourceport -d "Sourceport to use" -rF

# import subcommands
complete -c caco -n "__fish_seen_subcommand_from import; and not __fish_seen_subcommand_from idgames doomwiki doomworld url local auto" -a idgames -d "Import from idgames archive"
complete -c caco -n "__fish_seen_subcommand_from import; and not __fish_seen_subcommand_from idgames doomwiki doomworld url local auto" -a doomwiki -d "Import from Doom Wiki"
complete -c caco -n "__fish_seen_subcommand_from import; and not __fish_seen_subcommand_from idgames doomwiki doomworld url local auto" -a doomworld -d "Import from Doomworld forum"
complete -c caco -n "__fish_seen_subcommand_from import; and not __fish_seen_subcommand_from idgames doomwiki doomworld url local auto" -a url -d "Import from a URL"
complete -c caco -n "__fish_seen_subcommand_from import; and not __fish_seen_subcommand_from idgames doomwiki doomworld url local auto" -a local -d "Import a local file"
complete -c caco -n "__fish_seen_subcommand_from import; and not __fish_seen_subcommand_from idgames doomwiki doomworld url local auto" -a auto -d "Auto-detect source type"

# import common options
complete -c caco -n "__fish_seen_subcommand_from idgames doomwiki doomworld url local" -s t -l tag -d "Add tag"
complete -c caco -n "__fish_seen_subcommand_from idgames doomwiki doomworld url local" -s f -l force -d "Import even if duplicate exists"

# import idgames options
complete -c caco -n "__fish_seen_subcommand_from idgames" -s m -l multi -d "Allow multi-select (requires fzf)"

# import doomwiki options
complete -c caco -n "__fish_seen_subcommand_from doomwiki" -s m -l multi -d "Allow multi-select (requires fzf)"

# import doomworld options
complete -c caco -n "__fish_seen_subcommand_from doomworld" -l title -d "Override parsed title"
complete -c caco -n "__fish_seen_subcommand_from doomworld" -s a -l author -d "Override parsed author"
complete -c caco -n "__fish_seen_subcommand_from doomworld" -s y -l year -d "Override parsed year"
complete -c caco -n "__fish_seen_subcommand_from doomworld" -s s -l smart -d "Use LLM for metadata extraction"
complete -c caco -n "__fish_seen_subcommand_from doomworld" -l llm-backend -d "LLM backend" -xa "claude-code openrouter anthropic openai"
complete -c caco -n "__fish_seen_subcommand_from doomworld" -l llm-model -d "Model override for API backends"

# import url options
complete -c caco -n "__fish_seen_subcommand_from url" -s a -l author -d "Author name"
complete -c caco -n "__fish_seen_subcommand_from url" -s y -l year -d "Release year"
complete -c caco -n "__fish_seen_subcommand_from url" -s d -l description -d "Description"

# import local options
complete -c caco -n "__fish_seen_subcommand_from local" -s a -l author -d "Author name"
complete -c caco -n "__fish_seen_subcommand_from local" -s y -l year -d "Release year"

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

# map subcommands
complete -c caco -n "__fish_seen_subcommand_from map; and not __fish_seen_subcommand_from sync complete uncomplete list progress" -a sync -d "Sync map completions from stats.txt"
complete -c caco -n "__fish_seen_subcommand_from map; and not __fish_seen_subcommand_from sync complete uncomplete list progress" -a complete -d "Mark maps as completed"
complete -c caco -n "__fish_seen_subcommand_from map; and not __fish_seen_subcommand_from sync complete uncomplete list progress" -a uncomplete -d "Remove map completion records"
complete -c caco -n "__fish_seen_subcommand_from map; and not __fish_seen_subcommand_from sync complete uncomplete list progress" -a list -d "List completed maps"
complete -c caco -n "__fish_seen_subcommand_from map; and not __fish_seen_subcommand_from sync complete uncomplete list progress" -a progress -d "Show completion progress"

# map sync options
complete -c caco -n "__fish_seen_subcommand_from map; and __fish_seen_subcommand_from sync" -l all -d "Sync all WADs"
complete -c caco -n "__fish_seen_subcommand_from map; and __fish_seen_subcommand_from sync" -xa "(__caco_wads)"

# map complete options
complete -c caco -n "__fish_seen_subcommand_from map; and __fish_seen_subcommand_from complete" -s s -l skill -d "Skill level (1-5)" -xa "1 2 3 4 5"
complete -c caco -n "__fish_seen_subcommand_from map; and __fish_seen_subcommand_from complete" -s n -l notes -d "Notes"
complete -c caco -n "__fish_seen_subcommand_from map; and __fish_seen_subcommand_from complete" -xa "(__caco_wads)"

# map uncomplete options
complete -c caco -n "__fish_seen_subcommand_from map; and __fish_seen_subcommand_from uncomplete" -s s -l skill -d "Only remove specific skill" -xa "1 2 3 4 5"
complete -c caco -n "__fish_seen_subcommand_from map; and __fish_seen_subcommand_from uncomplete" -xa "(__caco_wads)"

# map list options
complete -c caco -n "__fish_seen_subcommand_from map; and __fish_seen_subcommand_from list" -l plain -d "Output as TSV for scripting"
complete -c caco -n "__fish_seen_subcommand_from map; and __fish_seen_subcommand_from list" -xa "(__caco_wads)"

# map progress options
complete -c caco -n "__fish_seen_subcommand_from map; and __fish_seen_subcommand_from progress" -s t -l total -d "Total number of maps"
complete -c caco -n "__fish_seen_subcommand_from map; and __fish_seen_subcommand_from progress" -l plain -d "Output as key=value for scripting"
complete -c caco -n "__fish_seen_subcommand_from map; and __fish_seen_subcommand_from progress" -xa "(__caco_wads)"

# config keys
complete -c caco -n "__fish_seen_subcommand_from config" -xa "sourceport iwad cache_dir stats_dir download_mirror sourceport_args"

# completions command
complete -c caco -n "__fish_seen_subcommand_from completions" -a "bash fish zsh" -d "Shell type"
complete -c caco -n "__fish_seen_subcommand_from completions" -l install -d "Install completions to config"
