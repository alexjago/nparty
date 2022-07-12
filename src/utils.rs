//! Assorted utility structs and functions.

use color_eyre::eyre::{Context, ContextCompat, Result};
use csv::StringRecord;
use inflector::cases::titlecase::to_title_case;
use std::char;
use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::{stdin, stdout, Cursor, Read, Seek, SeekFrom, Write};
use std::path;
use std::str::FromStr;

use super::term;
use SeekFrom::Start;

pub use ehttp::Response;

/// A number.
///
/// If you have more than 4 billion candidates on the ballot, something has gone very wrong.
pub type BallotNumber = u32;

/// A ticket code. These follow "Excel ordering": A, B, C, ..., Z, AA, AB, ...
pub type TicketString = String; // is this really needed? eh

/// A map with each entry representing a row of results.
///
/// * the keys are typically either a `{division}_{booth}` portmanteau or an SA1 ID.
/// * the values are a sequence of preference results in the same order that [`crate::booths::group_combos`] would give.
pub type PrefsMap = std::collections::BTreeMap<String, Vec<f64>>;

pub trait ToTicket {
    fn to_ticket(self) -> TicketString;
}

impl ToTicket for BallotNumber {
    /// Takes an integer like `31` and converts it to a ticket string like "AE".
    fn to_ticket(self) -> TicketString {
        /* Weird base-26 maths ahoy, especially since we don't actually have a zero digit
        Note these correspondences:

             1       A  1
            27      AA  (1*26) + 1
           703     AAA  (27*26) + 1
         18279    AAAA  (703*26) + 1
        475255   AAAAA  (18279 * 26) + 1

        We build our output from the least to the most significant digit.
        Subtract, modulo, shift right, recurse.
        */

        if self == 0 {
            // base case
            String::new()
        } else {
            let num = self - 1;
            let remainder = num % 26;
            let shift = (num - remainder) / 26;
            Self::to_ticket(shift) + &char::from_u32(remainder + 'A' as Self).unwrap().to_string()
        }
    }
}

pub trait ToBallotNumber {
    fn to_number(self) -> BallotNumber;
}

impl ToBallotNumber for TicketString {
    /// Takes a ticket string like "AE" and converts it to an integer like `31`
    ///
    /// N.B. `UG` is not handled specially.
    fn to_number(self) -> BallotNumber {
        let mut res = 0;
        let places = self.char_indices().count();
        for (i, c) in self.char_indices() {
            let p = 26_u32.pow((places - (i + 1)) as u32);
            let v = 1 + c.to_ascii_uppercase() as u32 - 'A' as u32;
            res += p * v;
        }
        res as BallotNumber
    }
}

impl ToBallotNumber for &str {
    fn to_number(self) -> BallotNumber {
        self.to_string().to_number()
    }
}

// converts pretty_number

/// Display `BallotNumbers` to a couple of significant figures and a relevant name.
pub trait PrettifyNumber {
    fn pretty_number(self) -> String;
}

impl PrettifyNumber for BallotNumber {
    fn pretty_number(self) -> String {
        const SCALE: [(BallotNumber, &str); 3] = [
            (1_000_000_000, "billion"),
            (1_000_000, "million"),
            (1_000, "thousand"),
        ];
        if self < SCALE[SCALE.len() - 1].0 {
            self.to_string()
        } else {
            for s in &SCALE {
                if self >= s.0 {
                    return format!("{} {}", self / s.0, s.1);
                }
            }
            String::new()
        }
    }
}

// In the prior Python project, `read_candidates(candsfile)` does the following:

// Generate a dictionary of candidate data from the supplied CSV.
//    `candsfile` must be a file-like object.
//    returns {state: {ticket: {surname:"foo", ballot_given_nm:"bar", ballot_number:123, party:"BAZ"} } } }

// The Rusty way is to define a struct for what gets returned, which seems eminently sensible
// we might need several things

#[derive(PartialEq, Eq, Hash, Debug, Serialize, Deserialize, Copy, Clone)]
#[allow(clippy::use_self)] // False positive: There's a bug here related to derives
#[allow(clippy::upper_case_acronyms)] // It's usual for these to be capitalised and there aren't contiguity issues
pub enum StateAb {
    ACT,
    NSW,
    NT,
    QLD,
    SA,
    TAS,
    VIC,
    WA,
}

pub trait ToStateAb {
    fn to_state_ab(self) -> StateAb;
}

