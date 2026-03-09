"""Tests for caco.stats_watcher module."""

import threading

import pytest

from caco.stats_watcher import (
    StatsWatcher,
    _WATCHER_FACTORIES,
    get_watcher,
    register_watcher,
    run_watcher_thread,
)


class TestRegistry:
    """Test watcher registry and lookup."""

    def test_get_watcher_returns_none_for_dsda(self, tmp_path):
        """dsda-doom has no watcher — uses passive stats reading."""
        assert get_watcher("dsda-doom", tmp_path) is None

    def test_get_watcher_returns_none_for_unknown(self, tmp_path):
        assert get_watcher("my-custom-port", tmp_path) is None

    def test_get_watcher_returns_helion_watcher(self, tmp_path):
        from caco.watchers.helion import HelionWatcher

        watcher = get_watcher("Helion", tmp_path)
        assert isinstance(watcher, HelionWatcher)

    def test_get_watcher_helion_lowercase(self, tmp_path):
        from caco.watchers.helion import HelionWatcher

        watcher = get_watcher("helion", tmp_path)
        assert isinstance(watcher, HelionWatcher)

    def test_register_and_retrieve(self, tmp_path):
        """Custom watcher can be registered and retrieved."""

        class DummyWatcher(StatsWatcher):
            def __init__(self, wad_data_dir):
                pass

            def start(self):
                pass

            def stop(self):
                pass

            def collect(self):
                return None

        register_watcher("_test_family", DummyWatcher)
        try:
            # Can't retrieve via get_watcher without a matching executable,
            # but the factory should be in the registry
            assert "_test_family" in _WATCHER_FACTORIES
        finally:
            del _WATCHER_FACTORIES["_test_family"]


class TestRunWatcherThread:
    """Test watcher thread lifecycle."""

    def test_starts_daemon_thread(self, tmp_path):
        """run_watcher_thread() starts a daemon thread."""
        started = threading.Event()

        class QuickWatcher(StatsWatcher):
            def __init__(self):
                self._stop = threading.Event()

            def start(self):
                started.set()
                self._stop.wait()

            def stop(self):
                self._stop.set()

            def collect(self):
                return None

        watcher = QuickWatcher()
        thread = run_watcher_thread(watcher)
        assert thread.daemon is True
        started.wait(timeout=2.0)
        assert started.is_set()
        watcher.stop()
        thread.join(timeout=2.0)
