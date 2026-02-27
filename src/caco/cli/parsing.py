"""Argument classification and modification parsing for beets-style CLI."""

import json
from dataclasses import dataclass

from caco.db._query import normalize_status

SORT_FIELDS = ["id", "playtime", "rating", "created", "title", "author", "last_played", "year"]

# Fields that can be set via field=value in modify args
MODIFY_FIELDS = {
    "title": "title",
    "author": "author",
    "year": "year",
    "description": "description",
    "status": "status",
    "rating": "rating",
    "notes": "notes",
    "iwad": "custom_iwad",
    "sourceport": "custom_sourceport",
    "args": "custom_args",
    "idgames-id": "idgames_id",
    "version": "version",
    "tag": "tag",  # special handling
}


@dataclass
class ModifyAction:
    """A single modification action parsed from CLI args."""
    field: str       # DB column name (or "tag" for tag operations, or "beaten" for completion ops)
    value: str | None
    # "set", "clear", "add_tag", "remove_all_tags", "remove_tag",
    # "beaten_add", "beaten_remove", "beaten_remove_ts", "beaten_set"
    action: str
    pattern: str | None = None  # glob pattern for remove_tag


def _parse_sort_option(sort: str | None) -> tuple[str | None, bool]:
    """Parse sort option. Returns (field, descending).

    Suffix notation (preferred):
        'playtime'  -> ('playtime', True)   Bare field defaults to descending
        'title+'    -> ('title', False)     '+' suffix = ascending
        'title-'    -> ('title', True)      '-' suffix = descending

    Legacy prefix notation (still supported):
        '-title'    -> ('title', False)     '-' prefix = ascending (inverted!)
        '+title'    -> ('title', True)      '+' prefix = descending
    """
    if not sort:
        return None, True

    if sort.endswith("+"):
        return sort[:-1], False
    if sort.endswith("-"):
        return sort[:-1], True

    if sort.startswith("-"):
        return sort[1:], False
    if sort.startswith("+"):
        return sort[1:], True

    return sort, True


def extract_sort_from_args(args: tuple[str, ...] | list[str]) -> tuple[list[str], str | None]:
    """Extract inline sort term from argument list.

    Tokens ending in '+' or '-' where the prefix is a known sort field
    are treated as sort terms. Bare field names without a suffix are NOT
    sort terms (they're query terms).

    Returns (remaining_args, sort_string_or_None).
    Raises click.UsageError if multiple sort terms found.
    """
    import click

    remaining = []
    sort_term = None

    for arg in args:
        # Check if token ends with + or - and prefix is a known sort field
        if len(arg) > 1 and arg[-1] in ("+", "-"):
            field = arg[:-1]
            if field in SORT_FIELDS:
                if sort_term is not None:
                    raise click.UsageError(f"Multiple sort terms: '{sort_term}' and '{arg}'")
                sort_term = arg
                continue
        remaining.append(arg)

    return remaining, sort_term


