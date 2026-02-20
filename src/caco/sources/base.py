"""Base class for source adapters."""

from __future__ import annotations


class BaseSource:
    """Mixin providing shared context-manager lifecycle for source adapters.

    Subclasses must set ``self.client`` to an object with a ``.close()`` method
    (typically a ``BaseHttpClient`` subclass).
    """

    def __enter__(self) -> BaseSource:
        return self

    def __exit__(self, *args: object) -> None:
        self.client.close()
