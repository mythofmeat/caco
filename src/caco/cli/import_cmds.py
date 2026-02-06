"""Import commands: import group and all subcommands."""

import sys

import click
from rich.table import Table

from caco import db
from caco.cli import (
    cli,
    console,
    err_console,
    _check_and_import_entry,
    _fzf_available,
    _fzf_select,
    _complete_tags,
)


# =============================================================================
# Internal helpers
# =============================================================================


def _detect_source_type(source: str) -> str:
    """Detect the type of import source.

    Returns: 'doomwiki_url', 'doomworld_url', 'url', 'local', 'idgames_id', or 'idgames_search'
    """
    from pathlib import Path

    # URL detection - check for specific sites first
    if source.startswith(("http://", "https://")):
        if "doomwiki.org/wiki/" in source:
            return "doomwiki_url"
        # Doomworld forum URLs (both new /forum/topic/ and old /vb/thread/ formats)
        if "doomworld.com/forum/topic/" in source or "doomworld.com/vb/thread/" in source:
            return "doomworld_url"
        return "url"

    # Local file detection (check if path exists)
    if Path(source).exists():
        return "local"

    # idgames ID detection (numeric)
    if source.isdigit():
        return "idgames_id"

    # Default to idgames search
    return "idgames_search"


def _infer_title_from_filename(filename: str) -> str:
    """Infer a reasonable title from a filename."""
    from pathlib import Path

    # Get base name without extension
    name = Path(filename).stem

    # Replace underscores and hyphens with spaces
    name = name.replace("_", " ").replace("-", " ")

    # Title case
    return name.title()


def _infer_title_from_url(url: str) -> str:
    """Infer a title from a URL by extracting the filename."""
    from urllib.parse import urlparse, unquote

    parsed = urlparse(url)
    path = unquote(parsed.path)

    # Get the filename part
    if "/" in path:
        filename = path.split("/")[-1]
    else:
        filename = path

    return _infer_title_from_filename(filename)


def _complete_llm_backends(ctx, param, incomplete):
    """Shell completion for LLM backends."""
    backends = ["claude-code", "openrouter", "anthropic", "openai"]
    return [b for b in backends if b.startswith(incomplete.lower())]


# =============================================================================
# Import group
# =============================================================================


@cli.group(name="import", invoke_without_command=True)
@click.pass_context
def import_cmd(ctx):
    """Import WADs from various sources.

    Without a subcommand, shows help. Use 'caco add <source>' for auto-detection.
    """
    if ctx.invoked_subcommand is None:
        click.echo(ctx.get_help())