impl ToStateAb for &str {
    fn to_state_ab(self) -> StateAb {
        match StateAb::from_str(self) {
            Ok(r) => r,
            Err(e) => panic!("{}", e),
        }
    }
}

impl FromStr for StateAb {
    type Err = &'static str;
    fn from_str(item: &str) -> std::result::Result<Self, Self::Err> {
        match item.to_uppercase().as_str() {
            "ACT" => Ok(Self::ACT),
            "NSW" => Ok(Self::NSW),
            "NT" => Ok(Self::NT),
            "QLD" => Ok(Self::QLD),
            "SA" => Ok(Self::SA),
            "TAS" => Ok(Self::TAS),
            "VIC" => Ok(Self::VIC),
            "WA" => Ok(Self::WA),
            _ => Err("Jurisdiction does not exist"),
        }
    }
}

impl std::convert::From<&str> for StateAb {
    fn from(item: &str) -> Self {
        item.to_state_ab()
    }
}

impl fmt::Display for StateAb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Deserialize, Copy, Clone)]
pub enum NominationType {
    H, // House
    S, // Senate
}

pub trait ToNominationType {
    fn to_nomination_type(self) -> NominationType;
}

impl ToNominationType for &str {
    fn to_nomination_type(self) -> NominationType {
        match self.to_uppercase().as_str().chars().next().unwrap() {
            'H' => NominationType::H,
            'S' => NominationType::S,
            _ => panic!(),
        }
    }
}

impl fmt::Display for NominationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// Define a candidate's data.
/// `surname` and `ballot_given_nm` and `party` should all be straightforward.
/// `ballot_number` represents a defined ordering for all (pseudo)candidates on the ballot.
/// All ticket votes, starting with A=1, then the candidates of A going down the ballot paper,
/// and finally all the ungrouped (UG) candidates.
#[derive(PartialEq, Eq, Hash, Debug)]
pub struct Candidate {
    pub surname: String,
    pub ballot_given_nm: String,
    pub ballot_number: BallotNumber,
    pub party: String,
}

pub type BallotPosition = BallotNumber;
/// position in their column i.e.
pub type Ticket = HashMap<BallotPosition, Candidate>;
pub type BallotPaper = HashMap<TicketString, Ticket>;
pub type CandsData = HashMap<StateAb, BallotPaper>;

// ^^ all this formalises the structure of the nested dicts

/// This represents a row in the candidates csv file.
/// Specifically, the fields we care about.
#[derive(Debug, Deserialize)]
struct CandidateRecord {
    pub nom_ty: NominationType,
    pub state_ab: StateAb,
    pub ticket: TicketString,
    pub ballot_position: BallotPosition,
    pub surname: String,
    pub ballot_given_nm: String,
    pub party_ballot_nm: String,
}

/// If a 2022 header is missing quotes around some values they'll be incorrectly split.
/// This unsplits them in a semi-intelligent fashion.
pub fn fix_prefs_headers(prefs_headers_raw: &StringRecord, atl_start: usize) -> Vec<String> {
    let mut prefs_headers_fixed: Vec<String> = Vec::with_capacity(prefs_headers_raw.len());

    // opening six are fine...
    for s in prefs_headers_raw.iter().take(atl_start) {
        prefs_headers_fixed.push(s.into());
    }

    // here we iterate over the entries, checking if they start with the expected TicketString
    // if they don't, then we assume their entry has been broken by lack of quoting
    // and join it back up to the previous entry
    let mut idx: BallotNumber = 0;
    for s in prefs_headers_raw.iter().skip(atl_start) {
        if s.starts_with("A:") {
            // set/reset
            idx = 1;
        }
        if s.starts_with(&(idx.to_ticket() + ":")) || s.starts_with("UG:") {
            // expected-case for ATLs (and UGs)
            prefs_headers_fixed.push(s.into());
            idx += 1;
        } else if s.starts_with(&((idx - 1).to_ticket() + ":")) {
            // second and subsequent BTL candidates of a ticket
            prefs_headers_fixed.push(s.into());
        } else {
            // if s does NOT start with a valid TicketString-colon...
            // coalesce
            let mut start = prefs_headers_fixed.pop().unwrap_or_default();
            start += ","; // put back the missing comma
            start += s;
            prefs_headers_fixed.push(start);
        }
    }
    prefs_headers_fixed
}

