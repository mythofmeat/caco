# TUI Improvements
- [ ] i don't love the current interface. here's how i would change it:
  - [ ] there should be tabs that can be switched between by using the tab key
    - [ ] tab 1: all wads
    - [ ] tab 2: status:playing
    - [ ] tab 3: status:to-play
    - [ ] tab 4: status:finished
    - [ ] tab 5: idgames search
  - [ ] the current method of sorting is bad and unwieldy. there should just be a dropdown menu
  - [ ] please implement the ability to play a WAD by pressing enter from the main page
- [ ] There should be a way to edit and update all the info, including updating the sourceport and args for launching wads
- [ ] Basically I want all the CLI features to be usable from the TUI, including adding WADs

# GUI
- [ ] gui for launching and managing WADs
- [ ] The GUI should be able to be called with `caco --gui`
- [ ] downloaded WADs should have a thumbnail which is extracted directly from the TITLEPIC in the WAD
  - there are various utilities that can do this. deutex is one, but there may be some python libraries that can extract WAD info
