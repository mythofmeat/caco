"""Hidden _complete command for fast shell completions."""

import click

from caco.cli import cli
from caco import db
from caco.cli.parsing import SORT_FIELDS, MODIFY_FIELDS
from caco.sourceports import SOURCEPORT_FAMILIES


CONTEXTS = [
    "wads",
    "tags",
    "iwads",
    "statuses",
    "sort-fields",
    "sourceports",
    "modify-fields",
    "query-fields",
]


@cli.command("_complete", hidden=True)
@click.argument("context", type=click.Choice(CONTEXTS))
def complete_cmd(context: str) -> None:
    """Emit completion data for shell scripts."""
    if context == "wads":
        with db.get_connection() as conn:
            rows = conn.execute(
                "SELECT id, title FROM wads WHERE deleted_at IS NULL ORDER BY id"
            ).fetchall()
        for row in rows:
            print(f"{row['id']}\t{row['title']}")

    elif context == "tags":
        for tag in db.get_all_tags():
            print(tag)

    elif context == "iwads":
        seen_families: set[str] = set()
        for iwad in db.get_all_iwads():
            family = iwad["family"]
            if family not in seen_families:
                print(family)
                seen_families.add(family)
            print(f"{family}/{iwad['variant']}")

    elif context == "statuses":
        for status in db.Status:
            print(status.value)

    elif context == "sort-fields":
        for field in SORT_FIELDS:
            print(f"{field}+")
            print(f"{field}-")

    elif context == "sourceports":
        for family in SOURCEPORT_FAMILIES.values():
            for exe in family["executables"]:
                print(exe)

    elif context == "modify-fields":
        for field_name in MODIFY_FIELDS:
            print(f"{field_name}=")
            # Clear variant (skip tag — it has special !tag: syntax)
            if field_name != "tag":
                print(f"!{field_name}")

    elif context == "query-fields":
        from caco.cli import QUERY_FIELDS
        for field in QUERY_FIELDS:
            print(field)