@import_cmd.command(name="auto")
@click.argument("source")
@click.option("--title", "-t", help="Override title (inferred from filename if not provided)")
@click.option("--author", "-a", help="Author name")
@click.option("--year", "-y", type=int, help="Year released")
@click.option("--tag", "tags", multiple=True, help="Tags to add", shell_complete=_complete_tags)
@click.option("--force", "-f", is_flag=True, help="Import even if duplicate exists")
@click.option("--multi", "-m", is_flag=True, help="Allow multi-select for idgames search (requires fzf)")
def import_auto(source: str, title: str | None, author: str | None, year: int | None,
                tags: tuple[str, ...], force: bool, multi: bool):
    """Smart import that auto-detects source type.

    SOURCE can be:
    - A Doomwiki URL (doomwiki.org/wiki/...) - imports from Doom Wiki
    - A Doomworld forum URL (doomworld.com/forum/topic/...) - imports from forum
    - A URL (http/https) - imports from URL
    - A local file path - imports from local filesystem
    - A number - looks up idgames file ID
    - Text - searches idgames archive

    \b
    Examples:
        caco import auto ~/Downloads/mymap.wad
        caco import auto https://doomwiki.org/wiki/Scythe
        caco import auto https://www.doomworld.com/forum/topic/134292-myhousewad/
        caco import auto https://example.com/map.zip
        caco import auto 12345
        caco import auto "scythe 2"
    """
    from pathlib import Path

    source_type = _detect_source_type(source)

    if source_type == "doomwiki_url":
        # Doomwiki URL import
        from caco.sources.doomwiki import DoomwikiSource
        from urllib.parse import urlparse, unquote

        # Extract page title from URL: https://doomwiki.org/wiki/Page_Title
        parsed = urlparse(source)
        path = unquote(parsed.path)  # Handle URL-encoded chars like %3A for :
        if path.startswith("/wiki/"):
            page_title = path[6:].replace("_", " ")  # Remove /wiki/ prefix
        else:
            page_title = path.split("/")[-1].replace("_", " ")

        with DoomwikiSource() as wiki:
            entry = wiki.get(page_title)
            if not entry:
                err_console.print(f"[red]Page not found:[/red] {page_title}")
                return

            existing = db.find_duplicate(
                db.SourceType.DOOMWIKI,
                source_id=str(entry.page_id),
            )
            if existing and not force:
                console.print(f"[yellow]Already in library:[/yellow] {existing['title']} (ID: {existing['id']})")
                console.print("[dim]Use --force to import anyway[/dim]")
                return

            wad_id = wiki.import_wad(entry, tags=list(tags) if tags else None)
            console.print(f"[green]Imported:[/green] {entry.display_name} (ID: {wad_id})")

    elif source_type == "url":
        # URL import - infer title if not provided
        inferred_title = title or _infer_title_from_url(source)

        existing = db.find_duplicate(
            db.SourceType.URL,
            source_url=source,
            filename=inferred_title,
            author=author,
        )
        if existing and not force:
            console.print(f"[yellow]Already in library:[/yellow] {existing['title']} (ID: {existing['id']})")
            console.print("[dim]Use --force to import anyway[/dim]")
            return

        wad_id = db.add_wad(
            title=inferred_title,
            source_type=db.SourceType.URL,
            source_url=source,
            author=author,
            year=year,
            tags=list(tags) if tags else None,
        )
        console.print(f"[green]Added:[/green] {inferred_title} (ID: {wad_id})")

    elif source_type == "local":
        # Local file import
        p = Path(source).resolve()
        inferred_title = title or _infer_title_from_filename(p.name)

        existing = db.find_duplicate(
            db.SourceType.LOCAL,
            source_url=str(p),
            filename=p.name,
            author=author,
        )
        if existing and not force:
            console.print(f"[yellow]Already in library:[/yellow] {existing['title']} (ID: {existing['id']})")
            console.print("[dim]Use --force to import anyway[/dim]")
            return

        wad_id = db.add_wad(
            title=inferred_title,
            source_type=db.SourceType.LOCAL,
            source_url=str(p),
            filename=p.name,
            cached_path=str(p),
            author=author,
            year=year,
            tags=list(tags) if tags else None,
        )
        console.print(f"[green]Added:[/green] {inferred_title} (ID: {wad_id})")

    elif source_type == "doomworld_url":
        # Doomworld forum URL import
        from caco.sources.doomworld import DoomworldSource
        from caco.doomworld import (
            DoomworldError,
            complevel_name,
            iwad_display_name,
            sourceport_display_name,
        )

        with DoomworldSource() as doomworld:
            try:
                thread = doomworld.get(source)
            except DoomworldError as e:
                err_console.print(f"[red]Error: {e}[/red]")
                return

            if not thread:
                err_console.print(f"[red]Thread not found:[/red] {source}")
                return

            existing = db.find_duplicate(
                db.SourceType.DOOMWORLD,
                source_id=str(thread.thread_id),
                source_url=thread.thread_url,
            )
            if existing and not force:
                console.print(f"[yellow]Already in library:[/yellow] {existing['title']} (ID: {existing['id']})")
                console.print("[dim]Use --force to import anyway[/dim]")
                return

            wad_id = doomworld.import_wad(
                thread,
                tags=list(tags) if tags else None,
                title=title,
                author=author,
                year=year,
            )
            console.print(f"[green]Imported:[/green] {thread.title} (ID: {wad_id})")

            # Show technical metadata (Phase 2)
            if thread.has_technical_info:
                if thread.iwad:
                    console.print(f"  [dim]IWAD:[/dim] {iwad_display_name(thread.iwad)}")
                if thread.sourceport:
                    console.print(f"  [dim]Port:[/dim] {sourceport_display_name(thread.sourceport)}")
                if thread.complevel is not None:
                    console.print(f"  [dim]Complevel:[/dim] {complevel_name(thread.complevel)}")
                if thread.download_links:
                    console.print(f"  [dim]Downloads:[/dim] {len(thread.download_links)} link(s)")

    elif source_type == "idgames_id":
        # idgames ID lookup
        from caco.sources.idgames import IdgamesSource

        with IdgamesSource() as idgames:
            try:
                entry = idgames.get(int(source))
            except Exception as e:
                err_console.print(f"[red]Failed to fetch idgames ID {source}: {e}[/red]")
                return

            wad_id = _check_and_import_entry(
                idgames, entry, db.SourceType.IDGAMES,
                list(tags) if tags else None, force,
                source_id=str(entry.id), filename=entry.filename, author=entry.author,
            )
            if wad_id:
                console.print(f"[green]Imported:[/green] {entry.title} (ID: {wad_id})")
            else:
                return

    else:
        # idgames search - delegate to existing command's logic
        from caco.sources.idgames import IdgamesSource

        with IdgamesSource() as idgames:
            results = idgames.search(source)
            if not results:
                console.print("[dim]No results found[/dim]")
                return

            if multi and not _fzf_available():
                err_console.print("[red]--multi requires fzf to be installed[/red]")
                sys.exit(1)

            if _fzf_available():
                fzf_items = []
                for entry in results[:50]:
                    entry_year = entry.date[:4] if entry.date else "????"
                    fzf_items.append(f"{entry.title} by {entry.author or 'Unknown'} ({entry_year})")

                selected_indices = _fzf_select(
                    fzf_items,
                    prompt="Select WAD(s)" if multi else "Select WAD",
                    multi=multi,
                )

                if selected_indices is None:
                    return

                imported = 0
                tags_list = list(tags) if tags else None
                for idx in selected_indices:
                    entry = results[idx]
                    wad_id = _check_and_import_entry(
                        idgames, entry, db.SourceType.IDGAMES, tags_list, force,
                        source_id=str(entry.id), filename=entry.filename, author=entry.author,
                    )
                    if wad_id:
                        console.print(f"[green]Imported:[/green] {entry.title} (ID: {wad_id})")
                        imported += 1

                if multi and imported > 1:
                    console.print(f"[green]Imported {imported} WAD(s)[/green]")
            else:
                table = Table(title="Search Results")
                table.add_column("#", style="dim")
                table.add_column("ID", style="dim")
                table.add_column("Title", style="cyan")
                table.add_column("Author")
                table.add_column("Date")

                for i, entry in enumerate(results[:20], 1):
                    table.add_row(str(i), str(entry.id), entry.title, entry.author, entry.date or "-")

                console.print(table)

                choice = click.prompt("Enter number to import (or 0 to cancel)", type=int, default=0)
                if choice == 0 or choice > len(results):
                    return

                entry = results[choice - 1]
                wad_id = _check_and_import_entry(
                    idgames, entry, db.SourceType.IDGAMES,
                    list(tags) if tags else None, force,
                    source_id=str(entry.id), filename=entry.filename, author=entry.author,
                )
                if wad_id:
                    console.print(f"[green]Imported:[/green] {entry.title} (ID: {wad_id})")