/// Read the candidates from a stream.
pub fn read_candidates<T>(candsfile: T) -> Result<CandsData>
where
    T: Read,
{
    let mut bigdict = CandsData::new();

    // iterate over file
    // In Python we do this as a DictReader.
    // In Rust define a custom struct `CandidateRecord` and use Serde
    // We want to take a file-like object here. T:Read should do it.

    let mut rdr = csv::Reader::from_reader(candsfile);

    for row in rdr.deserialize() {
        let cand_record: CandidateRecord =
            row.context("Could not understand a row in the candidates file")?;

        if cand_record.nom_ty != NominationType::S {
            continue;
        }
        //println!("{:?}", cand_record);

        bigdict
            .entry(cand_record.state_ab)
            .or_insert_with(BallotPaper::new);

        if !bigdict
            .get(&cand_record.state_ab)
            .context("TOCTOU")?
            .contains_key(&cand_record.ticket)
        {
            // create column
            bigdict
                .get_mut(&cand_record.state_ab)
                .context("TOCTOU")?
                .insert(cand_record.ticket.clone(), Ticket::new());

            // Create pseudocandidate if needed
            if cand_record.ticket != "UG" {
                bigdict
                    .get_mut(&cand_record.state_ab)
                    .context("TOCTOU")?
                    .get_mut(&cand_record.ticket)
                    .context("TOCTOU")?
                    .insert(
                        0,
                        Candidate {
                            surname: "TICKET".to_string(),
                            ballot_given_nm: "VOTE".to_string(),
                            ballot_number: cand_record.ticket.clone().to_number(),
                            party: cand_record.party_ballot_nm.clone(),
                        },
                    );
            }
        }

        bigdict
            .get_mut(&cand_record.state_ab)
            .context("TOCTOU")?
            .get_mut(&cand_record.ticket)
            .context("TOCTOU")?
            .insert(
                cand_record.ballot_position,
                Candidate {
                    surname: cand_record.surname,
                    ballot_given_nm: cand_record.ballot_given_nm,
                    ballot_number: 0, // we will update this below
                    party: cand_record.party_ballot_nm.clone(),
                },
            );
    }

    // Now iterate by state to fill in the `ballot_number`s

    for state in bigdict.values_mut() {
        let ticket_count = state.len();
        let mut ballot_number = (ticket_count - 1) as BallotNumber;

        for tnum in 1..ticket_count {
            let ticket = (tnum as BallotNumber).to_ticket();
            let candidate_count = state.get(&ticket).context("TOCTOU")?.len();
            for cnum in 1..candidate_count {
                // easiest way to skip pseuds
                ballot_number += 1;
                state
                    .get_mut(&ticket)
                    .context("TOCTOU")?
                    .get_mut(&(cnum as BallotPosition))
                    .context("TOCTOU")?
                    .ballot_number = ballot_number;
            }
        }
        if state.contains_key("UG") {
            let candidate_count = state.get("UG").unwrap().len();
            for cnum in 1..=candidate_count {
                ballot_number += 1;
                state
                    .get_mut("UG")
                    .context("TOCTOU")?
                    .get_mut(&(cnum as BallotPosition))
                    .context("TOCTOU")?
                    .ballot_number = ballot_number;
            }
        }
    }

    // We're done!
    Ok(bigdict)
}

/// This represents a row in the party csv file.
/// Specifically, the fields we care about.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[allow(dead_code)]
pub struct PartyRecord {
    state_ab: StateAb,
    party_ab: String,
    registered_party_ab: String,
    party_nm: String,
}

pub type PartyData = HashMap<String, String>;

/// Reads party abbreviations from the relevant file...
/// -> {(party name on ballot | party abbreviation) : party abbreviation}
pub fn read_party_abbrvs<T>(partyfile: T) -> PartyData
where
    T: Read,
{
    let mut bigdict = PartyData::new();

    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(partyfile);

    for pr in rdr.deserialize::<PartyRecord>().flatten() {
        // skip weird header rows and anything else
        if !pr.registered_party_ab.is_empty() {
            bigdict.insert(pr.registered_party_ab, to_title_case(&pr.party_ab));
        }
        bigdict.insert(pr.party_nm, to_title_case(&pr.party_ab));
    }

    bigdict
}

// next up is `filter_candidates`
// So at this point in the thing we have a dilemma. For `read_candidates` we used a
// Candidate struct that didn't use the ticket, since that was in the tree.
// But for this we want to return a Candidate struct that *does* include ticket...
// So we can either put ticket data into `Candidate` or we can make an almost identical type
// Very similar type it is!
// OK and then we need some regex crap
// Also this did very different things depending on whether it was a TTY.

