# -*- mode: python ; coding: utf-8 -*-
"""PyInstaller spec for caco — CLI + TUI build (no PySide6/GUI)."""

from pathlib import Path
from PyInstaller.utils.hooks import collect_data_files, collect_submodules

SRC = Path("src")

# Textual ships CSS, default themes, etc. that must be bundled
textual_datas = collect_data_files("textual")

# Our own data files
caco_datas = [
    (str(SRC / "caco" / "tui" / "styles.tcss"), "caco/tui"),
]

# Hidden imports — lazy/dynamic imports PyInstaller can't trace
hidden_imports = [
    # CLI submodules registered at bottom of cli/__init__.py
    "caco.cli.library",
    "caco.cli.import_cmds",
    "caco.cli.play_cmd",
    "caco.cli.cache",
    "caco.cli.config_cmd",
    "caco.cli.stats",
    "caco.cli.saves_cmd",
    "caco.cli.demos_cmd",
    "caco.cli.complete",
    "caco.cli.profile_cmd",
    "caco.cli.companion_cmd",
    # Lazy imports
    "caco.tui",
    "caco.tui.app",
    "caco.watchers.helion",
    "caco.watchers.uzdoom",
    # Source adapters (used via import_service)
    "caco.sources.idgames",
    "caco.sources.doomwiki",
    "caco.sources.doomworld",
    # Services
    "caco.services.import_service",
    "caco.services.resource_service",
    "caco.services.companion_service",
] + (
    collect_submodules("caco.tui")
    + collect_submodules("caco.db")
    + collect_submodules("textual.widgets")
    + collect_submodules("textual.css")
)

a = Analysis(
    ["build_entry.py"],
    pathex=[str(SRC)],
    datas=textual_datas + caco_datas,
    hiddenimports=hidden_imports,
    excludes=[
        # Exclude GUI stack entirely for the CLI+TUI build
        "PySide6",
        "PIL",
        "Pillow",
        "caco.gui",
        # Heavy stdlib modules we don't need
        "tkinter",
        "unittest",
        "xmlrpc",
    ],
)

pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    [],
    name="caco",
    console=True,
    exclude_binaries=True,
)

coll = COLLECT(
    exe,
    a.binaries,
    a.datas,
    name="caco",
)
