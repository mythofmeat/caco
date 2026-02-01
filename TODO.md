# BUGS
```sh
eshen@meat ~/.l/r/caco (version-tracking) [1]> caco import local (realpath /mnt/moon/Games/Doom/wad/archive/\[2025\]\ Belot\ \(Liah\ K.\)/Belot.wad )
Traceback (most recent call last):
  File "/home/eshen/.local/bin/caco", line 10, in <module>
    sys.exit(cli())
             ~~~^^
  File "/home/eshen/.local/repos/caco/.venv/lib/python3.14/site-packages/click/core.py", line 1485, in __call__
    return self.main(*args, **kwargs)
           ~~~~~~~~~^^^^^^^^^^^^^^^^^
  File "/home/eshen/.local/repos/caco/.venv/lib/python3.14/site-packages/click/core.py", line 1406, in main
    rv = self.invoke(ctx)
  File "/home/eshen/.local/repos/caco/.venv/lib/python3.14/site-packages/click/core.py", line 1873, in invoke
    return _process_result(sub_ctx.command.invoke(sub_ctx))
                           ~~~~~~~~~~~~~~~~~~~~~~^^^^^^^^^
  File "/home/eshen/.local/repos/caco/.venv/lib/python3.14/site-packages/click/core.py", line 1873, in invoke
    return _process_result(sub_ctx.command.invoke(sub_ctx))
                           ~~~~~~~~~~~~~~~~~~~~~~^^^^^^^^^
  File "/home/eshen/.local/repos/caco/.venv/lib/python3.14/site-packages/click/core.py", line 1269, in invoke
    return ctx.invoke(self.callback, **ctx.params)
           ~~~~~~~~~~^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  File "/home/eshen/.local/repos/caco/.venv/lib/python3.14/site-packages/click/core.py", line 824, in invoke
    return callback(*args, **kwargs)
  File "/home/eshen/.local/repos/caco/src/caco/cli.py", line 1998, in import_local
    wad_id = db.add_wad(
        title=file_title,
    ...<6 lines>...
        tags=list(tags) if tags else None,
    )
TypeError: add_wad() got an unexpected keyword argument 'cached_path'
```

# New Features *(ordered by priority)*

## Version Tracking
- [ ] Track version info for non idgames releases (as idgames releases are final by design)
- [ ] Create a new category for WADs awaiting updates/a full release.
  - idk what to call this category, but i'm open to suggestions

## Statistics
- [ ] `caco stats` command
  - [ ] Total playtime across all WADs
  - [ ] WADs played per month/year
  - [ ] Completion rate

## TUI
- [ ] Textual-based TUI
- [ ] The TUI should be able to be called with `caco --tui`
- [ ] Browse library with vim keybindings
- [ ] Quick-play from list
- [ ] Session history view

## GUI
- [ ] gui for launching and managing WADs
- [ ] The GUI should be able to be called with `caco --gui`
- [ ] downloaded WADs should have a thumbnail which is extracted directly from the TITLEPIC in the WAD
  - there are various utilities that can do this. deutex is one, but there may be some python libraries that can extract WAD info
