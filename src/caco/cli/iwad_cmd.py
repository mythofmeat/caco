"""IWAD management commands: iwad list/import/remove."""

import shutil
import sqlite3
import sys
from pathlib import Path

import click
from rich.table import Table

from caco import db
from caco.config import get_iwad_dir
from caco.db._iwads import (
    _compute_md5,
    identify_iwad,
    managed_iwad_filename,
    remove_iwad_with_paths,
)

from caco.cli import cli, console, err_console


@cli.group(name="iwad")
def iwad_cmd():
    """Manage IWAD registry."""
    pass


@iwad_cmd.command(name="list")
@click.option("--plain", is_flag=True, help="Output as TSV (for scripting)")
def iwad_list(plain: bool):
    """List registered IWADs."""
    iwads = db.get_all_iwads()

    if plain:
        click.echo("Family\tVariant\tTitle\tPath\tMD5")
        for iwad in iwads:
            click.echo(
                f"{iwad['family']}\t{iwad['variant']}\t{iwad.get('title') or ''}"
                f"\t{iwad['path']}\t{iwad.get('md5') or ''}"
            )
        return

    if not iwads:
        console.print("[dim]No IWADs registered[/dim]")
        console.print("[dim]Use 'caco iwad import <path>' to import an IWAD file or directory[/dim]")
        return

    # Build a set of preferred (family, variant) pairs for marking with *
    preferred: set[tuple[str, str]] = set()
    families_seen: set[str] = set()
    for iwad in iwads:
        fam = iwad["family"]
        if fam not in families_seen:
            families_seen.add(fam)
            pref = db.get_iwad(fam)
            if pref:
                preferred.add((pref["family"], pref["variant"]))

    table = Table(title=f"Registered IWADs ({len(iwads)})")
    table.add_column("Family", style="cyan")
    table.add_column("Variant")
    table.add_column("Title")
    table.add_column("Path", style="dim")
    table.add_column("MD5", style="dim")

    for iwad in iwads:
        path_str = iwad["path"]
        exists = Path(path_str).exists()
        if not exists:
            path_str = f"[red]{path_str} (missing)[/red]"

        is_preferred = (iwad["family"], iwad["variant"]) in preferred
        variant_display = iwad["variant"]
        if is_preferred:
            variant_display = f"[bold green]{variant_display} *[/bold green]"

        table.add_row(
            iwad["family"],
            variant_display,
            iwad.get("title") or "-",
            path_str,
            (iwad.get("md5") or "-")[:12] + "..." if iwad.get("md5") else "-",
        )

    console.print(table)


def _import_single_iwad(
    source: Path,
    iwad_dir: Path,
    iwad_family: str | None,
    iwad_variant: str | None,
) -> bool:
    """Import a single IWAD file: identify, copy to managed dir, register.

    Returns True if successfully imported.
    """
    md5 = _compute_md5(source)
    detected = identify_iwad(source)

    if detected:
        family, variant, title = detected
    else:
        family = source.stem.lower()
        variant = "unknown"
        title = None

    # Overrides
    if iwad_family:
        family = iwad_family
    if iwad_variant:
        variant = iwad_variant

    # Check if (family, variant) already registered
    existing_variant = db.get_iwad_variant(family, variant)
    if existing_variant:
        err_console.print(
            f"[yellow]{family}/{variant} already registered[/yellow] (path: {existing_variant['path']})"
        )
        return False

    # Copy to managed directory
    dest_name = managed_iwad_filename(family, variant)
    dest = iwad_dir / dest_name
    dest.parent.mkdir(parents=True, exist_ok=True)

    if dest.exists():
        err_console.print(f"[yellow]File already exists: {dest}[/yellow]")
        return False

    shutil.copy2(str(source), str(dest))

    try:
        db.add_iwad(family=family, variant=variant, path=str(dest), title=title, md5=md5)
    except sqlite3.IntegrityError:
        # Clean up the copied file on failure
        dest.unlink(missing_ok=True)
        err_console.print(f"[red]Failed to register {family}/{variant} — already exists[/red]")
        return False

    label = f"{family}/{variant}"
    if title:
        console.print(f"[green]Imported:[/green] {label} — {title}")
    else:
        console.print(f"[green]Imported:[/green] {label}")
    console.print(f"  [dim]{dest}[/dim]")
    return True


