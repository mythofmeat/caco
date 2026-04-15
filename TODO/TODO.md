- [x] bug: the icon doesn't appear for the .desktop launcher or on the top-left corner of the program window. however, editing the icon path in the caco.desktop file to: `/usr/share/icons/hicolor/1024x1024/apps/caco.png` does make it work in both cases.
- [x] bug: trying to play a wad that is not downloadable via idgames no longer opens a popup allowing the user to visit the relevant doomwiki/doomforums page and provide a way to link the wad in the gui
- [x] bug: `caco import https://www.doomworld.com/idgames/?id=18184` returns result "Error: API error: Invalid Doomworld forum URL: https://www.doomworld.com/idgames/?id=18184" despite obviously being an idgames link and not a doomforums link
- [x] bug: when asked to download an idgames json, the url is formatted completely incorrectly
```
Workaround: open this URL in your browser and save the JSON:
https://www.doomworld.com/idgames/api/api.php?action=search&query=https%3A%2F%2Fwww.doomworld.com%2Fidgames%2F%3Fid%3D18184&type=title&out=json
```
- [x] bug: uzdoom needs an `.ini` file. not a `.cfg`. so the profile system does not work properly

- [ ] the doomwiki, doomworld forums, and idgames have all been under cloudflare protection for over a month. this genuinely makes large parts of this program useless. we need a way around it. really, really bad.

- [ ] feature: implement an MCP server for debugging and programmatic use purposes.

- [ ] bug: wad id:61 is did not get marked as completed despite having all levels exited (27/27)

- [ ] chore: the readme documentation is woefully out of date
