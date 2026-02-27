# Dynamic Completions

- [x] Hidden `caco _complete <context>` subcommand that shell completion scripts call for live data.

## Data sources to complete
- [x] WAD titles/IDs for any QUERY argument
- [x] Tag names for `tag:` / `tag=`
- [x] IWAD families + variants for `iwad:` / `--iwad`
- [x] Status values for `status:`
- [x] Sort fields for `+`/`-` suffixes
- [x] Sourceport names for `--sourceport`
- [x] Field names for `modify` (`title=`, `author=`, etc.)
- [x] Query field prefixes (`id:`, `title:`, etc.)

## Notes
- Build on top of final command structure (implement after CLI rework)
- [x] Generate fish/bash/zsh scripts that call back to `caco _complete`
- [x] Fish completions updated to use `caco _complete` helpers
- [x] Bash completions using `_caco()` function + `caco _complete` helpers
- [x] Zsh completions using `_arguments` + `_describe` + `caco _complete` helpers
- [x] `caco completions [shell]` outputs hand-crafted scripts (not Click generic)
- [x] Scripts embedded in `src/caco/cli/_completion_scripts.py` for installed packages
- [x] Convenience copies at `completions/caco.bash` and `completions/_caco`
