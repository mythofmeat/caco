"""Base class for source adapters."""

from __future__ import annotations

from typing import Any, TypeVar

_T = TypeVar("_T", bound="BaseSource")


class BaseSource:
    """Mixin providing shared context-manager lifecycle for source adapters.

    Subclasses must set ``self.client`` to an object with a ``.close()`` method
    (typically a ``BaseHttpClient`` subclass).
    """

    client: Any  # Concrete type set by subclass __init__

    def __enter__(self: _T) -> _T:
        return self

    def __exit__(self, *args: object) -> None:
        self.client.close()