def parse_modify_args(
    args: tuple[str, ...] | list[str],
) -> tuple[list[str], list[ModifyAction], str | None]:
    """Parse modify command arguments into query terms and actions.

    Syntax:
        field=value   -> set action
        tag=value     -> add_tag action
        !field        -> clear action (or remove_all_tags for !tag)
        !tag:pattern  -> remove matching tags

    Returns (query_terms, actions, sort_term).
    Raises click.UsageError on validation failures.
    """
    import click

    query_terms: list[str] = []
    actions: list[ModifyAction] = []
    sort_term: str | None = None

    for arg in args:
        # Check for inline sort (field+ or field-)
        if len(arg) > 1 and arg[-1] in ("+", "-"):
            field = arg[:-1]
            if field in SORT_FIELDS:
                if sort_term is not None:
                    raise click.UsageError(f"Multiple sort terms: '{sort_term}' and '{arg}'")
                sort_term = arg
                continue

        # Check for beaten actions: beaten+N, beaten-N/beaten-TIMESTAMP, beaten=N
        if arg.startswith("beaten") and len(arg) > 6 and arg[6] in ("+", "-", "="):
            op = arg[6]
            suffix = arg[7:]

            if op == "+":
                # beaten+ (implicit 1) or beaten+N
                if suffix == "":
                    n = 1
                else:
                    try:
                        n = int(suffix)
                    except ValueError:
                        raise click.UsageError(f"Invalid beaten count: '{suffix}' (must be a positive integer)")
                    if n <= 0:
                        raise click.UsageError(f"Beaten count must be positive, got: {n}")
                actions.append(ModifyAction(
                    field="beaten",
                    value=str(n),
                    action="beaten_add",
                ))
                continue

            if op == "=":
                try:
                    n = int(suffix)
                except ValueError:
                    raise click.UsageError(f"Invalid beaten count: '{suffix}' (must be a non-negative integer)")
                if n < 0:
                    raise click.UsageError(f"Beaten count cannot be negative, got: {n}")
                actions.append(ModifyAction(
                    field="beaten",
                    value=str(n),
                    action="beaten_set",
                ))
                continue

            if op == "-":
                if suffix == "":
                    raise click.UsageError("Bare 'beaten-' is ambiguous — use beaten-N (count) or beaten-TIMESTAMP")
                # Try as integer first
                try:
                    n = int(suffix)
                    if n <= 0:
                        raise click.UsageError(f"Beaten remove count must be positive, got: {n}")
                    actions.append(ModifyAction(
                        field="beaten",
                        value=str(n),
                        action="beaten_remove",
                    ))
                    continue
                except ValueError:
                    pass
                # If starts with 4 digits, treat as timestamp
                if len(suffix) >= 4 and suffix[:4].isdigit():
                    actions.append(ModifyAction(
                        field="beaten",
                        value=suffix,
                        action="beaten_remove_ts",
                    ))
                    continue
                raise click.UsageError(f"Invalid beaten- argument: '{suffix}' (expected count or timestamp)")

        # Check for clear/remove: !field or !tag:pattern
        if arg.startswith("!") and len(arg) > 1:
            rest = arg[1:]
            if ":" in rest:
                # !tag:pattern -> remove matching tags
                field_name, _, pattern = rest.partition(":")
                field_name = field_name.lower()
                if field_name == "tag":
                    actions.append(ModifyAction(
                        field="tag",
                        value=None,
                        action="remove_tag",
                        pattern=pattern,
                    ))
                    continue
                # !field:value is not valid for non-tag fields
                raise click.UsageError(f"Invalid clear syntax: '{arg}' (use !field to clear, !tag:pattern to remove tags)")
            else:
                field_name = rest.lower()
                if field_name == "tag":
                    actions.append(ModifyAction(
                        field="tag",
                        value=None,
                        action="remove_all_tags",
                    ))
                    continue
                if field_name in MODIFY_FIELDS:
                    db_col = MODIFY_FIELDS[field_name]
                    actions.append(ModifyAction(
                        field=db_col,
                        value=None,
                        action="clear",
                    ))
                    continue
                raise click.UsageError(f"Unknown field: '{field_name}'")

        # Check for set: field=value
        if "=" in arg and not arg.startswith("="):
            field_name, _, value = arg.partition("=")
            field_name = field_name.lower()
            if field_name in MODIFY_FIELDS:
                db_col = MODIFY_FIELDS[field_name]

                # Special handling for tag
                if field_name == "tag":
                    actions.append(ModifyAction(
                        field="tag",
                        value=value,
                        action="add_tag",
                    ))
                    continue

                # Validate specific fields
                if field_name == "status":
                    try:
                        value = normalize_status(value)
                        from caco.db import Status
                        Status(value)  # validate it's a real status
                    except ValueError:
                        raise click.UsageError(f"Invalid status: '{value}'")

                if field_name == "rating":
                    try:
                        r = int(value)
                        if r < 1 or r > 5:
                            raise ValueError
                    except ValueError:
                        raise click.UsageError(f"Rating must be 1-5, got: '{value}'")

                if field_name == "year":
                    try:
                        int(value)
                    except ValueError:
                        raise click.UsageError(f"Year must be an integer, got: '{value}'")

                if field_name == "args":
                    # Accept JSON array or space-separated
                    try:
                        json.loads(value)
                    except json.JSONDecodeError:
                        value = json.dumps(value.split())

                actions.append(ModifyAction(
                    field=db_col,
                    value=value,
                    action="set",
                ))
                continue

        # Everything else is a query term
        query_terms.append(arg)

    return query_terms, actions, sort_term
