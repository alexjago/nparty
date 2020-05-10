# n-party preferred distributions and related things

This is a Rust rewrite of https://github.com/alexjago/nPP-Senate

After all, a great way to learn a language is to rewrite an existing project in it!

## Goals

1. match all existing functionality
2. provide a command line executable for all three major platforms compiled on GitHub
3. finally make a GUI using neutrino or tauri
4. Contra (2/3), what if this was a web app that leveraged WASM? Just go to, say, `abjago.net/nparty` and get going!

## Replication

Well, there's a lot. Especially fun will be replacing all the `dict`s that I love
to use in Python. But it will be worth it!

Order of operations:

1. `npp_utils.py` => `utils.rs`



## Configuration round 3

Looks like there's no feature-compatible equivalent to ConfigParser in Rust.

That's sort of OK anyway. The config format was hacky.

### TOML

We should be able to replicate most functionality in TOML. There are two main issues:

#### Defaults and interpolation

TOML doesn't have a native concept of a `[DEFAULT]` section, nor interpolation. We'd have to DIY this as an extension.

Defaults aren't too difficult. Interpolation is a bit of a nightmare. Keep the DEFAULTS section (rather than top level keys). No interpolation. 

#### `GROUPS = {}`

This should propagate across well enough. Inline table syntax doesn't work well for multiline so instead we do this:

    [SCENARIO_CODE]
    key = "value"

    [SCENARIO_CODE.GROUPS]
    Alp = ["J:Australian Labor Party", "J:GREEN Nita", ...]
    Lnp = ["D:Liberal National Party of Queensland", "D:SCARR Paul", ...]

Well, having tried that, we want to be a little less reliant on `[XYZ.GROUPS]` being entirely after the top level KVs of `XYZ`, so instead we'll use:

    [SCENARIO_CODE]
    key = "value"
    GROUPS.Alp = ["J:Australian Labor Party", "J:GREEN Nita", ...]
    GROUPS.Lnp = ["D:Liberal National Party of Queensland", "D:SCARR Paul", ...]

Which has the same semantic result.


## Booths iteration could surely be faster?

In an ideal world we'd only need to iterate over the prefs sequence once.
We'd probably lose some introspection ability but gain a bunch more in performance.

In particular:

- iterate over ATl and BTL segments separately
- do BTL first, because it's always needed
- need an efficient way to determine the sequence
  - need internal "group numbers" maybe?
  - this is probably the key, stringz r slow
  - but refs to strings not necessarily
  - should boil down to a pointer+length comparison

 It could probably be faster, but also cache locality is a thing. So the conversion to ints should probably stay as one piece.

 Maybe we can do something with iterators?
