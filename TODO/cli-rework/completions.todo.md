# Dynamic Completions

Hidden `caco _complete <context>` subcommand that shell completion scripts call for live data.

## Data sources to complete
- WAD titles/IDs for any QUERY argument
- Tag names for `tag:` / `tag=`
- IWAD families + variants for `iwad:` / `--iwad`
- Status values for `status:`
- Sort fields for `+`/`-` suffixes
- Sourceport names for `--sourceport`
- Field names for `modify` (`title=`, `author=`, etc.)

## Notes
- Build on top of final command structure (implement after CLI rework)
- Generate fish/bash/zsh scripts that call back to `caco _complete`
