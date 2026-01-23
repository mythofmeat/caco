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

# list options
complete -c caco -n "__fish_seen_subcommand_from list" -s s -l status -d "Filter by status" -xa "wishlist backlog playing finished abandoned"
complete -c caco -n "__fish_seen_subcommand_from list" -s t -l tag -d "Filter by tag" -xa "(__caco_tags)"
complete -c caco -n "__fish_seen_subcommand_from list" -l source -d "Filter by source" -xa "idgames doomwiki doomworld url local"

# info, update, delete, play - take WAD ID
complete -c caco -n "__fish_seen_subcommand_from info update delete play" -xa "(__caco_wads)"

# update options
complete -c caco -n "__fish_seen_subcommand_from update" -s s -l status -d "Set status" -xa "wishlist backlog playing finished abandoned"
complete -c caco -n "__fish_seen_subcommand_from update" -s r -l rating -d "Set rating (1-5)" -xa "1 2 3 4 5"
complete -c caco -n "__fish_seen_subcommand_from update" -s n -l notes -d "Set notes"

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

# tag add/remove - take WAD ID then tags
complete -c caco -n "__fish_seen_subcommand_from tag; and __fish_seen_subcommand_from add remove" -xa "(__caco_wads)"

# config keys
complete -c caco -n "__fish_seen_subcommand_from config" -xa "sourceport iwad cache_dir download_mirror sourceport_args"
