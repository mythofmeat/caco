# -*- mode: python ; coding: utf-8 -*-
"""PyInstaller spec for caco — full build with GUI (PySide6 + Pillow)."""

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
    "caco.gui",
    "caco.gui.app",
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
    + collect_submodules("caco.gui")
    + collect_submodules("caco.db")
    + collect_submodules("textual.widgets")
    + collect_submodules("textual.css")
)

# PySide6 modules we don't use — exclude to cut ~100MB
_UNUSED_QT_MODULES = [
    "PySide6.Qt3DAnimation",
    "PySide6.Qt3DCore",
    "PySide6.Qt3DExtras",
    "PySide6.Qt3DInput",
    "PySide6.Qt3DLogic",
    "PySide6.Qt3DRender",
    "PySide6.QtBluetooth",
    "PySide6.QtCharts",
    "PySide6.QtConcurrent",
    "PySide6.QtDataVisualization",
    "PySide6.QtGraphs",
    "PySide6.QtHttpServer",
    "PySide6.QtLocation",
    "PySide6.QtMultimedia",
    "PySide6.QtMultimediaWidgets",
    "PySide6.QtNfc",
    "PySide6.QtPdf",
    "PySide6.QtPdfWidgets",
    "PySide6.QtPositioning",
    "PySide6.QtQml",
    "PySide6.QtQuick",
    "PySide6.QtQuick3D",
    "PySide6.QtQuickControls2",
    "PySide6.QtQuickWidgets",
    "PySide6.QtRemoteObjects",
    "PySide6.QtScxml",
    "PySide6.QtSensors",
    "PySide6.QtSerialBus",
    "PySide6.QtSerialPort",
    "PySide6.QtSpatialAudio",
    "PySide6.QtSql",
    "PySide6.QtStateMachine",
    "PySide6.QtTest",
    "PySide6.QtTextToSpeech",
    "PySide6.QtVirtualKeyboard",
    "PySide6.QtWebChannel",
    "PySide6.QtWebEngineCore",
    "PySide6.QtWebEngineQuick",
    "PySide6.QtWebEngineWidgets",
    "PySide6.QtWebSockets",
    "PySide6.QtXml",
]

a = Analysis(
    ["build_entry.py"],
    pathex=[str(SRC)],
    datas=textual_datas + caco_datas,
    hiddenimports=hidden_imports,
    excludes=[
        # Heavy stdlib modules we don't need
        "tkinter",
        "unittest",
        "xmlrpc",
    ] + _UNUSED_QT_MODULES,
)

# Strip .pyi stubs — not needed at runtime
a.datas = [d for d in a.datas if not d[0].endswith(".pyi")]

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
    name="caco-gui",
)
