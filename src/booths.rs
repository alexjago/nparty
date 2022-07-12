//! The n-party-preferred distribution phase.
/// We want to reduce each unique preference sequence to some ordering
///    of each of the parties. For example, for four parties there are 65 orderings:
///   `(0!) + (4 * 1!) + (6 * 2!) + (4 * 3!) + (4!)`
///
/// In fact, there are even more orderings (voters might interleave candidates)
/// but we will consider the most-preferred candidate from each party as
/// representing it (e.g. a vote `A1 > B1 > B2 > B3 > A2 > A3` as `A > B`).
use color_eyre::eyre::{bail, eyre, Context, ContextCompat, Result};
use color_eyre::Section;
use factorial::Factorial;
use indexmap::IndexMap;
use itertools::Itertools;
use std::collections::HashMap;
use std::fs::create_dir_all;
use std::path::Path;
use string_interner::{backend::StringBackend, symbol::SymbolU16, StringInterner};
use tracing::{info, trace};

use super::term;
use super::utils::{fix_prefs_headers, open_csvz_from_path, StateAb};

/// The output file will start with these five columns:
/// Booth ID, division name, booth name, latitude and longitude.
const NPP_FIELD_NAMES: [&str; 5] = ["ID", "Division", "Booth", "Latitude", "Longitude"];
// for the output
// const BOOTH_FIELD_NAMES :[&str; 15] = ["State", "DivisionID", "DivisionNm", "PollingPlaceID", "PollingPlaceTypeID", "PollingPlaceNm",
//                               "PremisesNm", "PremisesAddress1", "PremisesAddress2", "PremisesAddress3", "PremisesSuburb",
//                               "PremisesStateAb", "PremisesPostCode", "Latitude", "Longitude"];

/// Preferences files in the 2019+ format begin with these six columns.
const PREFS_FIELD_NAMES: [&str; 6] = [
    "State",
    "Division",
    "Vote Collection Point Name",
    "Vote Collection Point ID",
    "Batch No",
    "Paper No",
];

const NON_BOOTH_CONVERT: [&str; 4] = ["ABSENT", "POSTAL", "PRE_POLL", "PROVISIONAL"];

fn non_booth_convert(input: &str) -> &str {
    match input {
        "ABSENT" => "Absent",
        "POSTAL" => "Postal",
        "PRE_POLL" => "Pre-Poll",
        "PROVISIONAL" => "Provisional",
        _ => "Other",
    }
}

// `let` can only be used in a function
fn ttyjump() -> &'static str {
    if atty::is(atty::Stream::Stderr) {
        term::TTYJUMP
    } else {
        ""
    }
}

/// Returns the sum of (1!, ..., input!)
pub fn factsum(input: usize) -> usize {
    let mut output: usize = 0;
    for i in 1..=input {
        output += i.factorial();
    }
    output
}

/// Construct all the orderings of the specified groups.
///
/// (i.e. the sequence of permutations of the groups,
/// from length 0 to length N)
pub fn group_combos(groups: &[&str]) -> Vec<String> {
    let mut combinations = Vec::with_capacity(factsum(groups.len()));
    combinations.push(String::from("None"));

    for r in 1..=groups.len() {
        for i in groups.iter().permutations(r) {
            let combo: String = i.iter().map(|x| (**x)).collect();
            combinations.push(combo);
        }
    }

    combinations
}

/// A "combo tree" is a map of the indexes of party orderings.  
/// Suppose we had N parties labelled as 0 through N-1. This allows us to
/// take an *ordering* of those parties, say `[0, 1]` and get the corresponding
/// index in the output of [`group_combos`]. In turn, that index can stand for
/// the ordering (which is itself a reduction of the ballot).
///
/// tl;dr this function exists to avoid a *lot* of string allocations.
pub fn make_combo_tree(groups_count: usize) -> HashMap<Vec<usize>, usize> {
    let mut output: HashMap<Vec<usize>, usize> = HashMap::new();
    output.insert(vec![], 0_usize);
    let mut c: usize = 1;
    for r in 1..=groups_count {
        for i in (0..groups_count).permutations(r) {
            let combo: Vec<usize> = i.into_iter().collect();
            output.insert(combo, c);
            c += 1;
        }
    }

    output
}

