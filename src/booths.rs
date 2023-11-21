//! The n-party-preferred *distribution* phase.
use super::term;
use super::utils::{fix_prefs_headers, open_csvz_from_path, StateAb};
/// We want to reduce each unique preference sequence to some ordering
///    of each of the parties. For example, for four parties there are 65 orderings:
///   `(0!) + (4 * 1!) + (6 * 2!) + (4 * 3!) + (4!)`
///
/// In fact, there are even more orderings (voters might interleave candidates)
/// but we will consider the most-preferred candidate from each party as
/// representing it (e.g. a vote `A1 > B1 > B2 > B3 > A2 > A3` as `A > B`).
use color_eyre::eyre::{eyre, Context, ContextCompat, Result};
use color_eyre::Section;
use factorial::Factorial;
use indexmap::IndexMap;
use itertools::Itertools;
use std::collections::{BTreeMap, HashMap};
use std::fs::create_dir_all;
use std::path::Path;
use string_interner::{backend::StringBackend, symbol::SymbolU16, StringInterner};
use tracing::{info, trace};

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

/// Special votes will contain one of these strings in the booth name
const NON_BOOTH_CONVERT: [&str; 4] = ["ABSENT", "POSTAL", "PRE_POLL", "PROVISIONAL"];

/// Convert the name of a "special" vote
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
    use std::io::IsTerminal;
    if std::io::stderr().is_terminal() {
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
pub fn group_combos(groups: &[&str]) -> Combinations {
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

/// The headers, essentially
pub type Combinations = Vec<String>;

/// A mapping between a party ID and a (pseudo)candidate number
/// (such numbers are relative column indexes)
type Groups = HashMap<usize, Vec<usize>>;

/// A mapping from an order of [`Groups`] keys, to an index into [`Combinations`].
///
/// Suppose we had N parties labelled as 0 through N-1. This allows us to
/// take an *ordering* of those parties, say `[0, 1]` and get the corresponding
/// index in the output of [`group_combos`]. In turn, that index can stand for
/// the ordering (which is itself a reduction of the ballot).
type ComboTree = HashMap<Vec<usize>, usize>;

/// It doesn't matter what the group ordering is as long as it's consistent...
fn make_combo_tree(groups_count: usize) -> ComboTree {
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

    // String Interning: because u16s are much cheaper keys than strings are
    let mut interner = StringInterner::<StringBackend<SymbolU16>>::new();

    info!("\tLoading polling places and candidates");
    let booths = load_polling_places(state, polling_places_path, &mut interner)?;

    // The 2019 format is that there are a few fixed headers ... and then a field for each [pseudo]candidate
    let mut prefs_rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .escape(Some(b'\\'))
        // .trim(csv::Trim::Fields) // Trimming at this stage more than doubles run time
        .from_reader(open_csvz_from_path(formal_prefs_path)?);

    let prefs_headers = prefs_rdr.headers()?.clone();
    trace!("\nNo actual preferences processed yet, but we successfully opened the zipfile and the raw headers look like this:\n{:#?}", prefs_headers);

    let above_start = PREFS_FIELD_NAMES.len();
    // 2022 lack-of-quoting problems
    let prefs_headers_fixed = fix_prefs_headers(&prefs_headers, above_start);

    /* ***** Get candidate/party/group info ***** */
    let (combinations, below_start, groups_above, groups_below) =
        make_candidate_info(parties, &prefs_headers_fixed, above_start)?;

    let mut below_groups: Vec<usize> = vec![usize::MAX; prefs_headers_fixed.len()];
    for (g, v) in &groups_below {
        for c in v {
            below_groups[*c + above_start - 1] = *g;
        }
    }
    // trace!("groups_below: {:?}", groups_below);
    // trace!("below_groups: {:?}", below_groups);

    /* ***** Start of main iteration ***** */
    info!("\tDistributing Preferences");
    eprintln!(); // still a normal eprintln for progress-jump reasons

    // Store all the things! DivBooth : rest of the derived columns
    let mut booth_counts: HashMap<DivBooth, Vec<usize>> = HashMap::new();
    let mut progress: usize = 0; // Diagnostics
    let mut btl_count: usize = 0; // Diagnostics

    // Hoists
    let mut bests: Vec<(usize, usize)> =
        Vec::with_capacity(groups_below.len().max(groups_above.len()));
    let mut order: Vec<usize> = Vec::with_capacity(bests.len());
    // let mut record = csv::StringRecord::new(); // Performance: <https://blog.burntsushi.net/csv/#amortizing-allocations>
    let mut record =
        csv::ByteRecord::with_capacity(prefs_headers_fixed.capacity(), prefs_headers_fixed.len());
    // while prefs_rdr.read_record(&mut record)? {
    while prefs_rdr.read_byte_record(&mut record)? {
        // String interning in action
        // let divnm = interner.get_or_intern(&record[1]);
        // let boothnm = interner.get_or_intern(&record[2]);
        let divnm = interner.get_or_intern(std::str::from_utf8(&record[1])?);
        let boothnm = interner.get_or_intern(std::str::from_utf8(&record[2])?);

        if (record[1]).starts_with(b"---") {
            // ^^ This conditional might be inverted for testing; 2019+ files do NOT contain a `---` line.
            return Result::Err(eyre!("Preferences file is in the 2016 format."))
                .suggestion("Upgrade the file to the 2019+ format with:\n\tnparty upgrade prefs");
        }
        /* // Saving for reference
        // First we must determine if it's ATL or BTL, then select appropriate candidates.
        let is_btl: bool = check_btl(&record, below_start);
        btl_count += if is_btl { 1 } else { 0 };
        let groups_which = if is_btl { &groups_below } else { &groups_above };

        // Next, actually distribute the preference.
        let pref_idx_old = distribute_preference(
            &record,
            groups_which,
            &combo_tree,
            above_start,
            prefs_headers_fixed.len() - above_start,
            &mut bests,
            &mut order,
        );
        */

        let pref_idx = handle_below(
            &record,
            below_start,
            &below_groups,
            &mut bests,
            &mut order,
            groups_below.len(),
            &mut btl_count,
        )
        .unwrap_or_else(|| {
            distribute_preference(
                &record,
                &groups_above,
                // &combo_tree,
                above_start,
                prefs_headers_fixed.len() - above_start,
                &mut bests,
                &mut order,
            )
        });

        /* // Saving for reference
        // if pref_idx != pref_idx_old {
        //     panic!(
        //         "Difference in result: old was {} but new is {} on iteration{}\n{}\nbests: {:?}",
        //         combinations[pref_idx_old],
        //         combinations[pref_idx],
        //         progress,
        //         record
        //             .iter()
        //             .zip(prefs_headers_fixed)
        //             .filter(|(v, _)| !v.is_empty())
        //             .map(|(v, k)| format!("{}\t{}\n", k, v))
        //             .collect::<String>(),
        //         bests
        //     );
        // } */

        // ... and store.
        let divbooth: DivBooth = (divnm, boothnm);
        let booth = booth_counts
            .entry(divbooth)
            .or_insert_with(|| vec![0_usize; combinations.len()]);
        booth[pref_idx] += 1;

        progress += 1;
        if progress % 100_000 == 0 {
            trace!("{:?}", record);
            info!(
                "{}\t\tPreferencing progress: {} ballots",
                ttyjump(),
                progress
            );
        }
    }

    info!(
        "{}\t\tPreferencing complete: {} ballots ({} were BTL)",
        ttyjump(),
        progress,
        btl_count
    );
    trace!(
        "Interned {} strings, with capacity for {}.",
        interner.len(),
        u16::MAX
    );
    /* ***** End of main iteration ***** */

    info!("\t\tAggregating Absents, Postals, Prepolls & Provisionals");
    let division_specials = aggregate_specials(&mut booth_counts, &combinations, &interner);

    info!("\t\tWriting File");
    write_output(
        npp_booths_path,
        &combinations,
        &booth_counts,
        division_specials,
        &booths,
        &interner,
    )
}

/// Load the polling places data from a path
#[inline(never)]
pub fn load_polling_places(
    state: StateAb,
    polling_places_path: &Path,
    interner: &mut StringInterner<StringBackend<SymbolU16>>,
) -> Result<HashMap<DivBooth, BoothRecord>> {
    // this is now just for actual booth data
    // For some gods-forsaken reason, the PollingPlaceID is not the Vote Collection Point ID
    // The only consistent identifier is ({Division}, {Booth})
    let mut booths: HashMap<DivBooth, BoothRecord> = HashMap::new();

    // OK, let's figure out polling places
    let mut pp_rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .has_headers(false)
        .from_path(polling_places_path)?;
    // 2019 problems: there's a pre-header line
    // we need to skip it, and we're going to do so manually.

    let pp_rdr_iter = pp_rdr.records();
    let mut row_count: usize = 0;

    for result in pp_rdr_iter.skip(2) {
        row_count += 1;
        let record: BoothRecord = result?.deserialize(None)?;
        if record.State != state {
            continue;
        }
        let division_nm = interner.get_or_intern(record.DivisionNm.clone());
        let booth_nm = interner.get_or_intern(record.PollingPlaceNm.clone());
        let dvb = (division_nm, booth_nm);
        booths.insert(dvb, record);
    }
    trace!("Loaded {} polling places", row_count - 2);
    Ok(booths)
}

/// Assemble all the candidate information from the [`Parties`] and the pref file headers.
/// Returns FIVE items:
/// 0. All the group name [`Combinations`]
/// 1. The [`ComboTree`] that acts as a LUT to column indexes in (0);
/// 2. The (absolute) starting index of the BTL candidates in each ballot record
/// 3. The ATL [`Groups`]
/// 4. The BTL [`Groups`]
#[inline(never)]
pub fn make_candidate_info(
    parties: &Parties,
    prefs_headers_fixed: &[String],
    above_start: usize,
) -> Result<(Combinations, usize, Groups, Groups)> {
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

    // The first ticket is labelled "A" and there are >= two candidates per ticket.
    // All tickets come before all candidates in the column order.
    // So the _second_ "A:", if it exists, is the initial BTL field.
    // If it doesn't exist then _all_ we have are UnGrouped candidates
    // (and they start at what we would normally consider `above_start`)
    let below_start = prefs_headers_fixed
        .iter()
        .enumerate()
        .skip(above_start + 1)
        .filter(|(_, s)| s.starts_with("A:"))
        .map(|(i, _)| i)
        .next()
        .unwrap_or(above_start);

    // Create candidate number index
    // We index fields by "TICKETCODE:LASTNAME Given Names"
    let mut cand_nums: HashMap<&str, usize> = HashMap::new();
    for (i, pref) in prefs_headers_fixed.iter().skip(above_start).enumerate() {
        cand_nums.insert(pref, 1 + i);
    }

    // set up some lookups...
    // A mapping between a party ID and a (pseudo)candidate number
    // (such numbers are relative column indexes)
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    // A mapping between a party ID and an ATL ticket number
    let mut groups_above: HashMap<usize, Vec<usize>> = HashMap::new();
    // A mapping between a party ID and a BTL candidate number
    let mut groups_below: HashMap<usize, Vec<usize>> = HashMap::new();

    for (party, cand_list) in parties {
        let mut party_cand_nums = Vec::new();
        let mut above_cands = Vec::new();
        let mut below_cands = Vec::new();
        for cand in cand_list {
            let cn = cand_nums
                .get::<str>(cand)
                .context("missing cand_num")?
                .to_owned();
            party_cand_nums.push(cn);
            if cn > (below_start - above_start) {
                // remember, below_start and above_start are relative to the file headers
                // NOT the candidate numbers
                below_cands.push(cn);
            } else {
                above_cands.push(cn);
            }
        }
        let p_idx = *party_indices
            .get(party.as_str())
            .with_context(|| format!("The party/group {party} is missing from party_indices"))?;
        groups.insert(p_idx, party_cand_nums);
        groups_above.insert(p_idx, above_cands);
        groups_below.insert(p_idx, below_cands);
    }

    trace!("cand_nums:\n{}", {
        let mut a: Vec<(&str, usize)> = Vec::new();
        a.extend(cand_nums);
        a.sort_by(|&(_, a), &(_, b)| a.cmp(&b));
        a.iter().map(|(s, u)| format!("{u:4}\t{s}")).join("\n")
    });
    trace!("Full Groups: {:?}", groups);
    trace!("Above Starts At: {}", above_start);
    trace!("ATL Groups: {:?}", groups_above);
    trace!("BTL Groups: {:?}", groups_below);
    trace!("Below Starts At: {}", below_start);
    // trace! {"Prefs headers fixed, in bytes:\n{:#?}", prefs_headers_fixed};

    Ok((combinations, below_start, groups_above, groups_below))
}

/*  // Saving for reference
/// Determine whether a preference record is a formal vote Below The Line.
///
/// Per section 268A of the Commonwealth Electoral Act, a vote is BTL-formal if it has
/// at least `[1]` through `[6]` marked BTL (and BTL-formality takes priority).
/// (If there are fewer than 6 candidates, all squares must be marked)
/// <http://classic.austlii.edu.au/au/legis/cth/consol_act/cea1918233/s268a.html>
// NOTE 2020-05-14: I am quite confident this ATL-vs-BTL code is correct.
// It produces the correct number of BTLs...
#[inline(never)]
pub fn check_btl(record: &csv::StringRecord, below_start: usize) -> bool {
    if record.len() > below_start {
        let mut btl_counts: [usize; 6] = [0; 6]; // NOTE: zero-indexing and potential fragility with wrapping adds.
                                                 // The latter is only a problem if there are more than usize::MAX candidates BTL
        for v in record.iter().skip(below_start) {
            match v {
                // Can we really rely on the lack of whitespace? We may need to trim() again
                // Not doing it does save like 0.1 seconds per run though, that's 5%
                "1" => btl_counts[0] += 1,
                "2" => btl_counts[1] += 1,
                "3" => btl_counts[2] += 1,
                "4" => btl_counts[3] += 1,
                "5" => btl_counts[4] += 1,
                "6" => btl_counts[5] += 1,
                _ => continue,
            }
        }
        // If each element of `btl_counts` is equal to exactly 1 then we have a valid 1-to-6 sequence.
        // The `.take()` clause accounted for the (unlikely) case where there are fewer than 6 BTL candidates.
        // ... except it results in the wrong number of BTLs.
        btl_counts.iter().all(|c| *c == 1)
    } else {
        false // If it's too short to be a BTL ballot, it's not.
    }
}
*/

/// Determine whether a preference record is a formal vote Below The Line.
/// And if so, return its preference index.
///
/// Per section 268A of the Commonwealth Electoral Act, a vote is BTL-formal if it has
/// at least `[1]` through `[6]` marked BTL (and BTL-formality takes priority).
/// (If there are fewer than 6 candidates, all squares must be marked)
/// <http://classic.austlii.edu.au/au/legis/cth/consol_act/cea1918233/s268a.html>
// NOTE 2022-07-14: I am quite confident this ATL-vs-BTL code is correct.
// It produces the correct number of BTLs and it has been fairly exhaustively checked against
// the previous version of the code.
#[inline(never)]
pub fn handle_below(
    record: &csv::ByteRecord,
    below_start: usize,
    below_groups: &[usize],
    bests: &mut Vec<(usize, usize)>,
    order: &mut Vec<usize>,
    groups_count: usize,
    count: &mut usize,
) -> Option<usize> {
    if record.len() > below_start {
        bests.clear();
        order.resize(groups_count, usize::MAX); // trying this
        order.fill(usize::MAX);
        let mut btl_counts: [usize; 6] = [0; 6]; // NOTE: zero-indexing and potential fragility with wrapping adds.
                                                 // The latter is only a problem if there are more than usize::MAX candidates BTL
        for (i, v) in record
            .iter()
            .enumerate()
            .skip(below_start)
            .filter(|(_, s)| !s.is_empty())
            // .map(|(i, s)| (i, s.parse::<usize>().unwrap()))
            .map(|(i, s)| (i, parse_u8_b10(s)))
        {
            match v {
                // Can we really rely on the lack of whitespace? We may need to trim() again
                // Not doing it does save like 0.1 seconds per run though, that's 5%
                1 => btl_counts[0] += 1,
                2 => btl_counts[1] += 1,
                3 => btl_counts[2] += 1,
                4 => btl_counts[3] += 1,
                5 => btl_counts[4] += 1,
                6 => btl_counts[5] += 1,
                _ => (),
            }

            let g = below_groups[i];
            if g < usize::MAX && v < order[g] {
                // ^^^ 2023-11-21 BUG with 2022's ACT_3CP
                // g can be greater than order.len()
                // (specifically because order was empty)
                // fixed an issue where candidates weren't allocated correctly
                // but we can still get a 0/0 here
                order[g] = v;
            }
        }
        // If each element of `btl_counts` is equal to exactly 1 then we have a valid 1-to-6 sequence.
        // The `.take()` clause accounted for the (unlikely) case where there are fewer than 6 BTL candidates.
        // ... except it results in the wrong number of BTLs.
        if btl_counts.iter().all(|c| *c == 1) {
            *count += 1;

            for (i, v) in order.iter().enumerate() {
                if *v < usize::MAX {
                    bests.push((*v, i));
                }
            }
            // Sort by bests, then convert to the order of indices
            // (Unstable sort is in-place and there shouldn't be any equal elements anyway)
            bests.sort_unstable();
            order.clear(); // this is very necessary!
            order.extend(bests.iter().map(|(_, p)| p));
            // panic!(
            //     "btl with bests: {:?} and order {:?} from record {:#?}",
            //     bests, order, record
            // );
            Some(calculate_index(order, groups_count))
        } else {
            None
        }
    } else {
        None // If it's too short to be a BTL ballot, it's not.
    }
}

/// Distribute the preference of a single ballot to an ordering of the specified [`Groups`].
///
/// Having determined the ballot's ATL/BTL status we determine the "best" preference
/// (if any) for each party we care about. (Candidates may be interleaved.)  
/// Then sort and determine the index into the relevant [`Combinations`].
/// For performance reasons, `bests` is hoisted.
#[inline(never)]
pub fn distribute_preference(
    record: &csv::ByteRecord,
    groups: &Groups,
    // combo_tree: &ComboTree,
    above_start: usize,
    cands_count: usize,
    bests: &mut Vec<(usize, usize)>,
    order: &mut Vec<usize>,
) -> usize {
    bests.clear();
    order.clear();
    for (group_num, candidate_nums) in groups {
        // For each group, iterate over its candidates and get the best-preferenced.
        let mut cur_best = cands_count;
        for i in candidate_nums {
            // ^^ Performance: candidate_nums *should* be disjoint across iterations...
            if let Some(x) = record.get(i + above_start - 1) {
                // ^^ always check: is this the right offset?
                if x.is_empty() {
                    continue;
                }
                let bal = parse_u8_b10(x);
                // if let Ok(bal) = x.trim().parse() {
                // ^^ Many if not most entries are empty...
                if bal < cur_best {
                    cur_best = bal;
                }
                // }
            }
        }
        if cur_best < cands_count {
            bests.push((cur_best, *group_num));
        }
    }

    // Sort by bests, then convert to the order of indices
    // (Unstable sort is in-place and there shouldn't be any equal elements anyway)
    bests.sort_unstable();
    order.extend(bests.iter().map(|x| x.1));
    calculate_index(order, groups.len())
    // panic!(
    //     "{:?}\nbests: {:?}\torder: {:?}\tindex: {}",
    //     record, bests, order, idx
    // );
}

/// Aggregate the "special" booths by Division, removing them from the main structure
/// Initially, the special votes are split up into e.g. `POSTAL_1` through `POSTAL_8`
/// (For backwards compatibility we'd like to print them at the end of the file)
#[inline(never)]
pub fn aggregate_specials(
    booth_counts: &mut HashMap<DivBooth, Vec<usize>>,
    combinations: &[String],
    interner: &StringInterner<StringBackend<SymbolU16>>,
) -> BTreeMap<(String, String), Vec<usize>> {
    let mut division_specials: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();

    let mut to_remove = Vec::new();

    for (bk, bv) in &*booth_counts {
        for w in &NON_BOOTH_CONVERT {
            // hoisting for file order
            let divbooth = (
                interner.resolve(bk.0).unwrap().to_string(),
                non_booth_convert(w).to_string(),
            );
            let db = division_specials
                .entry(divbooth)
                .or_insert_with(|| vec![0_usize; bv.len()]);
            if interner.resolve(bk.1).unwrap().contains(w) {
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
    division_specials
}

/// Write the output CSV for the distribution stage.
/// Format: `{NPP_FIELD_NAMES} + {combinations} + Total`
#[inline(never)]
pub fn write_output(
    npp_booths_path: &Path,
    combinations: &[String],
    booth_counts: &HashMap<DivBooth, Vec<usize>>,
    division_specials: BTreeMap<(String, String), Vec<usize>>,
    booths: &HashMap<DivBooth, BoothRecord>,
    interner: &StringInterner<StringBackend<SymbolU16>>,
) -> Result<()> {
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
    for i in combinations {
        npp_header.push(i.as_str());
    }
    npp_header.push("Total");

    wtr.write_record(npp_header)
        .context("error writing booths header")?;

    // Switching to string interning messed up the file order a little bit.
    // We'd like it to be sorted by ({division name}, {polling place name})
    // for all ordinary divisions, then the specials separately after ---
    // this matches previous behaviour.
    // (when sorted, old and new files have identical hashes,
    //    so we can be confident in the rest of everything)

    let mut sorted_booths: Vec<&(SymbolU16, SymbolU16)> = booth_counts.keys().collect();
    sorted_booths.sort_by_cached_key(|(div_id, booth_id)| {
        (
            interner.resolve(*div_id).unwrap(),
            interner.resolve(*booth_id).unwrap(),
        )
    });

    for bk in sorted_booths {
        let bv = booth_counts
            .get(bk)
            .context("missing entry in `booth_counts`")?;
        let br = booths.get(bk).with_context(|| {
            eyre!(
                "It's really weird, but {:?} (actually {:?}) isn't in `booths`.",
                bk,
                (interner.resolve(bk.0), interner.resolve(bk.1))
            )
        })?;
        let mut bdeets = vec![
            br.PollingPlaceID.to_string(),
            br.DivisionNm.clone(),
            br.PollingPlaceNm.clone(),
            br.Latitude.clone(),
            br.Longitude.clone(),
        ];
        let mut total = 0;
        for i in bv {
            bdeets.push(i.to_string());
            total += *i;
        }
        bdeets.push(total.to_string());
        let bdeets = bdeets;
        wtr.write_record(&bdeets).context("error writing booths")?;
    }

    wtr.flush().context("error writing booths")?;

    for (bk, bv) in division_specials {
        let mut bdeets: Vec<String> =
            vec![String::new(), bk.0, bk.1, String::new(), String::new()];

        let mut total = 0;
        for i in bv {
            bdeets.push(i.to_string());
            total += i;
        }
        bdeets.push(total.to_string());
        let bdeets = bdeets;
        wtr.write_record(&bdeets).context("error writing booths")?;
    }
    wtr.flush().context("Failed to finalise writing booths")?;
    Ok(())
}

/// Calculate a preference index given an ordering
/// not gonna lie, this is pretty cursed™
#[inline(never)]
pub fn calculate_index(order: &[usize], groups_count: usize) -> usize {
    let mut idx = 0_usize;

    // eprintln!("order: {:?},    groups_count:{groups_count}", order);

    if order.is_empty() || groups_count == 0 {
        return 0;
    }

    // Shorter lengths
    // SUM (N!/(N-i)!); for i in [0, L-1]; L <= N
    for i in 0..order.len() {
        let mut q: usize = 1;
        for j in 0..i {
            q *= groups_count - j;
        }
        idx += q;
    }

    // tail recursion over remaining entries
    for o in 0..order.len() {
        let n = groups_count - o;
        let l = order.len() - o;

        // `a` is `order[o]` but adjusted for any
        // "earlier" entries that were smaller
        let a = order[o] - order.iter().take(o).filter(|x| **x < order[o]).count();

        if a > 0 {
            // Earlier entries for this-level length
            // If there are N remaining parties, remaining length is L,
            // we've already processed O entries and
            // our adjusted index is A, then...
            // there are A * ( (N - 1) choose (L - 1)) entries before this one
            // <https://en.wikipedia.org/wiki/Binomial_coefficient#Multiplicative_formula>
            let mut t = a;
            // let mut ii = 1;
            for i in 1..l {
                t *= n - i;
            }
            idx += t; // / ii;

            // eprintln!("    o:{o},    n:{n},    l:{l},    a:{a},   t:{t}   =>  {idx}");
        }
    }

    idx
}

// not only buggy, but slower somehow!
// it was buggy because you used hex constants
/// Parse a `&[u8]` as though it were an ASCII base-10 string
/// skipping over any bytes not corresponding to ascii 1-10
#[inline(never)]
pub fn parse_u8_b10(input: &[u8]) -> usize {
    // eprintln!("{input:?}");
    let mut acc: usize = 0;

    for k in input {
        match k {
            48 => {
                // ascii 0
                acc *= 10;
            }
            49..=57 => acc = acc * 10 + ((*k - 48) as usize),
            _ => continue,
        }
        // eprintln!("\t{k} {acc}");
    }

    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_calculate_index() {
        // *** First-of-the-length ***
        // Empty
        assert_eq!(calculate_index(&[], 4), 0);

        // AlpGrn
        assert_eq!(calculate_index(&[0, 1], 4), 5);

        // AlpGrnLib
        assert_eq!(calculate_index(&[0, 1, 2], 4), 17);

        // AlpGrnLibPhn
        assert_eq!(calculate_index(&[0, 1, 2, 3], 4), 41);

        // *** Last-of-the-length ***

        // PhnLib
        assert_eq!(calculate_index(&[3, 2], 4), 16);

        // PhnLibGrn
        assert_eq!(calculate_index(&[3, 2, 1], 4), 40);

        // PhnLibGrnAlp
        assert_eq!(calculate_index(&[3, 2, 1, 0], 4), 64);

        // *** In-betweens ***
    }

    #[test]
    fn auto_combinator() {
        for groups_count in 0..10 {
            let uut = make_combo_tree(groups_count);
            for (order, idx) in uut {
                assert_eq!(calculate_index(&order, groups_count), idx);
            }
        }
    }

    #[test]
    fn u8_b10_test() {
        assert_eq!(0, parse_u8_b10(b""));

        assert_eq!(0, parse_u8_b10(b"   "));

        assert_eq!(100, parse_u8_b10(b" 1 0 0"));

        for i in 0..255 {
            assert_eq!(i, parse_u8_b10(i.to_string().as_bytes()));
        }
    }
}
