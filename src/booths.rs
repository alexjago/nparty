//! The n-party-preferred distribution phase.
/// We want to reduce each unique preference sequence to some ordering
///    of each of the parties. For example, for four parties there are 65 orderings:
///   (0!) + (4 * 1!) + (6 * 2!) + (4 * 3!) + (4!)
use color_eyre::eyre::{bail, eyre, Context, ContextCompat, Result};
use color_eyre::Section;
use factorial::Factorial;
use indexmap::IndexMap;
use itertools::Itertools;
use std::collections::{BTreeMap, HashMap};
use std::fs::create_dir_all;
use std::path::Path;
use tracing::{info, trace};

use super::term;
use super::utils::{fix_prefs_headers, open_csvz_from_path, StateAb};

const NPP_FIELD_NAMES: [&str; 5] = ["ID", "Division", "Booth", "Latitude", "Longitude"];
// for the output
// const BOOTH_FIELD_NAMES :[&str; 15] = ["State", "DivisionID", "DivisionNm", "PollingPlaceID", "PollingPlaceTypeID", "PollingPlaceNm",
//                               "PremisesNm", "PremisesAddress1", "PremisesAddress2", "PremisesAddress3", "PremisesSuburb",
//                               "PremisesStateAb", "PremisesPostCode", "Latitude", "Longitude"];
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

/// Returns the sum of (1!...input!)
pub fn factsum(input: usize) -> usize {
    let mut output: usize = 0;
    for i in 1..=input {
        output += i.factorial();
    }
    output
}

