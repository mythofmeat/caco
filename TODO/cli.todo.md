# list
- remove `list` and make `ls` the one and only command
- remove `--sort` and `-S` and instead use a similar syntax to beets. e.g., `caco ls author:"erik alm" id+` shows wads by erik alm sorted by id ascending
- remove `--json` and `--plain`, replace with `--output=[json|plain]`
- `--tags` flag to list all tags with counts (replaces `tag list`)

# info
- remove `--yes`: if multiple matches, display all results in sequence
- remove `--json` and `--plain`, replace with `--output=[json|plain]`

# update *big changes*
- rename `update` to `modify`
- no more `--title`/`--author` etc. flags to modify metadata, instead steal wholesale from beets
  - so you specify the query with the same syntax as `caco ls`, with a `:`, and you specify the modification with `[field]=[value]`
  - e.g., `caco modify id:22..28 status=to-play` or `caco modify author:"erik alm" rating=5` or `caco modify id:33 notes="decent fights but very obscure progression"`
- you can remove fields with `caco modify id:19..21 !tag` for example to remove all tags, or `caco modify id:19..21 !tag:"cacoward*"` to remove all tags that match the glob

## tag
- folded into `modify` with `tag:` for queries and `tag=` for modifications
- `caco ls --tags` shows all tags with counts (standalone read-only command)

## link
- folded into `modify --link PATH`
- default behavior: move file into managed wad cache (configurable to copy via config)
- config key: `link_mode = "move"` (or `"copy"`)

# delete / restore
- rename to `trash`
- `caco trash id:30` puts it in the trash
- `caco trash --list` shows trashed wads
- `caco trash --purge [query]`
- `caco trash --restore [query]`

# random
- leave as-is

# import
- leave as-is (source auto-detection stays)
- auto-detect IWAD files via MD5 — no `--iwad` flag needed
- when an IWAD is detected, manage it separately (copy to managed iwad dir, register in DB)
- document the auto-detection behavior

# play
- rename `--yes` to `--first` / `-1` (auto-select first match for scripting)
- `--iwad FAMILY[/VARIANT]` to play an IWAD directly
  - `caco play --iwad doom2` → preferred variant (priority resolution)
  - `caco play --iwad doom2/v1.9` → exact variant
  - `caco play --iwad doom2/bfg` → BFG edition
- `iwad:` prefix in query syntax is now a filter, not play syntax
  - `caco ls iwad:doom2` → list all PWADs requiring Doom 2
  - `caco ls iwad:tnt` → list all PWADs requiring TNT

# cache
- leave as-is

# iwad
- remove as standalone group
- `iwad list` → `caco ls --iwad`
- `iwad import` → folded into `caco import` (auto-detected via MD5)
- `iwad remove` → `caco trash --iwad FAMILY [VARIANT]`

# config
- `caco config` prints current configuration with defaults to stdout (pipeable)
- `caco config -e` / `caco config --edit` remains the same
- remove `--path`

# completions
- leave as-is (update after CLI rework is done)

# beaten / stats
- deferring until later
- beaten / stats should probably be merged somehow