@import_cmd.command(name="idgames")
@click.argument("query_or_id")
@click.option("--tag", "-t", "tags", multiple=True, help="Tags to add", shell_complete=_complete_tags)
@click.option("--force", "-f", is_flag=True, help="Import even if duplicate exists")
@click.option("--multi", "-m", is_flag=True, help="Allow selecting multiple WADs (requires fzf)")
def import_idgames(query_or_id: str, tags: tuple[str, ...], force: bool, multi: bool):
    """Import a WAD from idgames archive.

    Use fzf for interactive selection (if installed). Use --multi for batch import.
    """
    from caco.sources.idgames import IdgamesSource

    def _idgames_check_import(src, entry, tags_list):
        return _check_and_import_entry(
            src, entry, db.SourceType.IDGAMES, tags_list, force,
            source_id=str(entry.id), filename=entry.filename, author=entry.author,
        )

    with IdgamesSource() as source:
        # Try as ID first
        try:
            file_id = int(query_or_id)
            entry = source.get(file_id)
            wad_id = _idgames_check_import(source, entry, list(tags) if tags else None)
            if wad_id:
                console.print(f"[green]Imported:[/green] {entry.title} (ID: {wad_id})")
            return
        except ValueError:
            pass

        # Search
        results = source.search(query_or_id)
        if not results:
            console.print("[dim]No results found[/dim]")
            return

        # Multi-select requires fzf
        if multi and not _fzf_available():
            err_console.print("[red]--multi requires fzf to be installed[/red]")
            err_console.print("[dim]Install fzf: https://github.com/junegunn/fzf[/dim]")
            sys.exit(1)

        # Try fzf for selection
        if _fzf_available():
            # Format items for fzf: "Title by Author (Year)"
            fzf_items = []
            for entry in results[:50]:  # Allow more results with fzf
                year = entry.date[:4] if entry.date else "????"
                fzf_items.append(f"{entry.title} by {entry.author or 'Unknown'} ({year})")

            selected_indices = _fzf_select(
                fzf_items,
                prompt="Select WAD(s)" if multi else "Select WAD",
                multi=multi,
            )

            if selected_indices is None:
                return  # User cancelled

            # Import selected WADs
            imported = 0
            tags_list = list(tags) if tags else None
            for idx in selected_indices:
                entry = results[idx]
                wad_id = _idgames_check_import(source, entry, tags_list)
                if wad_id:
                    console.print(f"[green]Imported:[/green] {entry.title} (ID: {wad_id})")
                    imported += 1

            if multi and imported > 1:
                console.print(f"[green]Imported {imported} WAD(s)[/green]")

        else:
            # Fallback to numbered prompt
            table = Table(title="Search Results")
            table.add_column("#", style="dim")
            table.add_column("ID", style="dim")
            table.add_column("Title", style="cyan")
            table.add_column("Author")
            table.add_column("Date")

            for i, entry in enumerate(results[:20], 1):
                table.add_row(str(i), str(entry.id), entry.title, entry.author, entry.date or "-")

            console.print(table)

            choice = click.prompt("Enter number to import (or 0 to cancel)", type=int, default=0)
            if choice == 0 or choice > len(results):
                return

            entry = results[choice - 1]
            wad_id = _idgames_check_import(source, entry, list(tags) if tags else None)
            if wad_id:
                console.print(f"[green]Imported:[/green] {entry.title} (ID: {wad_id})")