pub fn group_combos(groups: &[&str]) -> Vec<String> {
    //! Generates a Vec<String> of all the orderings

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

/// This function exists to avoid a *lot* of string alloc
pub fn make_combo_tree(groups_count: usize) -> BTreeMap<Vec<usize>, usize> {
    let mut output: BTreeMap<Vec<usize>, usize> = BTreeMap::new();
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
#[allow(dead_code)]
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
pub type DivBooth = (String, String);
/// A map from the party name to a list of (pseudo)candidates
pub type Parties = IndexMap<String, Vec<String>>;

// TODO: make this take Read objects instead of paths.
//       otherwise it'll never work in WASM.
pub fn booth_npps(
    parties: &Parties,
    state: StateAb,
    formal_prefs_path: &Path,
    polling_places_path: &Path,
    npp_booths_path: &Path,
) -> Result<()> {
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

    let combo_tree = make_combo_tree(partykeys.len());

    let combinations = group_combos(&partykeys);
    trace!("Combinations:\n{:#?}", combinations);

    // this should get obsoleted by the combo_tree method
    let mut combo_order: HashMap<String, usize> = HashMap::new();
    combo_order.insert("None".to_string(), 0);
    let mut c: usize = 1;
    for i in &combinations {
        combo_order.insert(i.clone(), c);
        c += 1;
    }

    trace!(
        "Combo tree:\n{}",
        combo_tree
            .iter()
            .map(|(k, v)| format!("{:?}:\t{:?}\t{}", k, v, combinations[*v]))
            .join("\n")
    );

    // this is now just for actual booth data
    // For some gods-forsaken reason, the PollingPlaceID is not the Vote Collection Point ID
    // The only consistent identifier is {Division}_{Booth}
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

        let dvb = (record.DivisionNm.clone(), record.PollingPlaceNm.clone());
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

    let mut cand_nums: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, pref) in prefs_headers_fixed.iter().skip(above_start).enumerate() {
        cand_nums.insert(pref, 1 + i);
    }
    let cand_nums = cand_nums; // make immutable now

    // set up some lookups...
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut groups_above: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut groups_below: BTreeMap<usize, Vec<usize>> = BTreeMap::new();

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
    trace!("\nFull Groups: {:?}", groups);
    trace!("ATL Groups: {:?}", groups_above);
    trace!("BTL Groups: {:?}", groups_below);

    eprintln!(); // still a normal eprintln for reasons

    // At long last! It is time to actually go over the rows!

    // this is where we're going to store all the things
    let mut booth_counts: HashMap<DivBooth, Vec<usize>> = HashMap::new();

    let mut division_specials: BTreeMap<DivBooth, Vec<usize>> = BTreeMap::new();

    let cands_count = (&prefs_headers_fixed).len() - above_start;

    // we need to figure out how we're going to deserialize each record
    // or not - we need custom logic for most of it anyway

    let prefs_rdr_iter = prefs_rdr.records();
    let mut progress: usize = 0;

    // performance note: we tried amortizing the allocation and it was slower ?!?!?!?!?!
    // and then we tried again and it was faster.
    // let mut record = csv::StringRecord::new();
    // while prefs_rdr.read_record(&mut record)? {

    for row in prefs_rdr_iter {
        let record = row?;

        let divnm = &record[1];
        let boothnm = &record[2];

        if divnm.starts_with("---") {
            // TODO if this is a NOT, it is inverted for testing
            return Result::Err(eyre!("Preferences file is in the 2016 format."))
                .suggestion("Upgrade the file to the 2019+ format with:\n\tnparty upgrade prefs");
        }

        // Now we analyse nPP. We categorise the preference sequence by its highest value for each group of candidates

        // first we must determine if it's ATL or BTL.
        // Per section 268A of the Commonwealth Electoral Act, a vote is BTL-formal if it has at least [1] through [6] marked BTL
        // and BTL-formality takes priority over ATL-formality.
        // NOTE 2020-05-14: I am quite confident this ATL-vs-BTL code is correct. It produces the correct number of BTLs...
        // To be fair, non-BTLs don't have trailing commas to confuse the issue with...
        let mut is_btl: bool = false; // we must test whether it is, but...
        let bsa = above_start + below_start; // btl_start absolute
        if record.len() > bsa {
            // ^^^ this is the actual biggest speedup for default 2019 files.
            // If there aren't any fields for BTLs, there aren't any at all...
            // and the 2019 files don't bother with trailing commas.
            let mut btl_counts: [usize; 6] = [0; 6]; // Note zero-indexing now!
                                                     // fragility note: wrapping adds.
                                                     // This is only a problem if there are more than usize::MAX candidates BTL
                                                     // ... and someone plays *extreme* silly buggers

            for v in record.iter().skip(bsa) {
                // .skip() is a glorious thing, how did I not know it before
                // NB: this breaks if there's whitespace in the file.
                // We need to trim().
                // Given the whole lack-of-trailing-comma-sitch it won't even hurt too much
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

        let groups_which = if is_btl { &groups_below } else { &groups_above };
        if is_btl {
            btl_count += 1;
        }

        // This next bit is a little complicated. What we do is pre-generate a list of parties,
        // as well as a tree of combinations of parties, and we index by that. This avoids messing
        // about with strings as much as we can, especially in the inner loop.

        let mut bests: Vec<(usize, usize)> = Vec::with_capacity(partykeys.len());

        for (p, cns) in groups_which.iter() {
            let mut curbest: usize = cands_count; // this heavily reduced HashMapping
            for i in cns {
                let bal = match record.get(i + above_start - 1) {
                    // always check: is this the right offset?
                    Some(x) => match x.trim().parse::<usize>() {
                        Ok(n) => n,
                        Err(_) => continue,
                    },
                    None => continue,
                };
                if bal < curbest {
                    curbest = bal;
                }
            }
            if curbest < cands_count {
                bests.push((curbest, *p));
            }
        }

        bests.sort_unstable();
        let order: Vec<usize> = bests.iter().map(|x| x.1).collect();
        let pref_idx = *combo_tree.get(&order).context("no pref index?")?;

        // Using strings here is surely one of the slower parts of the operation
        // Actually this datastructure in general is one of the slower things.
        // Seems very difficult to avoid keying by DivBooth though.
        // But datastructure stuff is responsible for almost 20% of booths.rs runtime

        let divbooth: DivBooth = (divnm.to_string(), boothnm.to_string());

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
    info!("\t\tAggregating Absents, Postals, Prepolls & Provisionals");

    let mut to_remove = Vec::new();

    // What we're doing here is aggregating all special votes.
    for (bk, bv) in &booth_counts {
        for w in &NON_BOOTH_CONVERT {
            if bk.1.contains(w) {
                let divbooth: DivBooth = (bk.0.clone(), non_booth_convert(w).to_string());
                let db = division_specials
                    .entry(divbooth)
                    .or_insert_with(|| vec![0_usize; bv.len()]);
                for j in 0..combinations.len() {
                    db[j] += bv[j];
                }
                // ^^ Still not sure I like this version. We didn't need to do the addition on new entries before.
                to_remove.push(bk.clone());
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
            bk.0.clone(),
            bk.1.clone(),
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
