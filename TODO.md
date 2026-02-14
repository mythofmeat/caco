# define common sourceports and iwad dir config file
- i want to make it so that changing the sourceport is simpler for the user
- i think that a way to to this, is to make a commented list of sourceports in the config
- the user uncomments the sourceports that they have (changing the provided path if necessary)
- the default sourceport is listed above as "soureceport_default = dsda-doom"
- this way sourceports can be defined in the cli without having to use a path

- allow the user to point to the folder containing the iwads
- this will allow for specifying "doom2" / "doom2.wad" instead of using the full path in cli flags

what do you think of this approach?