@iwad_cmd.command(name="import")
@click.argument("path", type=click.Path(exists=True))
@click.option("--family", "iwad_family", help="Override auto-detected family name")
@click.option("--variant", "iwad_variant", help="Override auto-detected variant")
@click.option("--yes", "-y", is_flag=True, help="Import all discovered IWADs without prompting")
def iwad_import(path: str, iwad_family: str | None, iwad_variant: str | None, yes: bool):
    """Import IWAD file(s) into managed storage.

    PATH can be a single .wad file or a directory to scan.

    Auto-detects family and variant by MD5 checksum, falling back to filename.
    Files are copied to the managed IWAD directory (~/.local/share/caco/iwads/).

    \b
    Examples:
        caco iwad import ~/games/doom2.wad
        caco iwad import ~/games/doom2.wad --family doom2 --variant modded
        caco iwad import ~/iwads/                    # scan directory
        caco iwad import ~/iwads/ --yes              # auto-import all
    """
    resolved = Path(path).expanduser().resolve()
    iwad_dir = get_iwad_dir()

    if resolved.is_file():
        # Single file import
        if not _import_single_iwad(resolved, iwad_dir, iwad_family, iwad_variant):
            sys.exit(1)
    elif resolved.is_dir():
        # Directory scan
        discovered: list[tuple[Path, str, str, str, str]] = []

        for wad_file in sorted(resolved.iterdir()):
            if not wad_file.is_file():
                continue
            if wad_file.suffix.lower() != ".wad":
                continue

            md5 = _compute_md5(wad_file)
            detected = identify_iwad(wad_file)

            if detected:
                family, variant, title = detected
            else:
                continue  # Unknown file, skip in directory scan

            # Skip if (family, variant) already registered
            if db.get_iwad_variant(family, variant):
                continue

            discovered.append((wad_file, family, variant, title, md5))

        if not discovered:
            console.print("[dim]No new IWADs found[/dim]")
            return

        console.print(f"\n[bold]Discovered {len(discovered)} IWAD(s):[/bold]\n")
        for wad_path, family, variant, title, md5 in discovered:
            console.print(f"  [cyan]{family}[/cyan]/[bold]{variant}[/bold] — {title}")
            console.print(f"    [dim]{wad_path}[/dim]")

        if yes:
            registered = 0
            for wad_path, family, variant, title, md5 in discovered:
                if _import_single_iwad(wad_path, iwad_dir, iwad_family, iwad_variant):
                    registered += 1
            console.print(f"\n[green]Imported {registered} IWAD(s)[/green]")
        else:
            console.print()
            registered = 0
            for wad_path, family, variant, title, md5 in discovered:
                if click.confirm(f"  Import {family}/{variant} ({title})?", default=True):
                    if _import_single_iwad(wad_path, iwad_dir, iwad_family, iwad_variant):
                        registered += 1
            if registered:
                console.print(f"\n[green]Imported {registered} IWAD(s)[/green]")
            else:
                console.print("\n[dim]No IWADs imported[/dim]")
    else:
        err_console.print(f"[red]Path is neither a file nor a directory: {resolved}[/red]")
        sys.exit(1)


@iwad_cmd.command(name="remove")
@click.argument("family")
@click.argument("variant", required=False, default=None)
def iwad_remove(family: str, variant: str | None):
    """Unregister an IWAD by family (and optionally variant).

    Without a variant, removes ALL variants of the family (with warning).
    With a variant, removes only that specific variant.
    Also deletes the managed file if it lives inside the managed IWAD directory.

    \b
    Examples:
        caco iwad remove doom2 bfg     # remove just the BFG variant
        caco iwad remove doom2          # remove all doom2 variants
    """
    iwad_dir = get_iwad_dir()

    if variant:
        paths = remove_iwad_with_paths(family, variant)
        if paths:
            _delete_managed_files(paths, iwad_dir)
            console.print(f"[green]Removed:[/green] {family}/{variant}")
        else:
            err_console.print(f"[red]IWAD '{family}/{variant}' not found[/red]")
            sys.exit(1)
    else:
        # Count variants first for warning
        variants = db.get_family_iwads(family)
        if not variants:
            err_console.print(f"[red]No IWADs registered for family '{family}'[/red]")
            sys.exit(1)

        if len(variants) > 1:
            variant_names = ", ".join(v["variant"] for v in variants)
            if not click.confirm(
                f"Remove all {len(variants)} variants of {family} ({variant_names})?",
                default=False,
            ):
                return

        paths = remove_iwad_with_paths(family)
        _delete_managed_files(paths, iwad_dir)
        console.print(f"[green]Removed {len(paths)} variant(s) of {family}[/green]")


def _delete_managed_files(paths: list[str], iwad_dir: Path) -> None:
    """Delete files that live inside the managed IWAD directory."""
    resolved_iwad_dir = iwad_dir.resolve()
    for path_str in paths:
        p = Path(path_str)
        try:
            resolved = p.resolve()
            if p.exists() and resolved.is_relative_to(resolved_iwad_dir):
                p.unlink()
                # Clean up empty variant subdirectory
                if p.parent.resolve() != resolved_iwad_dir:
                    try:
                        p.parent.rmdir()
                    except OSError:
                        pass  # Not empty or already gone
        except OSError:
            pass
