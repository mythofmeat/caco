"""IWAD management commands: iwad list/add/remove/scan."""

import sqlite3
import sys
from pathlib import Path

import click
from rich.table import Table

from caco import db
from caco.config import get_iwad_dirs
from caco.db._iwads import (
    KNOWN_IWAD_FILENAMES,
    KNOWN_IWADS,
    _compute_md5,
    get_iwad_priority,
    identify_iwad,
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
        console.print("[dim]Use 'caco iwad scan' to discover IWADs or 'caco iwad add' to register one[/dim]")
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


@iwad_cmd.command(name="add")
@click.argument("path", type=click.Path(exists=True))
@click.option("--family", "iwad_family", help="Override auto-detected family name")
@click.option("--variant", "iwad_variant", help="Override auto-detected variant")
def iwad_add(path: str, iwad_family: str | None, iwad_variant: str | None):
    """Register an IWAD file.

    Auto-detects the IWAD family and variant by MD5 checksum, falling back
    to filename.  Use --family and --variant to override detection.

    \b
    Examples:
        caco iwad add ~/games/doom2.wad
        caco iwad add ~/wads/custom.wad --family doom2 --variant modded
    """
    resolved = Path(path).expanduser().resolve()
    abs_path = str(resolved)

    # Check if already registered by path
    existing = db.get_iwad_by_path(abs_path)
    if existing:
        err_console.print(
            f"[yellow]Already registered:[/yellow] {existing['family']}/{existing['variant']} ({abs_path})"
        )
        return

    # Compute MD5 and try to identify
    md5 = _compute_md5(resolved)
    detected = identify_iwad(resolved)

    if detected:
        family, variant, title = detected
    else:
        family = resolved.stem.lower()
        variant = "unknown"
        title = None

    # Overrides
    if iwad_family:
        family = iwad_family
    if iwad_variant:
        variant = iwad_variant

    # Check if (family, variant) already taken
    existing_variant = db.get_iwad_variant(family, variant)
    if existing_variant:
        err_console.print(
            f"[red]{family}/{variant} already registered[/red] (path: {existing_variant['path']})"
        )
        err_console.print("[dim]Use --variant to specify a different variant name[/dim]")
        sys.exit(1)

    try:
        db.add_iwad(family=family, variant=variant, path=abs_path, title=title, md5=md5)
    except sqlite3.IntegrityError:
        err_console.print(f"[red]Failed to register {family}/{variant} — already exists[/red]")
        sys.exit(1)

    label = f"{family}/{variant}"
    if title:
        console.print(f"[green]Registered:[/green] {label} — {title} ({abs_path})")
    else:
        console.print(f"[green]Registered:[/green] {label} ({abs_path})")


@iwad_cmd.command(name="remove")
@click.argument("family")
@click.argument("variant", required=False, default=None)
def iwad_remove(family: str, variant: str | None):
    """Unregister an IWAD by family (and optionally variant).

    Without a variant, removes ALL variants of the family (with warning).
    With a variant, removes only that specific variant.

    \b
    Examples:
        caco iwad remove doom2 bfg     # remove just the BFG variant
        caco iwad remove doom2          # remove all doom2 variants
    """
    if variant:
        removed = db.remove_iwad(family, variant)
        if removed:
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

        removed = db.remove_iwad(family)
        console.print(f"[green]Removed {removed} variant(s) of {family}[/green]")


@iwad_cmd.command(name="scan")
@click.option("--dir", "scan_dir", type=click.Path(exists=True, file_okay=False), help="Directory to scan (default: iwad_dirs)")
@click.option("--yes", "-y", is_flag=True, help="Register all discovered IWADs without prompting")
def iwad_scan(scan_dir: str | None, yes: bool):
    """Scan directories for known IWADs.

    Without --dir, scans all directories in the iwad_dirs config.
    Identifies IWADs by MD5 checksum, falling back to filename.

    \b
    Examples:
        caco iwad scan
        caco iwad scan --dir ~/games/iwads
        caco iwad scan --yes
    """
    if scan_dir:
        dirs = [Path(scan_dir).expanduser().resolve()]
    else:
        dirs = get_iwad_dirs()
        if not dirs:
            err_console.print("[yellow]No iwad_dirs configured[/yellow]")
            err_console.print("[dim]Set iwad_dirs in config: caco config iwad_dirs '[\"~/iwads\"]'[/dim]")
            err_console.print("[dim]Or use: caco iwad scan --dir /path/to/iwads[/dim]")
            return

    # Collect all .wad files
    discovered: list[tuple[Path, str, str, str, str]] = []  # (path, family, variant, title, md5)

    for d in dirs:
        if not d.is_dir():
            continue
        for wad_file in sorted(d.iterdir()):
            if not wad_file.is_file():
                continue
            if wad_file.suffix.lower() != ".wad":
                continue

            abs_path = str(wad_file.resolve())

            # Skip already registered
            if db.get_iwad_by_path(abs_path):
                continue

            md5 = _compute_md5(wad_file)

            # Try MD5 lookup
            if md5 in KNOWN_IWADS:
                family, variant, title = KNOWN_IWADS[md5]
            else:
                # Try filename fallback
                fname = wad_file.name.lower()
                if fname in KNOWN_IWAD_FILENAMES:
                    family, variant, title = KNOWN_IWAD_FILENAMES[fname]
                else:
                    continue  # Unknown file, skip

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
        # Register all
        registered = 0
        for wad_path, family, variant, title, md5 in discovered:
            try:
                db.add_iwad(family=family, variant=variant, path=str(wad_path.resolve()), title=title, md5=md5)
                registered += 1
            except sqlite3.IntegrityError:
                pass
        console.print(f"\n[green]Registered {registered} IWAD(s)[/green]")
    else:
        # Prompt for each
        console.print()
        registered = 0
        for wad_path, family, variant, title, md5 in discovered:
            if click.confirm(f"  Register {family}/{variant} ({title})?", default=True):
                try:
                    db.add_iwad(family=family, variant=variant, path=str(wad_path.resolve()), title=title, md5=md5)
                    registered += 1
                except sqlite3.IntegrityError:
                    err_console.print(f"  [yellow]Skipped (already exists): {family}/{variant}[/yellow]")
        if registered:
            console.print(f"\n[green]Registered {registered} IWAD(s)[/green]")
        else:
            console.print("\n[dim]No IWADs registered[/dim]")
