"""Config and completions commands."""

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
@click.option("--path", is_flag=True, help="Print config file path")
@click.option("--edit", "-e", is_flag=True, help="Open config in $EDITOR")
def config(path: bool, edit: bool):
    """View or edit configuration.

    Without options, displays the current config file contents.
    Edit the config file directly for full control over all settings.
    """
    import os

    config_path = CONFIG_FILE

    if path:
        click.echo(config_path)
        return

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

# Directory for caching downloaded WADs
cache_dir = "~/.cache/caco/wads"

# idgames download mirror (0-4, see https://www.doomworld.com/idgames/api/)
download_mirror = 0

# Extra arguments to pass to sourceport
sourceport_args = []

# [list] section for customizing list output (coming soon)
# format = ["id", "title", "author", "status"]
# sort = "id+"
'''
            config_path.write_text(default_content)
            console.print(f"[dim]Created default config at {config_path}[/dim]")

        if not shutil.which(editor):
            err_console.print(f"[red]Editor '{editor}' not found on PATH[/red]")
            sys.exit(1)
        subprocess.run([editor, str(config_path)])
        return

    # Default: show config contents
    console.print(f"[dim]Config file: {config_path}[/dim]")
    console.print()

    if config_path.exists():
        from rich.panel import Panel
        from rich.syntax import Syntax

        content = config_path.read_text()
        syntax = Syntax(content, "toml", theme="monokai", line_numbers=False)
        console.print(Panel(syntax, title="config.toml", border_style="dim"))
    else:
        console.print("[dim]No config file exists. Run 'caco config --edit' to create one.[/dim]")


@cli.command()
@click.argument("shell", required=False, type=click.Choice(["bash", "fish", "zsh"]))
@click.option("--install", is_flag=True, help="Install completions to shell config")
def completions(shell: str | None, install: bool):
    """Generate or install shell completions."""
    import os

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

    # Get completion script
    from click.shell_completion import get_completion_class

    comp_cls = get_completion_class(shell)
    if not comp_cls:
        err_console.print(f"[red]Unsupported shell: {shell}[/red]")
        sys.exit(1)

    comp = comp_cls(cli, {}, "caco", "_CACO_COMPLETE")
    script = comp.source()

    if not install:
        # Just print the script
        console.print(script)
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