pub struct FilteredCandidate {
    filter: regex::Regex,
    pub surname: String,
    pub ballot_given_nm: String,
    pub ballot_number: BallotNumber,
    pub party: String,
    pub ticket: String,
    cands_matches: [bool; 5],
}

impl fmt::Debug for FilteredCandidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FilteredCandidate {{\n  filter: {}\n  surname: {}\n  ballot_given_nm: {}\n  ballot_number: {}\n  party: {}\n  ticket: {}\n  matches: {:?}\n}}",
            self.filter, self.surname, self.ballot_given_nm, self.ballot_number, self.party, self.ticket, self.cands_matches)
        // no semicolon here, we're returning
    }
}

///
impl fmt::Display for FilteredCandidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}\n{}\n{}\n{}\n{}",
            self.surname, self.ballot_given_nm, self.ballot_number, self.party, self.ticket
        ) // no semicolon here, we're returning
    }
}

impl FilteredCandidate {
    pub fn fmt_tty(&self) -> String {
        let mut surname = self.surname.clone();
        let mut ballot_given_nm = self.ballot_given_nm.clone();
        let mut ballot_number = format!("{:4}", self.ballot_number);
        let mut party = self.party.clone();
        let mut ticket = self.ticket.clone();

        if self.cands_matches[0] {
            let s = self.filter.find(&self.surname).unwrap();
            surname = term::decorate_range(&surname, s.range(), term::UNDERLINE);
        }
        if self.cands_matches[1] {
            let s = self.filter.find(&self.ballot_given_nm).unwrap();
            ballot_given_nm = term::decorate_range(&ballot_given_nm, s.range(), term::UNDERLINE);
        }
        if self.cands_matches[2] {
            let s = self.filter.find(&ballot_number).unwrap();
            ballot_number = term::decorate_range(&ballot_number, s.range(), term::UNDERLINE);
        }
        if self.cands_matches[3] {
            let s = self.filter.find(&self.party).unwrap();
            party = term::decorate_range(&party, s.range(), term::UNDERLINE);
        }
        if self.cands_matches[4] {
            let s = self.filter.find(&self.ticket).unwrap();
            ticket = term::decorate_range(&ticket, s.range(), term::UNDERLINE);
        }
        format!(
            "{}\t{}\t{}\t{}\t{}",
            surname, ballot_given_nm, ballot_number, party, ticket
        ) // no semicolon here, we're returning
    }
}

/// Return a list of candidates matching some filter.
pub fn filter_candidates(
    candsdict: &CandsData,
    state: StateAb,
    filter: &str,
) -> Vec<FilteredCandidate> {
    let mut data = Vec::new();
    let filt = regex::RegexBuilder::new(filter)
        .case_insensitive(true)
        .build()
        .unwrap();
    for (tk, cands) in candsdict.get(&state).unwrap().iter() {
        for (_balnum, cv) in cands.iter() {
            // OK, field by field
            let mut cands_matches = [false; 5];
            cands_matches[0] = filt.is_match(&cv.surname);
            cands_matches[1] = filt.is_match(&cv.ballot_given_nm);
            cands_matches[2] = filt.is_match(&cv.ballot_number.to_string());
            cands_matches[3] = filt.is_match(&cv.party);
            cands_matches[4] = filt.is_match(tk);

            let any_match: bool = cands_matches.iter().fold(false, |acc, x| (acc | x));

            let disregard_if_ticket_literal =
                candsdict.get(&state).unwrap().contains_key(filter) & (filter != tk);
            // disregard filters that exactly specify OTHER ticket literals
            if any_match && !disregard_if_ticket_literal {
                data.push(FilteredCandidate {
                    filter: filt.clone(),
                    surname: cv.surname.clone(),
                    ballot_given_nm: cv.ballot_given_nm.clone(),
                    ballot_number: cv.ballot_number,
                    party: cv.party.clone().to_string(),
                    ticket: tk.to_string(),
                    cands_matches,
                });
            }
        }
    }
    // sort by candidate number
    data.sort_by(|a, b| a.ballot_number.cmp(&b.ballot_number));
    data
}

