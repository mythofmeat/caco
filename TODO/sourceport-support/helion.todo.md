# Helion support (DEFERRED DUE TO LEVELSTAT ISSUES)

## Baseline Features Needed
- [ ] config management
  - [ ] Helion uses .ini files by default instead of .cfg
- [ ] save dir and data dir flags
  - [ ] save dir: `-savedir [folder]`
  - [ ] data dir: no  direct data dir flag.

- [ ] stats.txt / levelstats info and syncing
  - Helion writes levelstats files to the LOCATION OF THE BINARY (which means /usr/bin/levelstat.txt in most cases)
    - this behavior CAN NOT be overridden with a simple symlink of the binary to a user-writeable folder
    - either something needs to change upstream or we will have to copy the Helion binary itself...

- [ ] complevel injection
  - [ ] cli flag `+complevel [arg]`
  - [ ] `+complevel` argument must be from `mbf21`, `boom`, `mbf`, or `vanilla`

## Features gained
- [ ] Helion supports `-loadgame [savefile]`, dsda-doom doesn't. we can leverage our savegame management infrastructure here
