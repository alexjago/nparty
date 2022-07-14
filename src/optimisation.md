Performance Improvement Notes
=============================

Due to dealing with input files on the order of a few million rows, rather than a few thousand,
the distribution stage is the slowest of the three and therefore needs the most optimisation.

Commands:

```sh
cargo flamegraph --root --bin nparty -- -qq run --phase distribute -s QLD_4PP configurations/2022.toml

for i in {1..10}; do time ./target/release/nparty -qq run --phase distribute -s QLD_4PP configurations/2022.toml; done
```

N.B. 

* Functions need to be public and non-inlined to show up in the flamegraph! 
* Rename a flamegraph once it's generated!


Candidate/Order indexing
------------------------

* In each Scenario we have a set of Groups. We distribute preferences to each possible order:
    `None, A, B, C, ..., AB, ..., BA, ..., CA, CB, ..., ABC, ACB, ..., BAC, BCA, ...`
* Assuming some initial ordering of the Groups, the above sequence is distinguished and deterministic
* Once we know the order of the groups, we can simply refer to it by index in the sequence.
* [make_combo_tree] and friends are responsible for this and it seems to have been worth the effort


BTL checking pt. 1
------------------

* Checking the length of the record produced a major speedup.
* If there aren't any fields for BTLs, there aren't any at all...
* And the 2019-style files don't bother with trailing commas by default.


String Interning
----------------

Because PollingPlaceID and VoteCollectionPointID aren't the same thing,
we need to use (Division, Booth) as our key. This poses a bit of an issue:
the StringRecord iterator gives us an &str to the fields, but
(a) the lifetime is only good for that iteration
(b) the types don't match for Map operations

So originally, DivBooths were `(String, String)` and we cloned extensively.
However, using string interning offers a potential benefit: we can swap out
our Strings for Symbols (which are just uints in disguise).
This also has HashMap benefits, so we'll have to test non-interned HashMaps

Relative result notes from `cargo-flamegraph`:

* with interning we seem to be running at about 4% of runtime on interning?
* without it we spend about 4% of time on alloc::borrow::ToOwned which doesn't show up with interning
* we also spend WAY more time in BTree::entry, 17% vs 4%

Absolute result notes from timing it (obviously machine-specific, but indicative):
(Default hashers, U16 symbols - QLD-2022 for example has < 2000 strings to intern.)

* Interning and main structure BTreeMap: 3.11 mean, 3.17 max, 3.04 min
* Interning and main structure HashMap: 3.09 mean, 3.15 max, 3.04 min
* String/cloning and main structure BTreeMap: 3.74 mean, 3.83 max, 3.71 min
* String/cloning and main structure HashMap: 3.38 mean, 3.46 max, 3.32 min

So interning is faster, but really not by much. A larger saving was just in switching to HashMap.
Still, we got a 17% speedup for this phase all told and that's pretty good.


Switching to amortized allocations
----------------------------------

* It's notably faster. With the same setup as above, we get mean 2.44, max 2.47, min 2.41 - a 22% speedup.


"Parsing the row up-front" vs. "Parsing the row as-needed"
----------------------------------------------------------

Currently, we parse individual entries in the row as-needed.
* When BTL-checking we need to iterate over (most of) the row and do string comparisons
** (cost of that vs cost of parsing unclear, probably not worth it right now)
* For determining "bests" we need to get as many entries as there are candidates
** assuming no duplication across groups, that's only O(n) anyway
* The other factor is that many entries in the row are actually nulls; they're empty.
** We'd want to substitute all of those for some too-large number if we parsed up-front.

Conclusion: parsing-as-needed is probably superior.

Extension: what if we switch the main booths reader to a ByteRecord?
We'll need to parse to strings to get
* Division name
* Booth name
* Candidate preference for bests
We can potentially parsing for BTL - compare bytes, not strings.

Result: this was actually slightly slower!

ALSO, using the `csv` trim method was very slow - runtime more than doubled.


Making `utils::open_csvz` NOT read everything up front
------------------------------------------------------

This one is more of a max-memory-usage thing. 

The problem:

* A ZipFile (which represents an actual entry) has a lifetime reference on its ZipArchive
** This means we can't just return a ZipFile
* So previously we read everything up-front and returned it wrapped in a Cursor
* Now, we're returning an object that contains the ZipArchive, but delegates Read to the ZipFile

Initial errors:

* when not pre-reading, the first few elements are always the same in the % 100k debug-prints
* weirdly, when debug-printing every iteration, it's OK for a little while, but then we go
    off the rails at what should be (1, 2, 44) and... reset?
* The first 8192 bytes exactly were being read.

This is because the `Read` implementation was *recreating* the ZipFile anew.

The fix: use a self-referential struct to smuggle out both a persistent ZipFile and its parent ZipArchive
* Credit to ark#7419 and also to kpreid#9810, Stargateur#1337 and shepmaster#6139 from the Rust Discord


Reducing allocations in [distribute_preference]
-----------------------------------------------

* By the power of `#[inline(always)]` we see a lot of time being spent in this function.
* How can we reduce it? Was `bests` being hoisted previously?
* Tried hoisting `bests`, still lots of allocations
* Hoisting `order` as well solved the allocations problem though! (Of course.) This gave most of the speedup.
* Created new function [calculate_index] to replace the [ComboTree] lookup.
  * it's *ever so slightly* faster? #overoptimisation
  * With lookup: 2.19 sec avg.
  * With calculation: 2.13 sec avg. 
  * `calculate_index`: 1.83%
  * `hash_one`: 2.67%


Parsing and BTL-checking redux
------------------------------

I'm not 100% certain how `btl_counts.iter().all(|c| *c == 1)` is optimising. Switching to pure manual `btl_counts[0] == 1 && ...` was potentially fractionally faster but not worth it especially given the intention to plumb `take` back in eventually. 

We have to iterate over all BTLs when they exist. What's the penalty for parsing them then and there, and then having specialised `distribute_above` and `distribute_below`?

Result: smashed the two-second mark!