# UX Review: caco egui GUI

## First Impression

The egui GUI is a clean, functional Doom library browser that covers the core workflow: browse your library, filter/sort, select a WAD, read about it, play it. The dark Doom-inspired theme is cohesive and the tab-based layout is immediately familiar. A user coming from the CLI would feel at home.

What's immediately clear: this is a library browser with tabs for status filtering, a search bar, and a list of WADs. What's immediately confusing: how to do anything beyond browsing. There's no menu bar, no right-click context menu, and the only hint that keyboard shortcuts exist is a small text line in the status bar (`j/k: nav  e: edit  d: delete  s: sessions  Enter: play`). If you don't notice that, you'd think clicking the four buttons in the detail panel is the only way to interact.

The Python Qt6 GUI feels like a desktop application. The egui GUI feels like a capable prototype.

## Critical Usability Issues

### 1. No right-click context menu anywhere

**What**: Right-clicking a WAD in the table or grid does nothing. The only ways to act on a WAD are: (a) keyboard shortcuts shown in the status bar, (b) buttons in the detail panel, or (c) double-clicking to play.

**The user's experience**: Every desktop application teaches users that right-click means "show me what I can do with this thing." When it does nothing, users assume the GUI is limited. They may never discover Edit, Delete, or Sessions without reading the status bar carefully.

**Suggested fix**: Add an egui context menu popup on right-click with: Play, Edit, Delete, Sessions, View Map Stats. This is the single highest-impact UX improvement.

### 2. No file picker dialogs

**What**: The Resources dialog (IWAD/id24 management) and Local File import both require manually typing or pasting file paths into a text input. There's no "Browse..." button that opens a system file picker.

**The user's experience**: "I want to register my doom2.wad. I have to... type the full path? In 2026?" This feels broken, not just inconvenient. Users expect file selection to use the OS file dialog.

**Suggested fix**: Use the `rfd` (Rusty File Dialogs) crate to open native file pickers. Add a "Browse..." button next to every path input field.

### 3. No companion file management in GUI

**What**: The Python GUI has a Companions tab in the Edit dialog where you can add, remove, enable, and disable companion files (DEH patches, music WADs, etc.). The egui GUI has no companion management at all — not in the edit dialog, not in the detail panel, nowhere.

**The user's experience**: A user who manages companion files from the CLI discovers they can't see or manage them in the GUI. The detail panel doesn't even show which companions are linked.

**Suggested fix**: Add a Companions tab to the Edit dialog (list with add/remove/enable/disable). Show linked companions in the detail panel.

### 4. No WAD Stats dialog

**What**: The Python GUI has a "Map Stats" button in the detail panel that opens a dialog showing per-map completion statistics (skill, time, kills/items/secrets) with import/export. The egui GUI has no equivalent.

**The user's experience**: A user who tracks per-map stats via the CLI (`caco info --levelstats`) has no way to view this data in the GUI. The stats tracking feature — one of caco's differentiators — is invisible.

**Suggested fix**: Add a WAD Stats dialog accessible from the detail panel. Show a table of maps with stats columns, plus import/export buttons.

### 5. No WAD Unavailable dialog

**What**: When a user tries to play a WAD whose cached file is missing or was never downloaded, the Python GUI shows a dialog offering to: open the source URL, link a local file, or cancel. The egui GUI presumably just shows an error.

**The user's experience**: "It says play failed but doesn't help me fix it. I know the WAD is on my disk, I just need to point caco at it."

**Suggested fix**: Add a Link dialog when play fails due to missing file. Offer: open source URL in browser, link local file (with file picker), cancel.

## Inconsistencies

### Sort controls: dropdown vs clickable headers

**The conflict**: Sorting is done via a dropdown + direction toggle button in the toolbar. But in the Python GUI (and in most desktop apps), you click a column header to sort by that column. The egui table has non-interactive headers.

**Which way to go**: Keep the dropdown (some users prefer explicit sort controls) but *also* make column headers clickable. Click once for ascending, click again for descending, click a third time to clear. This is the universal pattern users expect from tables.

### Grid navigation: vim keys only, no arrows