@import_cmd.command(name="doomwiki")
@click.argument("query_or_title")
@click.option("--tag", "-t", "tags", multiple=True, help="Tags to add", shell_complete=_complete_tags)
@click.option("--force", "-f", is_flag=True, help="Import even if duplicate exists")
@click.option("--multi", "-m", is_flag=True, help="Allow selecting multiple WADs (requires fzf)")
def import_doomwiki(query_or_title: str, tags: tuple[str, ...], force: bool, multi: bool):
    """Import a WAD from Doom Wiki.

    Searches the Doom Wiki (doomwiki.org) for WADs matching the query.
    Only pages with a {{Wad}} infobox template are shown.

    Use fzf for interactive selection (if installed). Use --multi for batch import.

    \b
    Examples:
        caco import doomwiki "Scythe"
        caco import doomwiki "Eviternity" --tag megawad
        caco import doomwiki --multi "cacoward"
    """
    from caco.sources.doomwiki import DoomwikiSource

    def _wiki_check_import(src, entry, tags_list):
        return _check_and_import_entry(
            src, entry, db.SourceType.DOOMWIKI, tags_list, force,
            source_id=str(entry.page_id),
        )

    with DoomwikiSource() as source:
        # Try exact page title match first
        entry = source.get(query_or_title)
        if entry:
            wad_id = _wiki_check_import(source, entry, list(tags) if tags else None)
            if wad_id:
                console.print(f"[green]Imported:[/green] {entry.display_name} (ID: {wad_id})")
            return

        # Fall back to search
        results = source.search(query_or_title)
        if not results:
            console.print("[dim]No WAD pages found (only pages with {{Wad}} infobox are shown)[/dim]")
            return

        # Multi-select requires fzf
        if multi and not _fzf_available():
            err_console.print("[red]--multi requires fzf to be installed[/red]")
            err_console.print("[dim]Install fzf: https://github.com/junegunn/fzf[/dim]")
            sys.exit(1)

        # Try fzf for selection
        if _fzf_available():
            # Format items for fzf: "Title by Author (Year)"
            fzf_items = []
            for entry in results[:50]:  # Allow more results with fzf
                year = str(entry.year) if entry.year else "????"
                fzf_items.append(f"{entry.display_name} by {entry.author or 'Unknown'} ({year})")

            selected_indices = _fzf_select(
                fzf_items,
                prompt="Select WAD(s)" if multi else "Select WAD",
                multi=multi,
            )

            if selected_indices is None:
                return  # User cancelled

            # Import selected WADs
            imported = 0
            tags_list = list(tags) if tags else None
            for idx in selected_indices:
                entry = results[idx]
                wad_id = _wiki_check_import(source, entry, tags_list)
                if wad_id:
                    console.print(f"[green]Imported:[/green] {entry.display_name} (ID: {wad_id})")
                    imported += 1

            if multi and imported > 1:
                console.print(f"[green]Imported {imported} WAD(s)[/green]")

        else:
            # Fallback to numbered prompt
            table = Table(title="Search Results")
            table.add_column("#", style="dim")
            table.add_column("Title", style="cyan")
            table.add_column("Author")
            table.add_column("Year")

            for i, entry in enumerate(results[:20], 1):
                year = str(entry.year) if entry.year else "-"
                table.add_row(str(i), entry.display_name, entry.author or "-", year)

            console.print(table)

            choice = click.prompt("Enter number to import (or 0 to cancel)", type=int, default=0)
            if choice == 0 or choice > len(results):
                return

            entry = results[choice - 1]
            wad_id = _wiki_check_import(source, entry, list(tags) if tags else None)
            if wad_id:
                console.print(f"[green]Imported:[/green] {entry.display_name} (ID: {wad_id})")