/// Opens a file, possibly zipped, for reading.
/// If the zipfile contains more than one file, the first will be returned.
/// Performance note: has to unzip and return the entire file.
pub fn open_csvz<T: 'static + Read + Seek>(mut infile: T) -> Result<Box<dyn Read>> {
    if is_zip(&mut infile)? {
        let mut zippah = zip::ZipArchive::new(infile).expect("error establishing the ZIP");
        let mut zippy = zippah.by_index(0).expect("no file in ZIP");
        // sigh. We're going to need to just go ahead and read the entire thing into memory here
        let zs: usize = zippy
            .size()
            .try_into()
            .with_context(|| format!("I don't support files greater than {} :(", usize::MAX))?;
        let mut bigbuf: Vec<u8> = Vec::with_capacity(zs);
        zippy.read_to_end(&mut bigbuf).expect("Error reading ZIP");
        Ok(Box::new(Cursor::new(bigbuf)))
    } else {
        Ok(Box::new(infile))
    }
}

/// opens blah.csv OR blah.zip
pub fn open_csvz_from_path(inpath: &path::Path) -> Result<Box<dyn Read>> {
    use std::ffi::OsStr;
    Ok(if inpath.exists() && inpath.is_file() {
        open_csvz(File::open(inpath)?)?
    } else {
        let ext = inpath.extension().unwrap_or_else(|| {
            panic!(
                "Could not find {:#?} whether compressed or not",
                inpath.display()
            )
        });
        if ext == OsStr::new("zip") {
            let newpath = inpath.with_extension("zip");
            open_csvz(File::open(newpath)?)?
        } else if ext == OsStr::new("csv") {
            let newpath = inpath.with_extension("csv");
            open_csvz(File::open(newpath)?)?
        } else {
            panic!(
                "Could not find {:#?} whether compressed or not",
                inpath.display()
            );
        }
    })
}

/// Peeks at the contents to check the magic number
/// slightly adapted from zip-extensions
/// to operate on a `Read+Seek` rather than a full `File`
pub fn is_zip<T>(infile: &mut T) -> Result<bool>
where
    T: Read + Seek,
{
    const ZIP_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];
    let pos = infile.seek(SeekFrom::Current(0))?;
    let mut buffer: [u8; 4] = [0; 4];
    let bytes_read = infile.read(&mut buffer)?;
    infile
        .seek(Start(pos))
        .context("couldn't seek back to the start after testing whether a file was a ZIP")?; // revert
    if bytes_read == buffer.len() && bytes_read == ZIP_SIGNATURE.len() {
        for i in 0..ZIP_SIGNATURE.len() {
            if buffer[i] != ZIP_SIGNATURE[i] {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    Ok(false)
}

/// Get a Writer to a file in a ZIP or die trying!
/// Will create a ZIP file with a single inner file, named the same as the ZIP bar the extension.
pub fn get_zip_writer_to_path(
    outpath: &path::Path,
    inner_ext: &str,
) -> Result<zip::ZipWriter<File>> {
    let mut outfile = zip::ZipWriter::new(
        File::create(&outpath.with_extension("zip")).expect("Couldn't create new output file"),
    );
    outfile.start_file(
        outpath
            .with_extension(inner_ext)
            .file_name()
            .context("no file name in path")?
            .to_str()
            .context("could not convert path to string")?,
        zip::write::FileOptions::default(),
    )?;
    Ok(outfile)
}

/// Get user input live, given a prompt, like the Python function of the same name.
///  
/// Credit to /u/Ophekkis
/// <https://www.reddit.com/r/rust/comments/fyjmbv/n00b_question_how_to_get_user_input/fn0d5va/>
pub fn input(prompt: &str) -> std::io::Result<String> {
    let mut stdout = stdout();
    write!(&mut stdout, "{}", prompt)?;
    stdout.flush()?;
    let mut response = String::new();
    stdin().read_line(&mut response)?;
    Ok(response.trim().to_string())
}

/// Fetch a URL in a blocking fashion despite async interface of `ehttp`.
/// Uses a `sync::mpsc::channel` under the hood.
pub fn fetch_blocking(url: impl ToString) -> Result<ehttp::Response, String> {
    let (sender, receiver) = std::sync::mpsc::channel();
    let req = ehttp::Request::get(url);
    ehttp::fetch(req, move |r| sender.send(r).unwrap());
    receiver.recv().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_ballot_number_conversions() {
        assert_eq!("AE", 31.to_ticket());
        assert_eq!(31, "AE".to_number());
        assert_eq!("123 thousand", 123000.pretty_number());
    }
    #[test]
    fn test_state_ab_conversions() {
        assert_eq!("ACT", StateAb::ACT.to_string());
        assert_eq!(StateAb::NSW, StateAb::from("nsw"));
        assert!(StateAb::from_str("this is not a state").is_err());
    }
}
