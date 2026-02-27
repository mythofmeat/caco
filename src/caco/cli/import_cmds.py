"""Import command: unified import with source flags."""

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


def _register_iwad(path, family: str, variant: str, title: str) -> None:
    """Register an IWAD file: copy to managed dir and add to DB."""
    import shutil
    from caco.config import get_iwad_dir
    from caco.utils import compute_md5
    from caco.db._iwads import add_iwad, get_iwad_variant, managed_iwad_filename

    existing = get_iwad_variant(family, variant)
    if existing:
        console.print(f"[yellow]Already registered:[/yellow] {title} ({family}/{variant})")
        return

    iwad_dir = get_iwad_dir()
    managed_rel = managed_iwad_filename(family, variant)
    dest = iwad_dir / managed_rel
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(str(path), str(dest))

    md5 = compute_md5(path)
    add_iwad(family, variant, str(dest), title=title, md5=md5)
    console.print(f"[green]Registered IWAD:[/green] {title} ({family}/{variant})")


def _register_id24(path, name: str, version: str, title: str) -> None:
    """Register an id24 WAD file: copy to managed dir and add to DB."""
    import shutil
    from caco.config import get_id24_dir
    from caco.utils import compute_md5
    from caco.db._id24 import add_id24, get_id24

    existing = get_id24(name)
    if existing:
        console.print(f"[yellow]Already registered:[/yellow] {title} ({name})")
        return

    id24_dir = get_id24_dir()
    id24_dir.mkdir(parents=True, exist_ok=True)
    dest = id24_dir / f"{name}.wad"
    shutil.copy2(str(path), str(dest))

    md5 = compute_md5(path)
    add_id24(name, str(dest), version=version, title=title, md5=md5)
    console.print(f"[green]Registered id24:[/green] {title} ({version})")


def _complete_llm_backends(ctx, param, incomplete):
    """Shell completion for LLM backends."""
    backends = ["claude-code", "openrouter", "anthropic", "openai"]
    return [b for b in backends if b.startswith(incomplete.lower())]


# =============================================================================
# Source-specific import helpers
# =============================================================================


def _do_auto_import(source: str, title: str | None, author: str | None,
                    year: int | None, tags: tuple[str, ...], force: bool,
                    multi: bool):
    """Auto-detect source type and dispatch to appropriate import."""
    from pathlib import Path

    source_type = _detect_source_type(source)

    if source_type == "doomwiki_url":
        from caco.sources.doomwiki import DoomwikiSource
        from urllib.parse import urlparse, unquote

        parsed = urlparse(source)
        path = unquote(parsed.path)
        if path.startswith("/wiki/"):
            page_title = path[6:].replace("_", " ")
        else:
            page_title = path.split("/")[-1].replace("_", " ")

        with DoomwikiSource() as wiki:
            wiki_entry = wiki.get(page_title)
            if not wiki_entry:
                err_console.print(f"[red]Page not found:[/red] {page_title}")
                return

            existing = db.find_duplicate(
                db.SourceType.DOOMWIKI,
                source_id=str(wiki_entry.page_id),
            )
            if existing and not force:
                console.print(f"[yellow]Already in library:[/yellow] {existing['title']} (ID: {existing['id']})")
                console.print("[dim]Use --force to import anyway[/dim]")
                return

            wad_id = wiki.import_wad(wiki_entry, tags=list(tags) if tags else None)
            console.print(f"[green]Imported:[/green] {wiki_entry.display_name} (ID: {wad_id})")

    elif source_type == "url":
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
        p = Path(source).resolve()

        # Check for IWAD
        iwad_info = db.identify_iwad(p)
        if iwad_info:
            _register_iwad(p, *iwad_info)
            return

        # Check for id24
        id24_info = db.identify_id24(p)
        if id24_info:
            _register_id24(p, *id24_info)
            return

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
        from caco.sources.idgames import IdgamesSource

        with IdgamesSource() as idgames:
            try:
                ig_entry = idgames.get(int(source))
            except Exception as e:
                err_console.print(f"[red]Failed to fetch idgames ID {source}: {e}[/red]")
                return

            wad_id_or_none = _check_and_import_entry(
                idgames, ig_entry, db.SourceType.IDGAMES,
                list(tags) if tags else None, force,
                source_id=str(ig_entry.id), filename=ig_entry.filename, author=ig_entry.author,
            )
            if wad_id_or_none:
                console.print(f"[green]Imported:[/green] {ig_entry.title} (ID: {wad_id_or_none})")
            else:
                return

    else:
        # idgames search
        _do_idgames_search(source, tags, force, multi)


