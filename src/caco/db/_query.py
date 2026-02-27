"""Query parser and search functions."""

import shlex
from typing import Any

from caco.db._models import (
    AndGroup,
    OR_SEPARATOR,
    ParsedQuery,
    QueryTerm,
    STATUS_SHORTCUTS,
    SourceType,
)
from caco.db._connection import get_connection, _attach_tags, _fetch_tags_batch


def _glob_to_like(pattern: str) -> str:
    """Convert a glob pattern to SQL LIKE pattern.

    Handles:
    - * → % (match any characters)
    - ? → _ (match single character)
    - Escapes existing % and _ in the pattern
    """
    # Check if it's a glob pattern
    if "*" not in pattern and "?" not in pattern:
        # Not a glob, return as-is for exact match
        return pattern

    # Escape existing SQL wildcards
    result = pattern.replace("%", r"\%").replace("_", r"\_")
    # Convert glob to LIKE
    result = result.replace("*", "%").replace("?", "_")
    return result


def _is_glob_pattern(pattern: str) -> bool:
    """Check if a string contains glob wildcards."""
    return "*" in pattern or "?" in pattern


def _split_or_groups(query: str) -> list[str]:
    """Split query by OR_SEPARATOR respecting quoted strings."""
    sep_len = len(OR_SEPARATOR)
    parts = []
    current: list[str] = []
    i = 0
    in_quotes = False
    quote_char = None

    while i < len(query):
        char = query[i]

        # Handle quote state
        if char in '"\'':
            if not in_quotes:
                in_quotes = True
                quote_char = char
            elif char == quote_char:
                in_quotes = False
                quote_char = None
            current.append(char)
            i += 1
            continue

        # Check for OR_SEPARATOR pattern (not inside quotes)
        if not in_quotes and i + sep_len <= len(query):
            if query[i:i+sep_len] == OR_SEPARATOR:
                parts.append("".join(current).strip())
                current = []
                i += sep_len
                continue

        current.append(char)
        i += 1

    # Add final part
    if current:
        parts.append("".join(current).strip())

    return [p for p in parts if p]


def _parse_and_group(group_str: str) -> list[QueryTerm]:
    """Parse a single AND group into terms."""
    terms = []

    try:
        tokens = shlex.split(group_str)
    except ValueError:
        tokens = group_str.split()

    for token in tokens:
        negated = False

        # Check for negation prefix (- or ^ like beets)
        # ^ is useful when - would be interpreted as a CLI option
        if (token.startswith("-") or token.startswith("^")) and len(token) > 1:
            negated = True
            token = token[1:]

        # Check for field:value pattern
        if ":" in token:
            field, _, value = token.partition(":")
            field = field.lower()

            # Normalize field aliases
            if field == "name":
                field = "title"

            terms.append(QueryTerm(field=field, value=value, negated=negated))
        else:
            # Free text term
            terms.append(QueryTerm(field=None, value=token, negated=negated))

    return terms


def parse_query(query: str) -> ParsedQuery:
    """
    Parse beets-style query into structured form.

    Syntax:
        - Field queries: field:value, field:"quoted value"
        - Free text: word (searches title/author/description)
        - Negation: -field:value, -word
        - OR groups: term1 term2 , term3 term4
          (comma surrounded by spaces creates OR boundary)
        - Field aliases: name: -> title:

    Examples:
        status:playing author:alm          -> AND(status=playing, author=alm)
        status:playing , status:to-play    -> OR(status=playing, status=to-play)
        -status:finished -tag:cacoward*    -> AND(NOT status=finished, NOT tag=cacoward*)
        "ancient aliens" , scythe          -> OR(free_text="ancient aliens", free_text=scythe)

    Returns:
        ParsedQuery with or_groups containing AndGroups of QueryTerms.
    """
    if not query or not query.strip():
        return ParsedQuery(or_groups=[])

    # Split by " , " (comma with surrounding spaces) for OR groups
    or_parts = _split_or_groups(query)

    or_groups = []
    for part in or_parts:
        terms = _parse_and_group(part)
        if terms:
            or_groups.append(AndGroup(terms=terms))

    return ParsedQuery(or_groups=or_groups)


def normalize_status(value: str) -> str:
    """Normalize status value, expanding shortcuts.

    Public API — also used by CLI's StatusChoice.
    """
    lower = value.lower()
    return STATUS_SHORTCUTS.get(lower, lower)


