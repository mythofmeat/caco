"""Config and completions commands."""

import os
import shutil
import subprocess
import sys
from pathlib import Path

import click

from caco.config import CONFIG_FILE

from caco.cli import (
    cli,
    console,
    err_console,
)


@cli.command()
@click.option("--edit", "-e", is_flag=True, help="Open config in $EDITOR")
def config(edit: bool):
    """View or edit configuration.

    Without options, prints the raw config file to stdout (pipeable).
    """
    config_path = CONFIG_FILE

    if edit:
        editor = os.environ.get("EDITOR", os.environ.get("VISUAL", "nano"))
        # Create config file with defaults if it doesn't exist
        if not config_path.exists():
            config_path.parent.mkdir(parents=True, exist_ok=True)
            default_content = '''# Caco configuration file
# Edit these settings to customize caco behavior

# Sourceport executable (name on PATH or full path, e.g., gzdoom, dsda-doom)
sourceport = ""

# Path to your IWAD file (e.g., doom2.wad, or just "doom2" with iwad_dirs)
iwad = ""

# Directories to search for IWADs (allows using short names like "doom2")
iwad_dirs = []

# Path to the library database file
db_path = "~/.local/share/caco/library.db"

# Directory for downloaded WADs
cache_dir = "~/.local/share/caco/wads"

# idgames download mirror (0-4, see https://www.doomworld.com/idgames/api/)
download_mirror = 0

# Extra arguments to pass to sourceport
sourceport_args = []

# Link mode: "move" or "copy" (for caco modify --link)
link_mode = "move"

# Manage per-WAD data directories for isolated saves and stats
manage_data_dirs = true

# Automatically track per-map stats after play sessions
auto_stats = true
'''
            config_path.write_text(default_content)
            err_console.print(f"[dim]Created default config at {config_path}[/dim]")

        if not shutil.which(editor):
            err_console.print(f"[red]Editor '{editor}' not found on PATH[/red]")
            sys.exit(1)
        subprocess.run([editor, str(config_path)])
        return

    # Default: print raw config to stdout (pipeable)
    if config_path.exists():
        print(config_path.read_text(), end="")
    else:
        err_console.print("[dim]No config file exists. Run 'caco config -e' to create one.[/dim]")


@cli.command()
@click.argument("shell", required=False, type=click.Choice(["bash", "fish", "zsh"]))
@click.option("--install", is_flag=True, help="Install completions to shell config")
def completions(shell: str | None, install: bool):
    """Generate or install shell completions."""
    # Auto-detect shell if not specified
    if not shell:
        shell_path = os.environ.get("SHELL", "")
        if "fish" in shell_path:
            shell = "fish"
        elif "zsh" in shell_path:
            shell = "zsh"
        elif "bash" in shell_path:
            shell = "bash"
        else:
            err_console.print("[red]Could not detect shell. Specify: bash, fish, or zsh[/red]")
            sys.exit(1)

    # Get hand-crafted completion script
    from caco.cli._completion_scripts import FISH_SCRIPT, BASH_SCRIPT, ZSH_SCRIPT

    scripts = {"fish": FISH_SCRIPT, "bash": BASH_SCRIPT, "zsh": ZSH_SCRIPT}
    script = scripts[shell]

    if not install:
        # Use click.echo to avoid Rich interpreting [ ] as markup in shell code
        click.echo(script)
        return

    # Install to appropriate location
    home = Path.home()
    if shell == "fish":
        dest = home / ".config" / "fish" / "completions" / "caco.fish"
    elif shell == "zsh":
        dest = home / ".zfunc" / "_caco"
    elif shell == "bash":
        dest = home / ".local" / "share" / "bash-completion" / "completions" / "caco"
    else:
        err_console.print(f"[red]Unknown shell: {shell}[/red]")
        sys.exit(1)

    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_text(script)
    console.print(f"[green]Installed completions to {dest}[/green]")

    if shell == "zsh":
        console.print("[dim]Add 'fpath=(~/.zfunc $fpath)' to ~/.zshrc and run 'compinit'[/dim]")
    elif shell == "bash":
        console.print("[dim]Add 'source ~/.local/share/bash-completion/completions/caco' to ~/.bashrc[/dim]")