def _do_idgames_import(query_or_id: str, tags: tuple[str, ...], force: bool, multi: bool):
    """Import from idgames archive (forced source)."""
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
        _do_idgames_search(query_or_id, tags, force, multi, source_override=source)


def _do_idgames_search(query: str, tags: tuple[str, ...], force: bool, multi: bool,
                        source_override=None):
    """Search idgames and import selected result(s)."""
    from caco.sources.idgames import IdgamesSource

    def _run_search(source):
        results = source.search(query)
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
                    source, entry, db.SourceType.IDGAMES, tags_list, force,
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
                source, entry, db.SourceType.IDGAMES,
                list(tags) if tags else None, force,
                source_id=str(entry.id), filename=entry.filename, author=entry.author,
            )
            if wad_id:
                console.print(f"[green]Imported:[/green] {entry.title} (ID: {wad_id})")

    if source_override:
        _run_search(source_override)
    else:
        with IdgamesSource() as source:
            _run_search(source)


def _do_doomwiki_import(query_or_title: str, tags: tuple[str, ...], force: bool, multi: bool):
    """Import from Doom Wiki (forced source)."""
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

        if multi and not _fzf_available():
            err_console.print("[red]--multi requires fzf to be installed[/red]")
            err_console.print("[dim]Install fzf: https://github.com/junegunn/fzf[/dim]")
            sys.exit(1)

        if _fzf_available():
            fzf_items = []
            for entry in results[:50]:
                year = str(entry.year) if entry.year else "????"
                fzf_items.append(f"{entry.display_name} by {entry.author or 'Unknown'} ({year})")

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
                wad_id = _wiki_check_import(source, entry, tags_list)
                if wad_id:
                    console.print(f"[green]Imported:[/green] {entry.display_name} (ID: {wad_id})")
                    imported += 1

            if multi and imported > 1:
                console.print(f"[green]Imported {imported} WAD(s)[/green]")

        else:
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


def _do_doomworld_import(url: str, tags: tuple[str, ...], title: str | None,
                         author: str | None, year: int | None, force: bool,
                         smart: bool, llm_backend: str | None, llm_model: str | None):
    """Import from Doomworld forum thread (forced source)."""
    from caco.sources.doomworld import DoomworldSource
    from caco.doomworld import (
        DoomworldError,
        complevel_name,
        iwad_display_name,
        sourceport_display_name,
    )

    # Validate URL
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

        # LLM-based extraction
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

        # Merge LLM metadata with regex-extracted data
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
            complevel=final_complevel,
        )
        console.print(f"[green]Imported:[/green] {thread.title} (ID: {wad_id})")

        # Show parsed metadata
        if thread.author:
            console.print(f"  [dim]Author:[/dim] {thread.author}")
        if thread.posted_date:
            console.print(f"  [dim]Posted:[/dim] {thread.posted_date[:10]}")

        # Show technical metadata
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
                for link in thread.download_links[:3]:
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


def _do_url_import(title: str, url: str, author: str | None, year: int | None,
                   tags: tuple[str, ...], description: str | None, force: bool):
    """Import from a URL (manual entry)."""
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


def _do_local_import(paths: tuple[str, ...], title: str | None, author: str | None,
                     year: int | None, tags: tuple[str, ...], force: bool):
    """Import local WAD file(s)."""
    from pathlib import Path as P

    if title and len(paths) > 1:
        err_console.print("[yellow]--title only works with single file imports[/yellow]")
        err_console.print("[dim]Titles will be inferred from filenames for batch imports[/dim]")

    imported = 0
    skipped = 0

    for path in paths:
        p = P(path).resolve()

        # Validate file existence (since we no longer use Click's Path(exists=True))
        if not p.exists():
            err_console.print(f"[red]File not found:[/red] {path}")
            continue

        # Check for IWAD
        iwad_info = db.identify_iwad(p)
        if iwad_info:
            _register_iwad(p, *iwad_info)
            imported += 1
            continue

        # Check for id24
        id24_info = db.identify_id24(p)
        if id24_info:
            _register_id24(p, *id24_info)
            imported += 1
            continue

        file_title = title if (title and len(paths) == 1) else _infer_title_from_filename(p.name)

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


# =============================================================================
# Import command (unified)
# =============================================================================


class MutuallyExclusiveSource(click.Option):
    """Custom Click option that enforces mutual exclusivity among source flags."""

    def handle_parse_result(self, ctx, opts, args):
        source_flags = ["idgames", "doomwiki", "doomworld", "local", "url"]
        current = self.name
        current_value = opts.get(current)

        if current_value:
            for flag in source_flags:
                if flag != current and opts.get(flag):
                    raise click.UsageError(
                        f"--{current.replace('_', '-')} and --{flag.replace('_', '-')} "
                        f"are mutually exclusive."
                    )

        return super().handle_parse_result(ctx, opts, args)


