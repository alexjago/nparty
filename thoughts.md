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

Conclusion: the entire thing gets done in like 5 seconds. Sufficiently fast for now. 

## Updating data

Ideally we shouldn't need to independently update anything, just do it all in a streaming fashion on-line. 

That means we need to sniff formats somehow (`tree_magic`?), in a layered fashion: 

- Is it a ZIP? If so, decode it to a string, wrap with io::Cursor. If not, return something that implements Read+Seek.
- Test for whether and what kind of spreadsheet it is. If it's one that's not an xSV, in-memory convert it to CSV with `calamine` (write to another buffer I guess).
- *Finally* pass that onto the main analysis bits. 

### Upgrading SA1s vs upgrading districts

* `nparty upgrade sa1s` should convert an `SA1_Prefs.csv`
* `nparty upgrade dists` would be a better choice for what is currently `upgrade sa1s`

## "Version 2.0" processing flow

TL;DR: factor out the deserialisation, the setup analysis, and the per-row analysis. 

Factored out deserialisation (especially with Serde making it so damn easy) effectively removes the need for a separate upgrade step. Just `.deserialize()`, do a minimal conversion, pass it in. 

The per-row analysis, as factored out, actually doesn't need to know anything about the booth. It just needs to know: 

- the pref data
- relevant candidate indices
- where BTLs start
- what candidates are in what groups
- an ordering on group-preferred sequences (i.e. 0:None, 1:Grn, 2:Alp, 3:Lnp, 4:GrnAlp, ...)

And then it simply outputs a column index to increment. 

(We can probably only parse relevant ints; even the BTL check can be done as a string comparison and it might well be faster... but also more fragile, what about whitespace etc?)

## Floats and accuracy

Sometimes there are slight differences between the Python and Rust output. I can only assume this is down to float errors. Concerning. 

## Configuration CLI vs Klask GUI

* chatbot config doesn't work with Klask
* what we really need there is probably beyond what Klask can provide

Libraries needed on Linux: 

    sudo apt-get install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libspeechd-dev libxkbcommon-dev libssl-dev

## GUI adventures

Refactor todo: have two binaries. 

`Cargo.toml` specifies two binaries:

    [[bin]]
    name = "nparty"
    path = "src/main.rs"

    [[bin]]
    name = "nparty-gui"
    path = "src/gui.rs"

Each of which has its own entrypoint to common code.
<<<<<<< Updated upstream

## Debian/Ubuntu notes

If you get a linking error with:

      = note: /usr/bin/ld: cannot find -lxcb-shape: No such file or directory
          /usr/bin/ld: cannot find -lxcb-xfixes: No such file or directory
          collect2: error: ld returned 1 exit status

Try:

    sudo apt install libxcb-shape0-dev libxcb-xfixes0-dev

