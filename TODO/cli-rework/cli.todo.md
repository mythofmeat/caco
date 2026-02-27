# list
- [x] remove `list` and make `ls` the one and only command
- [x] remove `--sort` and `-S` and instead use a similar syntax to beets. e.g., `caco ls author:"erik alm" id+` shows wads by erik alm sorted by id ascending
- [x] remove `--json` and `--plain`, replace with `--output=[json|plain]`
- [x] `--tags` flag to list all tags with counts (replaces `tag list`)

# info
- [x] remove `--yes`: if multiple matches, display all results in sequence
- [x] remove `--json` and `--plain`, replace with `--output=[json|plain]`

# update *big changes*
- [x] rename `update` to `modify`
- [x] no more `--title`/`--author` etc. flags to modify metadata, instead steal wholesale from beets
  - so you specify the query with the same syntax as `caco ls`, with a `:`, and you specify the modification with `[field]=[value]`
  - e.g., `caco modify id:22..28 status=to-play` or `caco modify author:"erik alm" rating=5` or `caco modify id:33 notes="decent fights but very obscure progression"`
- [x] you can remove fields with `caco modify id:19..21 !tag` for example to remove all tags, or `caco modify id:19..21 !tag:"cacoward*"` to remove all tags that match the glob

## tag
- [x] folded into `modify` with `tag:` for queries and `tag=` for modifications
- [x] `caco ls --tags` shows all tags with counts (standalone read-only command)

## link
- [x] folded into `modify --link PATH`
- [x] default behavior: move file into managed wad cache (configurable to copy via config)
- [x] config key: `link_mode = "move"` (or `"copy"`)

# delete / restore
- [x] rename to `trash`
- [x] `caco trash id:30` puts it in the trash
- [x] `caco trash --list` shows trashed wads
- [x] `caco trash --purge [query]`
- [x] `caco trash --restore [query]`

# random
- [x] leave as-is

# import
- [x] leave as-is (source auto-detection stays)
- [x] auto-detect IWAD files via MD5 — no `--iwad` flag needed
- [x] when an IWAD is detected, manage it separately (copy to managed iwad dir, register in DB)
- [x] document the auto-detection behavior

# play
- [x] rename `--yes` to `--first` / `-1` (auto-select first match for scripting)
- [x] `--iwad FAMILY[/VARIANT]` to play an IWAD directly
  - `caco play --iwad doom2` → preferred variant (priority resolution)
  - `caco play --iwad doom2/v1.9` → exact variant
  - `caco play --iwad doom2/bfg` → BFG edition
- [x] `iwad:` prefix in query syntax is now a filter, not play syntax
  - `caco ls iwad:doom2` → list all PWADs requiring Doom 2
  - `caco ls iwad:tnt` → list all PWADs requiring TNT

# cache
- [x] leave as-is

# iwad
- [x] remove as standalone group
- [x] `iwad list` → `caco ls --iwad`
- iwad import → folded into `caco import` (auto-detected via MD5) — already works
- [x] `iwad remove` → `caco trash --iwad FAMILY [VARIANT]`

# config
- [x] `caco config` prints current configuration with defaults to stdout (pipeable)
- [x] `caco config -e` / `caco config --edit` remains the same
- [x] remove `--path`

# completions
- [x] update after CLI rework is done

# beaten / stats
- [x] merge `beaten` command group into `modify` and `info`
  - [x] `beaten+N` / `beaten-N` / `beaten=N` syntax in `modify`
  - [x] `beaten-TIMESTAMP` for removing by date
  - [x] `--notes`, `--stats-file/-s`, `--date` on modify
  - [x] standalone `--stats-file` attach (modify -s without beaten action)
  - [x] suppress auto-completion when beaten actions handle it
  - [x] completions section in `info` output (replaces "Times beaten: N")
  - [x] `--levelstats` / `--live` / `--plain` / `-b` on info
  - [x] deleted entire `beaten` command group (7 subcommands)
  - [x] kept helper functions in stats.py for reuse
  - [x] updated shell completions (fish/bash/zsh)
  - [x] tests for parsing, modify beaten, info levelstats