@import_cmd.command(name="doomworld")
@click.argument("url")
@click.option("--tag", "-t", "tags", multiple=True, help="Tags to add", shell_complete=_complete_tags)
@click.option("--title", help="Override parsed title")
@click.option("--author", "-a", help="Override parsed author")
@click.option("--year", "-y", type=int, help="Override parsed year")
@click.option("--force", "-f", is_flag=True, help="Import even if duplicate exists")
@click.option("--smart", "-s", is_flag=True, help="Use LLM for intelligent metadata extraction")
@click.option("--llm-backend", type=click.Choice(["claude-code", "openrouter", "anthropic", "openai"]),
              help="LLM backend (auto-detects if not specified)", shell_complete=_complete_llm_backends)
@click.option("--llm-model", help="Model override for API backends (e.g., 'gpt-4' for openai)")
def import_doomworld(url: str, tags: tuple[str, ...], title: str | None,
                     author: str | None, year: int | None, force: bool,
                     smart: bool, llm_backend: str | None, llm_model: str | None):
    """Import a WAD from a Doomworld forum thread.

    Fetches metadata from the forum thread including title, author,
    date, and first post content. Use --smart for LLM-based extraction.

    \b
    Examples:
        caco import doomworld https://www.doomworld.com/forum/topic/134292-myhousewad/
        caco import doomworld URL --tag cacoward --tag megawad
        caco import doomworld URL --smart
        caco import doomworld URL --smart --llm-backend openrouter
    """
    from caco.sources.doomworld import DoomworldSource
    from caco.doomworld import (
        DoomworldError,
        complevel_name,
        iwad_display_name,
        sourceport_display_name,
    )

    # Validate URL - accept both new (/forum/topic/) and old (/vb/thread/) formats
    if "doomworld.com/forum/topic/" not in url and "doomworld.com/vb/thread/" not in url:
        err_console.print("[red]Invalid Doomworld forum URL[/red]")
        err_console.print("[dim]Expected: https://www.doomworld.com/forum/topic/{id}-{slug}/[/dim]")
        err_console.print("[dim]      or: https://www.doomworld.com/vb/thread/{id}[/dim]")
        sys.exit(1)

    with DoomworldSource() as doomworld:
        try:
            thread = doomworld.get(url)
        except DoomworldError as e:
            err_console.print(f"[red]Error: {e}[/red]")
            sys.exit(1)

        if not thread:
            err_console.print(f"[red]Thread not found:[/red] {url}")
            sys.exit(1)

        # LLM-based extraction (Phase 3)
        llm_metadata = None
        if smart:
            from caco.doomworld.llm import get_parser, LLMError, LLMNotAvailableError

            try:
                parser = get_parser(backend=llm_backend, model=llm_model)
                console.print(f"[dim]Using LLM backend: {parser.name}[/dim]")

                with console.status("[bold blue]Extracting metadata with LLM..."):
                    llm_metadata = parser.parse(thread.first_post_text)

                console.print("[green]LLM extraction complete[/green]")

            except LLMNotAvailableError as e:
                err_console.print(f"[yellow]LLM not available:[/yellow] {e}")
                err_console.print("[dim]Falling back to regex extraction[/dim]")
            except LLMError as e:
                err_console.print(f"[yellow]LLM error:[/yellow] {e}")
                err_console.print("[dim]Falling back to regex extraction[/dim]")

        # Check for duplicates
        existing = db.find_duplicate(
            db.SourceType.DOOMWORLD,
            source_id=str(thread.thread_id),
            source_url=thread.thread_url,
        )
        if existing and not force:
            console.print(f"[yellow]Already in library:[/yellow] {existing['title']} (ID: {existing['id']})")
            console.print("[dim]Use --force to import anyway[/dim]")
            return

        # Merge LLM metadata with regex-extracted data (LLM takes precedence where available)
        final_title = title
        final_author = author
        final_iwad = thread.iwad
        final_sourceport = thread.sourceport
        final_complevel = thread.complevel
        final_version = None

        if llm_metadata:
            if not final_title and llm_metadata.title:
                final_title = llm_metadata.title
            if not final_author and llm_metadata.author:
                final_author = llm_metadata.author
            if not final_iwad and llm_metadata.iwad:
                final_iwad = llm_metadata.iwad
            if not final_sourceport and llm_metadata.sourceport:
                final_sourceport = llm_metadata.sourceport
            if final_complevel is None and llm_metadata.complevel is not None:
                final_complevel = llm_metadata.complevel
            if llm_metadata.version:
                final_version = llm_metadata.version

        # Import with merged metadata
        wad_id = doomworld.import_wad(
            thread,
            tags=list(tags) if tags else None,
            title=final_title,
            author=final_author,
            year=year,
            version=final_version,
        )
        console.print(f"[green]Imported:[/green] {thread.title} (ID: {wad_id})")

        # Show parsed metadata
        if thread.author:
            console.print(f"  [dim]Author:[/dim] {thread.author}")
        if thread.posted_date:
            console.print(f"  [dim]Posted:[/dim] {thread.posted_date[:10]}")

        # Show technical metadata (Phase 2 regex + Phase 3 LLM)
        display_iwad = final_iwad or thread.iwad
        display_port = final_sourceport or thread.sourceport
        display_complevel = final_complevel if final_complevel is not None else thread.complevel

        if display_iwad or display_port or display_complevel is not None or thread.download_links:
            if display_iwad:
                console.print(f"  [dim]IWAD:[/dim] {iwad_display_name(display_iwad)}")
            if display_port:
                console.print(f"  [dim]Port:[/dim] {sourceport_display_name(display_port)}")
            if display_complevel is not None:
                console.print(f"  [dim]Complevel:[/dim] {complevel_name(display_complevel)}")
            if thread.download_links:
                console.print(f"  [dim]Downloads:[/dim] {len(thread.download_links)} link(s) found")
                for link in thread.download_links[:3]:  # Show first 3
                    console.print(f"    [blue]{link}[/blue]")
                if len(thread.download_links) > 3:
                    console.print(f"    [dim]... and {len(thread.download_links) - 3} more[/dim]")

        # Show LLM-specific metadata
        if llm_metadata:
            if llm_metadata.version:
                console.print(f"  [dim]Version:[/dim] {llm_metadata.version}")
            if llm_metadata.description:
                console.print(f"  [dim]Description:[/dim] {llm_metadata.description[:100]}...")
            if llm_metadata.map_count:
                console.print(f"  [dim]Maps:[/dim] {llm_metadata.map_count}")
            if llm_metadata.difficulty:
                console.print(f"  [dim]Difficulty:[/dim] {llm_metadata.difficulty}")
            if llm_metadata.themes:
                console.print(f"  [dim]Themes:[/dim] {', '.join(llm_metadata.themes)}")