/// This represents a row in the `polling_places` file
#[derive(Debug, Deserialize)]
#[allow(non_snake_case)] // look, this isn't aesthetic but it matches the file
#[allow(dead_code)] // most of these columns aren't actually used
pub struct BoothRecord {
    State: StateAb,
    DivisionID: usize,
    DivisionNm: String,
    PollingPlaceID: usize,
    PollingPlaceTypeID: usize,
    PollingPlaceNm: String,
    PremisesNm: String,
    PremisesAddress1: String,
    PremisesAddress2: String,
    PremisesAddress3: String,
    PremisesSuburb: String,
    PremisesStateAb: StateAb,
    PremisesPostCode: Option<usize>,
    Latitude: String, // yes these are floats, but we don't actually care about their values
    Longitude: String, // and now we don't have to care about deserialising them either
}

/// A (Division, Booth) combination
type DivBooth = (SymbolU16, SymbolU16);

/// A map from the party name to a list of (pseudo)candidates of that party.
pub type Parties = IndexMap<String, Vec<String>>;

/// Perform the distribution over a specified set of parties.
///
/// * `formal_prefs_path`: the input preferences (one row per ballot)
/// * `polling_places_path`: the input info on polling places
/// * `npp_booths_path`: where to write the output.
pub fn booth_npps(
    parties: &Parties,
    state: StateAb,
    formal_prefs_path: &Path,
    polling_places_path: &Path,
    npp_booths_path: &Path,
) -> Result<()> {
    // TODO: make this take Read objects instead of paths.
    //       otherwise it'll never work in WASM.
    info!("\tDistributing Preferences");
    let mut partykeys = Vec::with_capacity(parties.len());
    for i in parties.keys() {
        partykeys.push(i.as_str());
    }
    partykeys.sort_unstable();
    let partykeys = partykeys;

    let mut party_indices: HashMap<&str, usize> = HashMap::new();
    for (i, val) in partykeys.iter().enumerate() {
        party_indices.insert(val, i);
    }
    let party_indices = party_indices;

    let combinations = group_combos(&partykeys);
    trace!("Combinations:\n{:#?}", combinations);
    let combo_tree = make_combo_tree(partykeys.len());

    trace!(
        "Combo tree:\nOrder\tIndex\tPreference\n{}",
        combo_tree
            .iter()
            .map(|(k, v)| format!("{:?}\t{:5}\t{}", k, v, combinations[*v]))
            .join("\n")
    );

    // this is now just for actual booth data
    // For some gods-forsaken reason, the PollingPlaceID is not the Vote Collection Point ID
    // The only consistent identifier is ({Division}, {Booth})
    let mut interner = StringInterner::<StringBackend<SymbolU16>>::new();
    let mut booths: HashMap<DivBooth, BoothRecord> = HashMap::new();

    // but here we use Serde

    // OK, let's figure out polling places
    let mut pp_rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .has_headers(false)
        .from_path(polling_places_path)?;
    // 2019 problems: there's a pre-header line
    // we need to skip it, and we're going to do so manually.

    let pp_rdr_iter = pp_rdr.records();
    let mut row_count: usize = 0;
    let mut btl_count: usize = 0;

    for result in pp_rdr_iter {
        row_count += 1;
        if row_count < 3 {
            continue;
        }

        // if row_count > 22 {
        //     trace!("{:#?}", booths);
        //     break;
        // }

        let record: BoothRecord = result?.deserialize(None)?; //
                                                              // do actual-useful things with record
        if record.State != state {
            continue;
        }
        let division_nm = interner.get_or_intern(record.DivisionNm.clone());
        let booth_nm = interner.get_or_intern(record.PollingPlaceNm.clone());
        let dvb = (division_nm, booth_nm);
        booths.insert(dvb, record);
    }

    trace!("Loaded {} polling places", row_count - 2);

    // ***** Iterating over Preferences *****

    // The 2019 format is that there are a few fixed headers ...
    // and then a field for each [pseudo]candidate

    // faster! https://blog.burntsushi.net/csv/#amortizing-allocations

    let mut prefs_rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .escape(Some(b'\\')) //.trim(csv::Trim::All)
        .from_reader(open_csvz_from_path(formal_prefs_path)?);

    let prefs_headers = prefs_rdr.headers()?.clone();
    trace!("\nNo actual preferences processed yet, but we successfully opened the zipfile and the raw headers look like this:\n{:#?}", prefs_headers);

    // Now we figure out a bunch of things.
    // We index fields by "TICKETCODE:LASTNAME Given Names"

    let above_start = PREFS_FIELD_NAMES.len(); // relative to in general
    let mut below_start: usize = 0; // relative to atl_start

    // 2022 lack-of-quoting problems
    let prefs_headers_fixed = fix_prefs_headers(&prefs_headers, above_start);

    for i in (above_start + 1)..prefs_headers_fixed.len() {
        // The first ticket is labelled "A" and there are two candidates per ticket.
        // So the _second_ "A:", if it exists, is the first BTL field.
        // If it doesn't exist (loop exhausts) then _all_ we have are UnGrouped candidates
        // and thus the first BTL field is simply the first prefs field at all.
        if prefs_headers_fixed
            .get(i)
            .context("No candidates")?
            .starts_with("A:")
        {
            below_start = i - above_start;
            break;
        }
    }
    let below_start = below_start; // make immutable now

    // Create candidate number index

    let mut cand_nums: HashMap<&str, usize> = HashMap::new();
    for (i, pref) in prefs_headers_fixed.iter().skip(above_start).enumerate() {
        cand_nums.insert(pref, 1 + i);
    }
    let cand_nums = cand_nums; // make immutable now

    // set up some lookups...
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut groups_above: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut groups_below: HashMap<usize, Vec<usize>> = HashMap::new();

    for (party, cand_list) in parties.iter() {
        let mut party_cand_nums = Vec::new();
        let mut above_cands = Vec::new();
        let mut below_cands = Vec::new();
        for cand in cand_list {
            let cn = cand_nums
                .get::<str>(cand)
                .context("missing cand_num")?
                .to_owned();
            party_cand_nums.push(cn);
            if cn > below_start {
                below_cands.push(cn);
            } else {
                above_cands.push(cn);
            }
        }
        let p_idx = *party_indices
            .get(party.as_str())
            .with_context(|| format!("The party/group {} is missing from party_indices", party))?;
        groups.insert(p_idx, party_cand_nums);
        groups_above.insert(p_idx, above_cands);
        groups_below.insert(p_idx, below_cands);
    }

    trace!("cand_nums:\n{}", {
        let mut a: Vec<(&str, usize)> = Vec::new();
        a.extend(cand_nums);
        a.sort_by(|&(_, a), &(_, b)| a.cmp(&b));
        a.iter().map(|(s, u)| format!("{:4}\t{}", u, s)).join("\n")
    });
    trace!("Full Groups: {:?}", groups);
    trace!("ATL Groups: {:?}", groups_above);
    trace!("BTL Groups: {:?}", groups_below);

    eprintln!(); // still a normal eprintln for reasons

    // At long last! It is time to actually go over the rows!

    // this is where we're going to store all the things
    let mut booth_counts: HashMap<DivBooth, Vec<usize>> = HashMap::new();

    let cands_count = (&prefs_headers_fixed).len() - above_start;

    let prefs_rdr_iter = prefs_rdr.records();
    let mut progress: usize = 0;

    // We won't do a "magic" deserialisation as only a few initial columns
    // are the same between different ballot files
    for row in prefs_rdr_iter {
        let record = row?;

        let divnm = interner.get_or_intern(&record[1]);
        let boothnm = interner.get_or_intern(&record[2]);

        if (&record[1]).starts_with("---") {
            // ^^ This conditional might be inverted for testing; 2019+ files do NOT contain a `---` line.
            return Result::Err(eyre!("Preferences file is in the 2016 format."))
                .suggestion("Upgrade the file to the 2019+ format with:\n\tnparty upgrade prefs");
        }

        // Now we analyse nPP. We categorise the preference sequence by its highest value for each group of candidates.

        // First we must determine if it's ATL or BTL.

        // Per section 268A of the Commonwealth Electoral Act, a vote is BTL-formal if it has at least [1] through [6] marked BTL
        // and BTL-formality takes priority over ATL-formality.
        // NOTE 2020-05-14: I am quite confident this ATL-vs-BTL code is correct. It produces the correct number of BTLs...
        let mut is_btl: bool = false; // we must test whether it is, but...
        let bsa = above_start + below_start; // BTL, start, absolute
        if record.len() > bsa {
            // ^^^ this is the actual biggest speedup for default 2019 files.
            // If there aren't any fields for BTLs, there aren't any at all...
            // and the 2019 files don't bother with trailing commas.
            let mut btl_counts: [usize; 6] = [0; 6]; // Note zero-indexing now!
                                                     // fragility note: wrapping adds.
                                                     // This is only a problem if there are more than usize::MAX candidates BTL
                                                     // ... and someone plays *extreme* silly buggers

            for v in record.iter().skip(bsa) {
                match v.trim() {
                    "1" => btl_counts[0] += 1,
                    "2" => btl_counts[1] += 1,
                    "3" => btl_counts[2] += 1,
                    "4" => btl_counts[3] += 1,
                    "5" => btl_counts[4] += 1,
                    "6" => btl_counts[5] += 1,
                    _ => continue,
                }
            }
            is_btl = btl_counts.iter().all(|c| *c == 1);
        }

        // Select the appropriate candidates
        let groups_which = if is_btl { &groups_below } else { &groups_above };
        if is_btl {
            btl_count += 1;
        }

        // Having determined the ballot's ATL/BTL status we determine the "best" preference
        // (if any) for each party we care about. We take only the "best" if candidates
        // are interleaved. Thus `bests` is a list of `(preference, party number)` pairs.
        // (Each party has an implicit number, like we mentioned in [make_combo_tree]).
        let mut bests: Vec<(usize, usize)> = Vec::with_capacity(partykeys.len());

        for (party_num, candidate_nums) in groups_which.iter() {
            // Manually iterate over the candidates and get the best preferenced.
            // There's unlikely to be more than about 13 per party, so O(n) is OK.
            let mut cur_best: usize = cands_count;
            for i in candidate_nums {
                if let Some(x) = record.get(i + above_start - 1) {
                    // ^^ always check: is this the right offset?
                    if let Ok(bal) = x.trim().parse::<usize>() {
                        if bal < cur_best {
                            cur_best = bal;
                        }
                    }
                }
            }
            if cur_best < cands_count {
                bests.push((cur_best, *party_num));
            }
        }
        // sort to order them
        bests.sort_unstable();
        let order: Vec<usize> = bests.iter().map(|x| x.1).collect();
        let pref_idx = *combo_tree.get(&order).context("no pref index?")?;

        let divbooth: DivBooth = (divnm, boothnm);

        let booth = booth_counts
            .entry(divbooth)
            .or_insert_with(|| vec![0_usize; combinations.len()]);
        booth[pref_idx] += 1;

        // progress!
        progress += 1;
        if progress % 100_000 == 0 {
            info!(
                "{}\t\tPreferencing progress: {} ballots", // normally a leading {}
                ttyjump(),
                progress
            );
        }
    }

    // and we are done with the main task!
    info!(
        "{}\t\tPreferencing complete: {} ballots ({} were BTL)",
        ttyjump(),
        progress,
        btl_count
    );

    trace!(
        "We interned {} strings out of a possible {}.",
        interner.len(),
        u16::MAX
    );

    // Initially, the special votes are split up into e.g. POSTAL_1 through POSTAL_8
    // This isn't useful for us, so we'll aggregate them.
    info!("\t\tAggregating Absents, Postals, Prepolls & Provisionals");

    let mut division_specials: HashMap<DivBooth, Vec<usize>> = HashMap::new();

    let mut to_remove = Vec::new();

    // What we're doing here is aggregating all special votes.
    for (bk, bv) in &booth_counts {
        for w in &NON_BOOTH_CONVERT {
            if interner.resolve(bk.1).unwrap().contains(w) {
                let divbooth: DivBooth = (bk.0, interner.get_or_intern(non_booth_convert(w)));
                let db = division_specials
                    .entry(divbooth)
                    .or_insert_with(|| vec![0_usize; bv.len()]);
                for j in 0..combinations.len() {
                    db[j] += bv[j];
                }
                // ^^ Still not sure I like this version. We didn't need to do the addition on new entries before.
                to_remove.push(*bk);
                break;
            }
        }
    }

    for bk in &to_remove {
        booth_counts.remove(bk);
    }

    // [NPP_FIELD_NAMES] + [combinations] + [total]

    info!("\t\tWriting File");

    // and now we write
    // first create directory if needed
    create_dir_all(
        npp_booths_path
            .parent()
            .with_context(|| format!("{} has no parent", npp_booths_path.display()))?,
    )?;
    let mut wtr = csv::WriterBuilder::new()
        .terminator(csv::Terminator::CRLF)
        .has_headers(false)
        .from_path(npp_booths_path)?;

    let npp_header = &mut NPP_FIELD_NAMES.to_vec();
    for i in &combinations {
        npp_header.push(i.as_str());
    }
    npp_header.push("Total");

    wtr.write_record(npp_header)
        .context("error writing booths header")?;

    for (bk, bv) in booth_counts.iter().sorted() {
        let br = match booths.get(bk) {
            Some(x) => x,
            _ => bail!("It's really weird, but {:?} isn't in `booths`.", bk),
        };
        let mut bdeets = vec![
            br.PollingPlaceID.to_string(),
            br.DivisionNm.clone(),
            br.PollingPlaceNm.clone(),
            br.Latitude.clone(),
            br.Longitude.clone(),
        ];
        let mut total = 0;
        for i in bv.iter() {
            bdeets.push(i.to_string());
            total += *i;
        }
        bdeets.push(total.to_string());
        let bdeets = bdeets;
        wtr.write_record(&bdeets).context("error writing booths")?;
    }
    wtr.flush().context("error writing booths")?;

    for (bk, bv) in &division_specials {
        let mut bdeets: Vec<String> = vec![
            "".to_string(),
            interner.resolve(bk.0).unwrap().to_string(),
            interner.resolve(bk.1).unwrap().to_string(),
            "".to_string(),
            "".to_string(),
        ];

        let mut total = 0;
        for i in bv.iter() {
            bdeets.push(i.to_string());
            total += *i;
        }
        bdeets.push(total.to_string());
        let bdeets = bdeets;
        wtr.write_record(&bdeets).context("error writing booths")?;
    }
    wtr.flush().context("Failed to finalise writing booths")?;

    Ok(())
}