def _build_term_sql(term: QueryTerm) -> tuple[str, list[Any]]:
    """Build SQL clause for a single QueryTerm."""
    clause = ""
    params: list[Any] = []

    if term.field is None:
        # Free text search
        clause = "(wads.title LIKE ? OR wads.author LIKE ? OR wads.description LIKE ?)"
        like = f"%{term.value}%"
        params = [like, like, like]

    elif term.field == "id":
        try:
            clause = "wads.id = ?"
            params = [int(term.value)]
        except ValueError:
            return "", []

    elif term.field == "title":
        clause = "wads.title LIKE ?"
        params = [f"%{term.value}%"]

    elif term.field == "author":
        clause = "wads.author LIKE ?"
        params = [f"%{term.value}%"]

    elif term.field == "year":
        try:
            clause = "wads.year = ?"
            params = [int(term.value)]
        except ValueError:
            return "", []

    elif term.field == "filename":
        clause = "wads.filename LIKE ?"
        params = [f"%{term.value}%"]

    elif term.field == "status":
        clause = "wads.status = ?"
        params = [normalize_status(term.value)]

    elif term.field == "source":
        clause = "wads.source_type = ?"
        params = [term.value.lower()]

    elif term.field == "tag":
        tag_pattern = term.value.lower()
        if _is_glob_pattern(tag_pattern):
            like_pattern = _glob_to_like(tag_pattern)
            clause = "wads.id IN (SELECT wad_id FROM tags WHERE tag LIKE ? ESCAPE '\\')"
            params = [like_pattern]
        else:
            # Substring match for non-glob — escape SQL wildcards in the literal
            escaped_tag = tag_pattern.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_")
            clause = "wads.id IN (SELECT wad_id FROM tags WHERE tag LIKE ? ESCAPE '\\')"
            params = [f"%{escaped_tag}%"]

    elif term.field == "iwad":
        clause = "wads.custom_iwad LIKE ?"
        params = [f"%{term.value}%"]

    elif term.field == "complevel":
        from caco.complevel import parse_complevel
        cl = parse_complevel(term.value)
        if cl is not None:
            clause = "wads.complevel = ?"
            params = [cl]
        else:
            return "", []

    elif term.field == "config":
        clause = "wads.custom_config LIKE ?"
        params = [f"%{term.value}%"]

    else:
        # Unknown field - treat as free text
        clause = "(wads.title LIKE ? OR wads.author LIKE ? OR wads.description LIKE ?)"
        like = f"%{term.value}%"
        params = [like, like, like]

    # Apply negation
    if term.negated and clause:
        clause = f"NOT ({clause})"

    return clause, params


def _build_query_sql(parsed: ParsedQuery) -> tuple[str, list[Any]]:
    """Build SQL WHERE clause from ParsedQuery."""
    if parsed.is_empty():
        return "", []

    or_clauses = []
    all_params: list[Any] = []

    for and_group in parsed.or_groups:
        and_clauses = []
        group_params: list[Any] = []

        for term in and_group.terms:
            clause, term_params = _build_term_sql(term)
            if clause:
                and_clauses.append(clause)
                group_params.extend(term_params)

        if and_clauses:
            or_clauses.append(f"({' AND '.join(and_clauses)})")
            all_params.extend(group_params)

    if not or_clauses:
        return "", []

    return " OR ".join(or_clauses), all_params


