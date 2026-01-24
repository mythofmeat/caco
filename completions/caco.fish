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

# Main commands
complete -c caco -n __fish_use_subcommand -a list -d "List WADs in your library"
complete -c caco -n __fish_use_subcommand -a info -d "Show details about a WAD"
complete -c caco -n __fish_use_subcommand -a update -d "Update a WAD's metadata"
complete -c caco -n __fish_use_subcommand -a delete -d "Delete a WAD from the library"
complete -c caco -n __fish_use_subcommand -a play -d "Play a WAD"
complete -c caco -n __fish_use_subcommand -a import -d "Import WADs from various sources"
complete -c caco -n __fish_use_subcommand -a tag -d "Manage tags"
complete -c caco -n __fish_use_subcommand -a config -d "View or set configuration"
complete -c caco -n __fish_use_subcommand -a completions -d "Generate shell completions"
complete -c caco -n __fish_use_subcommand -a pl -d "List playing WADs"
complete -c caco -n __fish_use_subcommand -a wl -d "List wishlist WADs"
complete -c caco -n __fish_use_subcommand -a bl -d "List backlog WADs"

# list options
complete -c caco -n "__fish_seen_subcommand_from list" -s s -l status -d "Filter by status" -xa "wishlist backlog playing finished abandoned"
complete -c caco -n "__fish_seen_subcommand_from list" -s t -l tag -d "Filter by tag" -xa "(__caco_tags)"
complete -c caco -n "__fish_seen_subcommand_from list" -l source -d "Filter by source" -xa "idgames doomwiki doomworld url local"
complete -c caco -n "__fish_seen_subcommand_from list" -l plain -d "Output as TSV for scripting"

# pl, wl, bl options
complete -c caco -n "__fish_seen_subcommand_from pl wl bl" -l plain -d "Output as TSV for scripting"

# Query field completions for list, pl, wl, bl
complete -c caco -n "__fish_seen_subcommand_from list pl wl bl" -a "id:" -d "Filter by ID"
complete -c caco -n "__fish_seen_subcommand_from list pl wl bl" -a "title:" -d "Filter by title"
complete -c caco -n "__fish_seen_subcommand_from list pl wl bl" -a "author:" -d "Filter by author"
complete -c caco -n "__fish_seen_subcommand_from list pl wl bl" -a "year:" -d "Filter by year"
complete -c caco -n "__fish_seen_subcommand_from list pl wl bl" -a "filename:" -d "Filter by filename"
complete -c caco -n "__fish_seen_subcommand_from list pl wl bl" -a "tag:" -d "Filter by tag"
complete -c caco -n "__fish_seen_subcommand_from list pl wl bl" -a "status:" -d "Filter by status"
complete -c caco -n "__fish_seen_subcommand_from list pl wl bl" -a "source:" -d "Filter by source"

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
complete -c caco -n "__fish_seen_subcommand_from update" -s s -l status -d "Set status" -xa "wishlist backlog playing finished abandoned"
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
complete -c caco -n "__fish_seen_subcommand_from import; and not __fish_seen_subcommand_from idgames url local" -a idgames -d "Import from idgames archive"
complete -c caco -n "__fish_seen_subcommand_from import; and not __fish_seen_subcommand_from idgames url local" -a url -d "Import from a URL"
complete -c caco -n "__fish_seen_subcommand_from import; and not __fish_seen_subcommand_from idgames url local" -a local -d "Import a local file"

# import idgames/url/local options
complete -c caco -n "__fish_seen_subcommand_from idgames url local" -s t -l tag -d "Add tag"

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

# config keys
complete -c caco -n "__fish_seen_subcommand_from config" -xa "sourceport iwad cache_dir download_mirror sourceport_args"

# completions command
complete -c caco -n "__fish_seen_subcommand_from completions" -a "bash fish zsh" -d "Shell type"
complete -c caco -n "__fish_seen_subcommand_from completions" -l install -d "Install completions to config"
