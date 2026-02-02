# TUI Improvements

## Completed
- [x] Tab should only be for swapping between tabs
- [x] Pressing j/k should go up and down the list
- [x] Pressing `/` or `f` should activate the filter
- [x] Pressing `enter` on the filter should move the focus back to the main wad list view
- [x] Pressing `escape` when filtering should empty the filter and move the focus back to the main wad list view
- [x] Pressing `o` on the main list should open the sort dropdown
- [x] Launch a WAD by pressing enter from the main list
- [x] Tabs that can be switched between by using the tab key:
  - [x] Tab 1: All WADs
  - [x] Tab 2: status:playing
  - [x] Tab 3: status:to-play
  - [x] Tab 4: status:finished
  - [x] Tab 5: idgames search
- [x] Sort dropdown instead of keyboard cycling
- [x] Edit and update all WAD info, including sourceport and args for launching

## Remaining
- [ ] All CLI features usable from TUI (some import sources not yet integrated)

# GUI
- [ ] GUI for launching and managing WADs
- [ ] The GUI should be able to be called with `caco --gui`
- [ ] Downloaded WADs should have a thumbnail extracted from TITLEPIC in the WAD
  - There are various utilities that can do this. deutex is one, but there may be some python libraries that can extract WAD info
