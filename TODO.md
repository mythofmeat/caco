# General
- [ ] The description from idgames should also include the `textfile` element from the idgames api (https://www.doomworld.com/idgames/api/)
- [ ] i think we should just remove the `completed maps` feature entirely. not enough sourceports provide ways to access this data and it's not particularly useful.
- [ ] there should be a `caco random` command to print the info of a random WAD for use in scripting.
  - [ ] it should support filtering arguments, and a command like `caco play $(caco random status:to-play)` should work

# TUI Improvements
- [ ] able to choose a default start page and sort via the config file

# GUI
- [ ] GUI for launching and managing WADs
- [ ] The GUI should be able to be called with `caco --gui`
- [ ] Downloaded WADs should have a thumbnail extracted from TITLEPIC in the WAD
  - There are various utilities that can do this. deutex is one, but there may be some python libraries that can extract WAD info