**The conflict**: The list view uses `j/k` for navigation (vim style). The grid view uses `h/j/k/l` (also vim style). But arrow keys don't work in either view. The Python GUI supports both.

**Which way to go**: Support both. Arrow keys are the universal default; vim keys are a power-user bonus. `h/j/k/l` and `←/↓/↑/→` should be equivalent.

### Timestamps: absolute dates everywhere

**The conflict**: "Last Played" shows `2026-03-15` in the table and detail panel. The Python GUI shows relative times like "2 days ago" which are more useful at a glance.

**Which way to go**: Show relative time in the table column and detail panel ("2d ago", "3 weeks ago"), with the full date as a tooltip or on hover.

## Missing Affordances

### No menu bar
The application has no File/View/Help menu. This means:
- No discoverable path to Cache, Resources, or Stats dialogs (they're toolbar buttons with no labels)
- No keyboard accelerators (Ctrl+Q to quit, Ctrl+E to edit, etc.)
- No Help > About or Help > Keyboard Shortcuts

Add a minimal menu bar: File (Import, Cache, Resources, Quit), View (List/Grid, Show Detail Panel, Stats), Help (Keyboard Shortcuts, About).

### No progress indicators for downloads
When playing a WAD that needs downloading, the status bar shows "Playing: {title}..." but no download progress. The Python GUI shows a progress bar. Use `indicatif`-style progress or an egui progress bar in the status area.

### No map completion progress in detail panel
The Python GUI shows a visual progress bar ("12/32 maps, 37%") in the detail panel stats section. The egui GUI just shows "Beaten: 2×". Add a progress bar or fraction display when `stats_snapshot` data is available.

### No tooltips on toolbar buttons
The Cache, Resources, and Stats buttons in the top bar have no labels and no tooltips. A new user has to click each one to find out what it does. Add tooltips at minimum, or text labels.

### Edit dialog not tabbed
All edit fields are in a single scrollable list. The Python GUI organizes them into 5 tabs (Metadata, Notes, Sourceport, Sources, Companions). The flat layout makes it hard to find specific settings, especially sourceport config vs metadata. Group related fields, even if you don't use tabs — at least use collapsible sections or visual headers.

### No clickable source links
Source URLs in the detail panel are plain text. In the Python GUI they're clickable (opens browser). Use `ui.hyperlink()` or `open::that()` to make URLs actionable.

## Minor Polish

- **Description truncated to 500 chars** — show full description in a scroll area instead
- **Grid card width hardcoded at 200px** — make responsive to window width or add a size slider
- **No rating stars in grid cards** — Python shows them, adds visual information
- **Status bar keyboard hints could use separators** — "j/k: nav | e: edit | d: delete" is easier to scan than space-separated
- **"No WAD selected" in detail panel** — add a subtle icon or instruction ("Select a WAD to see details")
- **Import tab number hints** — "1-5: switch source" is cryptic; consider labeling tabs "1. idgames" etc.
- **Dialog escape behavior** — closing Edit with Escape should warn if there are unsaved changes
- **No `G`/`gg` vim bindings** — j/k work but Shift+G (jump to bottom) and gg (jump to top) don't

## What Works Well

- **Tab-based status filtering** is clean and immediately understandable. The WAD count per tab is a nice touch.
- **Keyboard hints in the status bar** are a good idea — they just need to be more visible.
- **The theme** is cohesive. The Doom color palette (dark reds, oranges, grays) gives the app identity without being garish.
- **Session history dialog** is well done — the maps-played column with delta computation ("E1M1, E1M2 + 3 more") is genuinely useful.
- **Crash detection display** in sessions ("Crash (127)" in red) is clear and informative.
- **Filter debounce at 150ms** feels snappier than the Python GUI's 300ms.
- **Grid view with thumbnails** — the async thumbnail loading pipeline (cache → TITLEPIC → wiki scrape → placeholder) is the right architecture even if display needs polish.
- **Import source tabs with number keys** (1-5) is efficient once you know about it.
- **Delete dialog** showing session count and playtime is a smart "are you sure?" — it tells you what you'd lose.
