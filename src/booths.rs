use factorial::Factorial;
use itertools::Itertools;
/// This file translates Booth_NPP.py

/// We want to reduce each unique preference sequence to some ordering
///    of each of the parties. For example, for four parties there are 65 orderings:
///   (0!) + (4 * 1!) + (6 * 2!) + (4 * 3!) + (4!)
use std::collections::{BTreeMap, HashMap};
use std::fs::{create_dir_all, File};
use std::path::Path;

use super::term;
use super::utils::*;

const NPP_FIELD_NAMES: [&str; 5] = ["ID", "Division", "Booth", "Latitude", "Longitude"]; // for the output
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
    match atty::is(atty::Stream::Stderr) {
        true => term::TTYJUMP,
        false => "",
    }
}

/// Returns the sum of (1!...input!)
pub fn factsum(input: usize) -> usize {
    let mut output: usize = 0;
    for i in 1..=input {
        output += i.factorial();
    }
    return output;
}

/// Returns [groups! / i! where i = 0..=groups]
fn make_combo_levels(groups: usize) -> Vec<usize> {
    let mut output: Vec<usize> = Vec::with_capacity(groups + 1);
    let gg = groups.factorial();
    for i in 0..=groups {
        output.push(gg / i.factorial())
    }
    return output;
}

pub fn group_combos(groups: &Vec<&str>) -> Vec<String> {
    //! Generates a Vec<String> of all the orderings

    let mut combinations = Vec::with_capacity(factsum(groups.len()));
    combinations.push(String::from("None"));

    for r in 1..=groups.len() {
        for i in groups.clone().into_iter().permutations(r) {
            let combo: String = i.into_iter().collect();
            combinations.push(combo);
        }
    }

    return combinations;
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

    return output;
}

/// This represents a row in the polling_places file
#[derive(Debug, Deserialize)]
#[allow(non_snake_case)] // look, this isn't aesthetic but it matches the file
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

pub type DivBooth = (String, String);
pub type Parties = BTreeMap<String, Vec<String>>;

