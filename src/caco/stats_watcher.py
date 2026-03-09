"""Stats watcher framework — background thread monitors for stats file changes during play."""

import logging
import threading
from abc import ABC, abstractmethod
from pathlib import Path

logger = logging.getLogger(__name__)


class StatsWatcher(ABC):
    """Base class for sourceport-specific stats watchers.

    Runs in a background thread during a play session, monitoring for
    stats changes and returning accumulated results on completion.
    """

    @abstractmethod
    def start(self) -> None:
        """Begin watching. Called from the watcher thread. Blocks until stop()."""

    @abstractmethod
    def stop(self) -> None:
        """Signal the watcher to stop. Called from main thread. Must be thread-safe."""

    @abstractmethod
    def collect(self) -> str | None:
        """After stop()+join(), return levelstat.txt-format string, or None."""

    def extra_args(self) -> list[str]:
        """CLI args to inject before launch (e.g. -levelstat). Default: none."""
        return []


# Registry: family name -> factory callable(wad_data_dir) -> StatsWatcher
_WATCHER_FACTORIES: dict[str, type[StatsWatcher]] = {}


def register_watcher(family: str, factory: type[StatsWatcher]) -> None:
    """Register a watcher factory for a sourceport family."""
    _WATCHER_FACTORIES[family] = factory


_registered = False


def _ensure_watchers_registered() -> None:
    """Lazy-import watcher modules to populate the registry."""
    global _registered
    if _registered:
        return
    _registered = True
    try:
        import caco.watchers.helion  # noqa: F401
    except ImportError:
        logger.debug("Helion watcher module not available")


def get_watcher(executable: str, wad_data_dir: Path) -> StatsWatcher | None:
    """Look up and instantiate a watcher for the given sourceport.

    Returns None if no watcher is registered for this family.
    """
    from caco.sourceports import get_family_name

    _ensure_watchers_registered()

    family = get_family_name(executable)
    if not family or family not in _WATCHER_FACTORIES:
        return None

    factory = _WATCHER_FACTORIES[family]
    return factory(wad_data_dir)


def run_watcher_thread(watcher: StatsWatcher) -> threading.Thread:
    """Start a watcher in a daemon thread, return the thread handle."""
    thread = threading.Thread(target=watcher.start, daemon=True)
    thread.start()
    return thread