/*
    *** Performance and string interning ***

    Because PollingPlaceID and VoteCollectionPointID aren't the same thing,
    we need to use (Division, Booth) as our key. This poses a bit of an issue:
    the StringRecord iterator gives us an &str to the fields, but
    (a) the lifetime is only good for that iteration
    (b) the types don't match for Map operations

    So originally, DivBooths were `(String, String)` and we cloned extensively.
    However, using string interning offers a potential benefit: we can swap out
    our Strings for Symbols (which are just uints in disguise).
    This also has HashMap benefits, so we'll have to test non-interned HashMaps

    *** Results ***

    Relative result notes from `cargo-flamegraph`, main datastructures are HashMaps

    * with interning we seem to be running at about 4% of runtime on interning?
    * without it we spend about 4% of time on alloc::borrow::ToOwned which doesn't show up with interning
    * we also spend WAY more time in BTree::entry, 17% vs 4%

    Absolute result notes from timing it (obviously machine-specific, but indicative):
    `for i in {1..10}; do time ./target/release/nparty -qq run -p distribute -s QLD_4PP configurations/2022.toml; done`
    (Default hashers, U16 symbols - QLD-2022 for example has < 2000 strings to intern.)

    * Interning and main structure BTreeMap: 3.11 mean, 3.17 max, 3.04 min
    * Interning and main structure HashMap: 3.09 mean, 3.15 max, 3.04 min
    * String/cloning and main structure BTreeMap: 3.74 mean, 3.83 max, 3.71 min
    * String/cloning and main structure HashMap: 3.38 mean, 3.46 max, 3.32 min

    So interning is faster, but really not by much. A larger saving was just in switching to HashMap.
    Still, we got a 17% speedup for this phase all told and that's pretty good.

*/