@cli.command(name="import")
@click.argument("source", nargs=-1)
@click.option("--idgames", is_flag=True, cls=MutuallyExclusiveSource, help="Force idgames source")
@click.option("--doomwiki", is_flag=True, cls=MutuallyExclusiveSource, help="Force Doom Wiki source")
@click.option("--doomworld", is_flag=True, cls=MutuallyExclusiveSource, help="Force Doomworld forum source")
@click.option("--local", is_flag=True, cls=MutuallyExclusiveSource, help="Force local file import")
@click.option("--url", "url_source", default=None, help="Import from URL (value is the download URL)")
@click.option("--title", "-t", help="Override title")
@click.option("--author", "-a", help="Author name")
@click.option("--year", type=int, help="Year released")
@click.option("--tag", "tags", multiple=True, help="Tags to add", shell_complete=_complete_tags)
@click.option("--force", "-f", is_flag=True, help="Import even if duplicate exists")
@click.option("--multi", "-m", is_flag=True, help="Allow multi-select (requires fzf)")
@click.option("--description", "-d", help="Description (for --url imports)")
@click.option("--smart", "-s", is_flag=True, help="Use LLM for metadata extraction (--doomworld)")
@click.option("--llm-backend", type=click.Choice(["claude-code", "openrouter", "anthropic", "openai"]),
              help="LLM backend (--doomworld --smart)", shell_complete=_complete_llm_backends)
@click.option("--llm-model", help="Model override for API backends (--doomworld --smart)")
@click.pass_context
def import_cmd(ctx, source: tuple[str, ...], idgames: bool, doomwiki: bool,
               doomworld: bool, local: bool, url_source: str | None,
               title: str | None, author: str | None, year: int | None,
               tags: tuple[str, ...], force: bool, multi: bool,
               description: str | None, smart: bool,
               llm_backend: str | None, llm_model: str | None):
    """Import WADs from various sources.

    By default, auto-detects the source type from the input. Use source
    flags to force a specific source.

    \b
    Auto-detect (default):
        caco import "scythe 2"                     # Search idgames
        caco import 19509                           # idgames file ID
        caco import https://doomwiki.org/wiki/Scythe
        caco import ~/Downloads/map.wad             # Local file
        caco import https://example.com/map.zip     # URL

    \b
    Forced source:
        caco import "scythe" --idgames             # Force idgames search
        caco import "Scythe" --doomwiki            # Force Doom Wiki search
        caco import URL --doomworld                # Force Doomworld forum
        caco import *.wad --local                  # Force local file(s)
        caco import "My WAD" --url https://...     # Manual URL import

    \b
    Options:
        --smart/-s        Use LLM for metadata (with --doomworld)
        --multi/-m        Multi-select from search results (requires fzf)
        --force/-f        Import even if duplicate exists
    """
    # No args at all -> show help
    if not source and not url_source:
        click.echo(ctx.get_help())
        return

    # --url: positional becomes title, url_source is the URL
    if url_source:
        if not source:
            err_console.print("[red]Title required for --url imports[/red]")
            err_console.print("[dim]Usage: caco import \"Title\" --url https://...[/dim]")
            sys.exit(1)
        import_title = " ".join(source)
        _do_url_import(import_title, url_source, author, year, tags, description, force)
        return

    # --local: all positional args are file paths
    if local:
        if not source:
            err_console.print("[red]File path(s) required for --local imports[/red]")
            sys.exit(1)
        _do_local_import(source, title, author, year, tags, force)
        return

    # --doomworld: first positional is the URL
    if doomworld:
        if not source:
            err_console.print("[red]Doomworld forum URL required[/red]")
            sys.exit(1)
        _do_doomworld_import(
            source[0], tags, title, author, year, force,
            smart, llm_backend, llm_model,
        )
        return

    # --idgames: positional becomes query/ID
    if idgames:
        if not source:
            err_console.print("[red]Query or ID required for --idgames imports[/red]")
            sys.exit(1)
        query = " ".join(source)
        _do_idgames_import(query, tags, force, multi)
        return

    # --doomwiki: positional becomes query/title
    if doomwiki:
        if not source:
            err_console.print("[red]Query or title required for --doomwiki imports[/red]")
            sys.exit(1)
        query = " ".join(source)
        _do_doomwiki_import(query, tags, force, multi)
        return

    # Default: auto-detect from first positional arg
    source_str = " ".join(source)
    _do_auto_import(source_str, title, author, year, tags, force, multi)
