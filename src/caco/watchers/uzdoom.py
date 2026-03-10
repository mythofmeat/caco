"""UZDoom sourceport stats watcher — injects ZScript mod and parses console log output.

UZDoom (and other ZDoom-family ports) support ZScript mods. This watcher:
1. Creates a small .pk3 mod with a ZScript EventHandler that prints stats to console
2. Injects the mod via -file and redirects console output to a log file via +logfile
3. Polls the log file for CACOSTATS lines and accumulates per-map stats
4. Returns results in levelstat.txt format for the stats tracking framework
"""

import logging
import re
import threading
import zipfile
from pathlib import Path

from caco.stats_watcher import StatsWatcher, register_watcher
from caco.wad_stats import MapStats, WadStats, format_stats

logger = logging.getLogger(__name__)

# Doom runs at 35 tics per second
_TICS_PER_SECOND = 35

# ZScript mod that reports stats via Console.PrintfEx with a unique prefix.
# Uses CacoStatsReporter to avoid name conflicts with other mods.
# AddEventHandlers in MAPINFO is additive (won't replace existing handlers).
# PRINT_LOG suppresses on-screen notifications while still writing to +logfile.
_ZSCRIPT_SOURCE = """\
version "4.0"

class CacoStatsReporter : EventHandler
{
    int tickCounter;

    override void WorldTick()
    {
        tickCounter++;
        // Report every 35 ticks (once per second)
        if (tickCounter % 35 == 0)
        {
            ReportStats();
        }
    }

    override void WorldUnloaded(WorldEvent e)
    {
        ReportStats();
    }

    void ReportStats()
    {
        int sk = G_SkillPropertyInt(SKILLP_ACSReturn);
        Console.PrintfEx(PRINT_LOG, "CACOSTATS|%s|%d|%d|%d/%d|%d/%d|%d/%d",
            level.MapName,
            sk,
            level.maptime,
            level.killed_monsters, level.total_monsters,
            level.found_items, level.total_items,
            level.found_secrets, level.total_secrets
        );
    }
}
"""

_MAPINFO_SOURCE = """\
GameInfo
{
    AddEventHandlers = "CacoStatsReporter"
}
"""

# Parse: CACOSTATS|MAP01|3|1050|50/100|20/30|3/5
_STATS_RE = re.compile(
    r"CACOSTATS\|(\S+)\|(-?\d+)\|(\d+)"
    r"\|(\d+)/(\d+)\|(\d+)/(\d+)\|(\d+)/(\d+)"
)


def _get_mod_dir() -> Path:
    """Get the directory for caco-managed mods."""
    from caco.config import DB_DIR

    return DB_DIR / "mods"


def _ensure_pk3(mod_dir: Path | None = None) -> Path:
    """Create or update the stats reporter .pk3 mod."""
    if mod_dir is None:
        mod_dir = _get_mod_dir()
    mod_dir.mkdir(parents=True, exist_ok=True)
    pk3_path = mod_dir / "caco_stats_reporter.pk3"

    with zipfile.ZipFile(pk3_path, "w", zipfile.ZIP_DEFLATED) as zf:
        zf.writestr("zscript.zs", _ZSCRIPT_SOURCE)
        zf.writestr("MAPINFO", _MAPINFO_SOURCE)

    return pk3_path


class UZDoomWatcher(StatsWatcher):
    """Watches UZDoom's console log for stats from injected ZScript mod.

    Injects a small .pk3 mod that outputs CACOSTATS lines via Console.PrintfEx
    (PRINT_LOG level — log-only, no on-screen notifications), and redirects
    console output to a log file via +logfile. The watcher polls the log file
    for new CACOSTATS lines and accumulates per-map stats.
    """

    def __init__(self, wad_data_dir: Path, mod_dir: Path | None = None):
        self._wad_data_dir = wad_data_dir
        self._mod_dir = mod_dir
        self._log_path = wad_data_dir / "uzdoom_stats.log"
        self._stop_event = threading.Event()
        self._poll_interval: float = 1.0
        self._file_pos: int = 0
        # Latest stats per map: map_name -> (skill, maptime_tics, kills,
        #   total_kills, items, total_items, secrets, total_secrets)
        self._map_stats: dict[str, tuple[int, int, int, int, int, int, int, int]] = {}

    def extra_args(self) -> list[str]:
        """Inject the ZScript pk3 mod and logfile redirection."""
        pk3_path = _ensure_pk3(self._mod_dir)
        return ["-file", str(pk3_path), "+logfile", str(self._log_path)]

    def start(self) -> None:
        """Poll the log file for CACOSTATS lines until stopped."""
        # GZDoom's +logfile truncates the file, so always start from 0.
        # The file may not exist yet if the sourceport hasn't started writing.
        self._file_pos = 0

        while not self._stop_event.wait(self._poll_interval):
            self._read_new_lines()

    def stop(self) -> None:
        """Signal stop and do final read."""
        self._stop_event.set()
        self._read_new_lines()

    def collect(self) -> str | None:
        """Return accumulated stats as levelstat.txt-format string."""
        if not self._map_stats:
            return None

        maps = []
        cumulative_secs = 0.0
        for lump, (
            skill,
            maptime_tics,
            kills,
            total_kills,
            items,
            total_items,
            secrets,
            total_secrets,
        ) in self._map_stats.items():
            time_secs = maptime_tics / _TICS_PER_SECOND
            cumulative_secs += time_secs
            maps.append(
                MapStats(
                    lump=lump,
                    time_secs=time_secs,
                    total_time_secs=cumulative_secs,
                    kills=kills,
                    total_kills=total_kills,
                    items=items,
                    total_items=total_items,
                    secrets=secrets,
                    total_secrets=total_secrets,
                    best_skill=skill + 1 if skill >= 0 else 4,
                )
            )

        stats = WadStats(format="levelstat_txt", maps=maps)
        return format_stats(stats)

    def _read_new_lines(self) -> None:
        """Read new lines from the log file and parse CACOSTATS entries."""
        try:
            if not self._log_path.exists():
                return

            with open(self._log_path, "r") as f:
                f.seek(self._file_pos)
                new_data = f.read()
                self._file_pos = f.tell()

            if not new_data:
                return

            for line in new_data.splitlines():
                m = _STATS_RE.search(line)
                if m:
                    self._map_stats[m.group(1)] = (
                        int(m.group(2)),  # skill (0-indexed)
                        int(m.group(3)),  # maptime_tics
                        int(m.group(4)),  # kills
                        int(m.group(5)),  # total_kills
                        int(m.group(6)),  # items
                        int(m.group(7)),  # total_items
                        int(m.group(8)),  # secrets
                        int(m.group(9)),  # total_secrets
                    )
        except OSError:
            logger.debug("Failed to read UZDoom log file", exc_info=True)


register_watcher("uzdoom", UZDoomWatcher)
