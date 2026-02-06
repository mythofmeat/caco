"""Tag management commands: tag add/remove/list."""

import click

from caco import db
from caco.cli import (
    cli,
    console,
    resolve_wad_query,
)


@cli.group()
def tag():
    """Manage tags."""
    pass


@tag.command(name="add")
@click.argument("query")
@click.argument("tags", nargs=-1, required=True)
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation for multi-WAD updates")
@click.option("--dry-run", is_flag=True, help="Show what would change without making changes")
def tag_add(query: str, tags: tuple[str, ...], yes: bool, dry_run: bool):
    """Add tags to WAD(s). QUERY: ID, ID range (3-6,9), or query (author:romero)."""
    wads = resolve_wad_query(query, mode="multiple", yes=yes)
    if not wads:
        return  # User cancelled

    if dry_run:
        console.print(f"\n[bold]Would add tag(s) {', '.join(tags)} to {len(wads)} WAD(s):[/bold]\n")
        for wad in wads[:10]:
            console.print(f"  [dim][{wad['id']}][/dim] {wad['title']}")
        if len(wads) > 10:
            console.print(f"  [dim]... and {len(wads) - 10} more[/dim]")
        console.print("\n[dim]No changes made (dry run)[/dim]")
        return

    for wad in wads:
        for t in tags:
            db.add_tag(wad["id"], t)

    console.print(f"[green]Added tag(s) to {len(wads)} WAD(s)[/green]")


@tag.command(name="remove")
@click.argument("query")
@click.argument("tags", nargs=-1, required=True)
@click.option("--yes", "-y", is_flag=True, help="Skip confirmation for multi-WAD updates")
@click.option("--dry-run", is_flag=True, help="Show what would change without making changes")
def tag_remove(query: str, tags: tuple[str, ...], yes: bool, dry_run: bool):
    """Remove tags from WAD(s). QUERY: ID, ID range (3-6,9), or query (author:romero)."""
    wads = resolve_wad_query(query, mode="multiple", yes=yes)
    if not wads:
        return  # User cancelled

    if dry_run:
        console.print(f"\n[bold]Would remove tag(s) {', '.join(tags)} from {len(wads)} WAD(s):[/bold]\n")
        for wad in wads[:10]:
            console.print(f"  [dim][{wad['id']}][/dim] {wad['title']}")
        if len(wads) > 10:
            console.print(f"  [dim]... and {len(wads) - 10} more[/dim]")
        console.print("\n[dim]No changes made (dry run)[/dim]")
        return

    for wad in wads:
        for t in tags:
            db.remove_tag(wad["id"], t)

    console.print(f"[green]Removed tag(s) from {len(wads)} WAD(s)[/green]")


@tag.command(name="list")
def tag_list():
    """List all tags."""
    tags = db.get_all_tags()
    if not tags:
        console.print("[dim]No tags[/dim]")
        return

    for t in tags:
        console.print(t)