def search_wads(
    query: str | None = None,
    sort_by: str | None = None,
    sort_desc: bool = True,
    include_deleted: bool = False,
    limit: int = 0,
) -> list[dict[str, Any]]:
    """
    Search WADs with beets-style query syntax.

    Query supports:
        - Field queries: status:playing, author:romero, tag:megawad
        - Negation: -status:finished, -tag:cacoward*
        - OR groups: status:playing , status:to-play
        - Free text: scythe (searches title/author/description)
        - Glob patterns: tag:caco* (matches cacoward, etc.)
        - Status shortcuts: status:p (playing), status:f (finished), etc.

    Sort fields: playtime, rating, created, title, author, last_played, year

    Args:
        query: Beets-style query string
        sort_by: Field to sort by
        sort_desc: Sort descending (default True)
        include_deleted: If True, only show deleted WADs. If False (default),
                        exclude deleted WADs.
    """
    # Validate sort field before use in SQL construction
    allowed_sort_fields = {"id", "playtime", "rating", "created", "title", "author", "last_played", "year", "random"}
    if sort_by and sort_by not in allowed_sort_fields:
        raise ValueError(f"Invalid sort field: {sort_by}")

    conditions = []
    params: list[Any] = []

    # Filter by deleted status
    if include_deleted:
        conditions.append("wads.deleted_at IS NOT NULL")
    else:
        conditions.append("wads.deleted_at IS NULL")

    if query:
        parsed = parse_query(query)
        if not parsed.is_empty():
            query_sql, query_params = _build_query_sql(parsed)
            if query_sql:
                conditions.append(f"({query_sql})")
                params.extend(query_params)

    # SAFETY: conditions built by _build_query_sql() which uses parameterized queries
    where = " AND ".join(conditions) if conditions else "1=1"

    # Determine sort order
    direction = "DESC" if sort_desc else "ASC"
    reverse_dir = "ASC" if sort_desc else "DESC"  # For text fields where default should be opposite
    # For nullable fields: DESC = NULLS LAST (best first), ASC = NULLS FIRST (worst/empty first)
    nulls = "NULLS LAST" if sort_desc else "NULLS FIRST"
    reverse_nulls = "NULLS FIRST" if sort_desc else "NULLS LAST"

    # Map sort field to SQL expression (all values are hardcoded, not user-controlled)
    sort_map = {
        "id": f"wads.id {reverse_dir}",  # ID default ascending
        "playtime": f"COALESCE(SUM(sessions.duration_seconds), 0) {direction}",
        "rating": f"wads.rating {direction} {nulls}",
        "created": f"wads.created_at {direction}",
        "title": f"LOWER(wads.title) {reverse_dir}",  # Title default ascending (A-Z)
        "author": f"LOWER(wads.author) {reverse_dir} {reverse_nulls}",  # Author default ascending
        "last_played": f"MAX(sessions.started_at) {direction} {nulls}",
        "year": f"wads.year {direction} {nulls}",
        "random": "RANDOM()",
    }

    if sort_by and sort_by in sort_map:
        order_by = sort_map[sort_by]
        use_group_by = sort_by in ("playtime", "last_played")
    else:
        # Default sort: ID ascending (simplest, most predictable)
        order_by = "wads.id ASC"
        use_group_by = False

    limit_clause = f" LIMIT {int(limit)}" if limit > 0 else ""

    with get_connection() as conn:
        if use_group_by:
            # For playtime/last_played, need to JOIN with sessions
            sql = f"""
                SELECT wads.*
                FROM wads
                LEFT JOIN sessions ON sessions.wad_id = wads.id
                WHERE {where}
                GROUP BY wads.id
                ORDER BY {order_by}{limit_clause}
            """
        else:
            sql = f"SELECT wads.* FROM wads WHERE {where} ORDER BY {order_by}{limit_clause}"

        rows = conn.execute(sql, params).fetchall()

        results = [dict(row) for row in rows]

        # Batch-fetch tags for all results
        if results:
            wad_ids = [w["id"] for w in results]
            tags_by_wad = _fetch_tags_batch(conn, wad_ids)
            for wad in results:
                wad["tags"] = tags_by_wad.get(wad["id"], [])

        return results


def find_duplicate(
    source_type: SourceType,
    source_id: str | None = None,
    source_url: str | None = None,
    filename: str | None = None,
    author: str | None = None,
) -> dict[str, Any] | None:
    """
    Find a potential duplicate WAD in the library.

    Detection strategy (in priority order):
    1. idgames: exact match on source_id
    2. doomwiki: exact match on source_id (wiki page ID)
    3. doomworld: exact match on source_id (thread ID)
    4. URL/local: exact match on source_url
    5. Fallback: normalized filename + author match

    Returns the existing WAD dict if found, or None.
    """
    with get_connection() as conn:
        # Strategy 1-3: Match by source_type + source_id (idgames, doomwiki, doomworld)
        if source_id and source_type in (SourceType.IDGAMES, SourceType.DOOMWIKI, SourceType.DOOMWORLD):
            row = conn.execute(
                "SELECT * FROM wads WHERE source_type = ? AND source_id = ?",
                (source_type.value, source_id),
            ).fetchone()
            if row:
                return _attach_tags(conn, dict(row))

        # Strategy 4: Match by source_url (for URL and local)
        if source_url and source_type in (SourceType.URL, SourceType.LOCAL):
            row = conn.execute(
                "SELECT * FROM wads WHERE source_type = ? AND source_url = ?",
                (source_type.value, source_url),
            ).fetchone()
            if row:
                return _attach_tags(conn, dict(row))

        # Strategy 5: Fuzzy match on normalized filename + author
        if filename:
            # Normalize filename: lowercase, strip extension
            normalized = filename.lower()
            for ext in (".zip", ".wad", ".pk3", ".pk7"):
                if normalized.endswith(ext):
                    normalized = normalized[: -len(ext)]
                    break

            # Build query: filename LIKE pattern, optionally with author
            if author:
                row = conn.execute(
                    """
                    SELECT * FROM wads
                    WHERE LOWER(filename) LIKE ?
                    AND LOWER(author) LIKE ?
                    """,
                    (f"%{normalized}%", f"%{author.lower()}%"),
                ).fetchone()
            else:
                row = conn.execute(
                    "SELECT * FROM wads WHERE LOWER(filename) LIKE ?",
                    (f"%{normalized}%",),
                ).fetchone()

            if row:
                return _attach_tags(conn, dict(row))

        return None