@import_cmd.command(name="url")
@click.argument("title")
@click.argument("url")
@click.option("--author", "-a")
@click.option("--year", "-y", type=int)
@click.option("--tag", "-t", "tags", multiple=True, shell_complete=_complete_tags)
@click.option("--description", "-d")
@click.option("--force", "-f", is_flag=True, help="Import even if duplicate exists")
def import_url(title: str, url: str, author: str | None, year: int | None,
               tags: tuple[str, ...], description: str | None, force: bool):
    """Import a WAD from a URL (e.g., Doomworld forums)."""
    # Check for duplicate
    existing = db.find_duplicate(
        db.SourceType.URL,
        source_url=url,
        filename=title,
        author=author,
    )
    if existing and not force:
        console.print(f"[yellow]Already in library:[/yellow] {existing['title']} (ID: {existing['id']})")
        console.print("[dim]Use --force to import anyway[/dim]")
        return

    wad_id = db.add_wad(
        title=title,
        source_type=db.SourceType.URL,
        source_url=url,
        author=author,
        year=year,
        description=description,
        tags=list(tags) if tags else None,
    )
    console.print(f"[green]Added:[/green] {title} (ID: {wad_id})")


@import_cmd.command(name="local")
@click.argument("paths", nargs=-1, required=True, type=click.Path(exists=True))
@click.option("--title", "-t", help="Override title (only for single file imports)")
@click.option("--author", "-a")
@click.option("--year", "-y", type=int)
@click.option("--tag", "tags", multiple=True, help="Tags to add", shell_complete=_complete_tags)
@click.option("--force", "-f", is_flag=True, help="Import even if duplicate exists")
def import_local(paths: tuple[str, ...], title: str | None, author: str | None, year: int | None,
                 tags: tuple[str, ...], force: bool):
    """Import local WAD file(s).

    Supports multiple paths for batch import. Titles are inferred from filenames.

    \b
    Examples:
        caco import local ~/Downloads/mymap.wad
        caco import local *.wad --tag new --author "Me"
        caco import local map1.wad map2.pk3 --tag batch
    """
    from pathlib import Path as P

    if title and len(paths) > 1:
        err_console.print("[yellow]--title only works with single file imports[/yellow]")
        err_console.print("[dim]Titles will be inferred from filenames for batch imports[/dim]")

    imported = 0
    skipped = 0

    for path in paths:
        p = P(path).resolve()

        # Infer title from filename if not provided (or multiple files)
        file_title = title if (title and len(paths) == 1) else _infer_title_from_filename(p.name)

        # Check for duplicate
        existing = db.find_duplicate(
            db.SourceType.LOCAL,
            source_url=str(p),
            filename=p.name,
            author=author,
        )
        if existing and not force:
            console.print(f"[yellow]Skipped (duplicate):[/yellow] {p.name} \u2192 {existing['title']} (ID: {existing['id']})")
            skipped += 1
            continue

        wad_id = db.add_wad(
            title=file_title,
            source_type=db.SourceType.LOCAL,
            source_url=str(p),
            filename=p.name,
            cached_path=str(p),
            author=author,
            year=year,
            tags=list(tags) if tags else None,
        )
        console.print(f"[green]Added:[/green] {file_title} (ID: {wad_id})")
        imported += 1

    if len(paths) > 1:
        summary = f"[green]Imported {imported} WAD(s)[/green]"
        if skipped:
            summary += f" [dim]({skipped} skipped as duplicates)[/dim]"
        console.print(summary)