// TODO: make this take Read objects instead of paths.
//       otherwise it'll never work as wasm.
pub fn booth_npps(
    parties: &Parties,
    state: &StateAb,
    formal_prefs_path: &Path,
    polling_places_path: &Path,
    npp_booths_path: &Path,
) {
    // cut down for WIP-ing
    eprintln!("\tDistributing Preferences");
    let mut partykeys = Vec::with_capacity(parties.len());
    for i in parties.keys() {
        partykeys.push(i.as_str());
    }
    partykeys.sort();
    let partykeys = partykeys;

    let mut party_indices: HashMap<&str, usize> = HashMap::new();
    for (i, val) in partykeys.iter().enumerate() {
        party_indices.insert(val, i);
    }
    let party_indices = party_indices;

    let combo_tree = make_combo_tree(partykeys.len());

    let combinations = group_combos(&partykeys);
    // eprintln!("Combinations:\n{:#?}", combinations);

    // this should get obsoleted by the combo_tree method
    let mut combo_order: HashMap<String, usize> = HashMap::new();
    combo_order.insert("None".to_string(), 0);
    let mut c: usize = 1;
    for i in combinations.iter() {
        combo_order.insert(i.clone(), c);
        c += 1;
    }

    // eprintln!("Combo tree:\n");
    // for (k, v) in combo_tree.iter(){
    //     eprintln!("{:?}:\t{:?}\t{}", k, v, combinations[*v]);
    // }
    //
    // return;

    // this is now just for actual booth data
    // For some gods-forsaken reason, the PollingPlaceID is not the Vote Collection Point ID
    // The only consistent identifier is {Division}_{Booth}
    let mut booths: HashMap<DivBooth, BoothRecord> = HashMap::new();

    // but here we use Serde

    // OK, let's figure out polling places
    let mut pp_rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .has_headers(false)
        .from_path(polling_places_path)
        .unwrap();
    // 2019 problems: there's a pre-header line
    // we need to skip it, and we're going to do so manually.

    let mut pp_rdr_iter = pp_rdr.records();
    let mut row_count: usize = 0;
    let mut btl_count: usize = 0;

    loop {
        if let Some(result) = pp_rdr_iter.next() {
            row_count += 1;
            if row_count < 3 {
                continue;
            }

            // if row_count > 22 {
            //     eprintln!("{:#?}", booths);
            //     break;
            // }

            let record: BoothRecord = result.unwrap().deserialize(None).unwrap(); //
                                                                                  // do actual-useful things with record
            if record.State != *state {
                continue;
            }

            let dvb = (
                record.DivisionNm.to_owned(),
                record.PollingPlaceNm.to_owned(),
            );
            booths.insert(dvb, record);
        } else {
            break;
        }
    }

    // eprintln!("Loaded {} polling places", row_count - 2);

    // ***** Iterating over Preferences *****

    // The 2019 format is that there are a few fixed headers ...
    // and then a field for each [pseudo]candidate

    // faster! https://blog.burntsushi.net/csv/#amortizing-allocations

    let mut prefs_rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .escape(Some(b'\\')) //.trim(csv::Trim::All)
        .from_reader(open_csvz_from_path(formal_prefs_path));

    let prefs_headers = prefs_rdr.headers().unwrap().clone();
    // eprintln!("Prefs headers: {:?}", prefs_headers.as_slice());
    // eprintln!("\nNo actual preferences processed yet, but we successfully opened the zipfile and the headers look like this:\n{:#?}", prefs_headers);

    // Now we figure out a bunch of things.
    // We index fields by "TICKETCODE:LASTNAME Given Names"

    let atl_start = PREFS_FIELD_NAMES.len(); // relative to in general
    let mut btl_start: usize = 0; // relative to atl_start

    for i in (atl_start + 1)..prefs_headers.len() {
        // The first ticket is labelled "A" and there are two candidates per ticket.
        // So the _second_ "A:", if it exists, is the first BTL field.
        // If it doesn't exist (loop exhausts) then _all_ we have are UnGrouped candidates
        // and thus the first BTL field is simply the first prefs field at all.
        if prefs_headers.get(i).unwrap().starts_with("A:") {
            btl_start = i - atl_start;
            break;
        }
    }
    let btl_start = btl_start; // make immutable now

    // eprintln!("\nATL start: {}\tBTL start: +{}\t Total: {}", atl_start, btl_start, prefs_headers.len());

    // Create candidate number index

    let mut cand_nums: HashMap<&str, usize> = HashMap::new();
    for i in atl_start..prefs_headers.len() {
        cand_nums.insert(prefs_headers.get(i).unwrap(), 1 + i - atl_start);
    }
    let cand_nums = cand_nums; // make immutable now

    eprintln!("\nCandidate numbers: \n{:#?}", cand_nums);

    // finally, some lookups for the groups of interest...

    // eprintln!("\nPARTIES:\n{:#?}", parties);

    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut groups_atl: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut groups_btl: HashMap<usize, Vec<usize>> = HashMap::new();

    for (party, cand_list) in parties.iter() {
        // eprintln!("{}   {:?}", party, cand_list);
        let mut pcand_nums = Vec::new();
        let mut acands = Vec::new();
        let mut bcands = Vec::new();
        for cand in cand_list {
            let cn = cand_nums.get::<str>(cand).unwrap().to_owned();
            pcand_nums.push(cn);
            if cn > btl_start {
                bcands.push(cn)
            } else {
                acands.push(cn)
            }
        }
        groups.insert(*party_indices.get(party.as_str()).unwrap(), pcand_nums);
        groups_atl.insert(*party_indices.get(party.as_str()).unwrap(), acands);
        groups_btl.insert(*party_indices.get(party.as_str()).unwrap(), bcands);
    }

    eprintln!("\nFull Groups: {:#?}", groups);
    eprintln!("ATL Groups: {:#?}", groups_atl);
    eprintln!("BTL Groups: {:#?}", groups_btl);

    eprintln!("");

    // At long last! It is time to actually go over the rows!

    // this is where we're going to store all the things
    let mut booth_counts: HashMap<DivBooth, Vec<usize>> = HashMap::new();
    let mut bbooth_counts: HashMap<&str, HashMap<&str, Vec<usize>>> = HashMap::new();

    let mut division_specials: BTreeMap<DivBooth, Vec<usize>> = BTreeMap::new();
    // let mut ppids = HashMap::new();

    let cands_count = prefs_headers.len() - atl_start;
    // let mut seq : Vec<usize> = Vec::with_capacity(cands_count); // just hoisting an alloc

    // we need to figure out how we're going to deserialize each record
    // or not - we need custom logic for most of it anyway

    let prefs_rdr_iter = prefs_rdr.records();
    let mut progress: usize = 0;

    // performance note: we tried amortizing the allocation and it was slower ?!?!?!?!?!
    // and then we tried again and it was faster.
    // let mut record = csv::StringRecord::new();
    // while prefs_rdr.read_record(&mut record).unwrap() {

    for row in prefs_rdr_iter {
        let record = row.unwrap();

        let divnm = &record[1];
        let boothnm = &record[2];

        if divnm.starts_with("---") {
            panic!("Please use `16to19.py` to first upgrade your old data to the new format.");
        }

        // Now we analyse nPP. We categorise the preference sequence by its highest value for each group of candidates

        // first we must determine if it's ATL or BTL.
        // NOTE 2020-05-14: I am quite confident this ATL-vs-BTL code is correct. It produces the correct number of BTLs...
        // To be fair, non-BTLs don't have trailing commas to confuse the issue with...
        let mut is_btl: bool = false; // we must test whether it is, but...
        let bsa = atl_start + btl_start; // btl_start absolute
        if record.len() > bsa {
            // ^^^ this is the actual biggest speedup for default 2019 files.
            // If there aren't any fields for BTLs, there aren't any at all...
            // and the 2019 files don't bother with trailing commas.
            is_btl = true;
            let mut btl_counts: [u16; 6] = [0; 6]; // Note zero-indexing now!
                                                   // fragility note: wrapping adds.
                                                   // This is only a problem if there are more than 65536 candidates BTL and someone plays extreme silly buggers
                                                   // .skip() is a glorious thing, how did I not know it before
            for v in record.iter().skip(bsa) {
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
            for j in 0..6 {
                is_btl &= btl_counts[j] == 1;
            }
        }

        let groups_which = match is_btl {
            true => &groups_btl,
            false => &groups_atl,
        };
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
                let bal = match record.get(i + atl_start - 1) { // always check: is this the right offset?
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

        bests.sort();
        let order: Vec<usize> = bests.iter().map(|x| x.1).collect();
        let pref_idx = *combo_tree.get(&order).unwrap();

        // Using strings here is surely one of the slower parts of the operation
        // Actually this datastructure in general is one of the slower things.
        // Seems very difficult to avoid keying by DivBooth though.
        // But datastructure stuff is responsible for almost 20% of booths.rs runtime

        let divbooth: DivBooth = (divnm.to_string(), boothnm.to_string());

        let booth = booth_counts
            .entry(divbooth)
            .or_insert(vec![0_usize; combinations.len()]);
        booth[pref_idx] += 1;

        // progress!
        progress += 1;
        if progress % 10000 == 0 { // normally 100K
            eprintln!(
                "\t\tPreferencing progress: {} ballots", // normally a leading {}
                //term::TTYJUMP,
                progress
            );
	eprintln!("{:#?}", &record);
	eprintln!("{:?}\t{}", &bests, &combinations[pref_idx]);
        }
    }

    // and we are done with the main task!
    eprintln!(
        "{}\t\tPreferencing complete: {} ballots. ({} were BTL)",
        term::TTYJUMP,
        progress,
        btl_count
    );
    eprintln!("\t\tAggregating Absents, Postals, Prepolls & Provisionals");

    let mut to_remove = Vec::new();

    // What we're doing here is aggregating all special votes.
    for (bk, bv) in booth_counts.iter() {
        for w in &NON_BOOTH_CONVERT {
            if bk.1.contains(w) {
                let divbooth: DivBooth = (bk.0.clone(), non_booth_convert(w).to_string());
                if division_specials.contains_key(&divbooth) {
                    for j in 0..combinations.len() {
                        division_specials.get_mut(&divbooth).unwrap()[j] += bv[j];
                    }
                } else {
                    division_specials.insert(divbooth, bv.clone());
                }
                to_remove.push(bk.clone());
                break;
            }
        }
    }

    for bk in to_remove.iter() {
        booth_counts.remove(bk);
    }

    // [NPP_FIELD_NAMES] + [combinations] + [total]

    // eprintln!("\t\tWriting File");

    // and now we write
    // first create directory if needed
    create_dir_all(npp_booths_path.parent().unwrap()).unwrap();
    let mut wtr = csv::WriterBuilder::new()
        .terminator(csv::Terminator::CRLF)
        .has_headers(false)
        .from_path(npp_booths_path)
        .unwrap();

    let mut npp_header = &mut NPP_FIELD_NAMES.to_vec();
    for i in combinations.iter() {
        npp_header.push(i.as_str());
    }
    npp_header.push("Total");

    wtr.write_record(npp_header)
        .expect("error writing booths header");

    for (bk, bv) in booth_counts.iter().sorted() {
        let br = booths.get(bk).expect(&format!(
            "It's really weird, but {:#?} isn't in `booths`.",
            bk
        ));
        let mut bdeets = Vec::new();
        bdeets.push(br.PollingPlaceID.to_string());
        bdeets.push(br.DivisionNm.clone());
        bdeets.push(br.PollingPlaceNm.clone());
        bdeets.push(br.Latitude.clone());
        bdeets.push(br.Longitude.clone());
        let mut total = 0;
        for i in bv.iter() {
            bdeets.push(i.to_string());
            total += *i;
        }
        bdeets.push(total.to_string());
        let bdeets = bdeets;
        wtr.write_record(&bdeets).expect("error writing booths");
    }
    wtr.flush().expect("error writing booths");

    for (bk, bv) in division_specials.iter() {
        let mut bdeets = Vec::new();
        // bdeets.push("").push(bksplit[0]).push(bksplit[1]).push("").push("");
        // let bdeets = &["", bksplit[0], bksplit[1], "", ""].to_vec();
        bdeets.push("".to_string());
        bdeets.push(bk.0.clone());
        bdeets.push(bk.1.clone());
        bdeets.push("".to_string());
        bdeets.push("".to_string());

        let mut total = 0;
        for i in bv.iter() {
            bdeets.push(i.to_string());
            total += *i;
        }
        bdeets.push(total.to_string());
        let bdeets = bdeets;
        wtr.write_record(&bdeets).expect("error writing booths");
    }
    wtr.flush().expect("Failed to finalise writing booths");

    // eprintln!("\t\tDone!");
}
