#![allow(clippy::upper_case_acronyms)]
// use log;
use inflector::cases::titlecase::to_title_case;
use std::char;
use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::{stdin, stdout, Cursor, Read, Seek, SeekFrom, Write};
use std::path;

use super::term;
use SeekFrom::Start;

pub type BallotNumber = u32; // if you have more than 4 billion candidates on the ballot, something has gone very wrong
pub type TicketString = String; // is this really needed? eh

// deprecate printv in favour of using `log`

// The next section converts number_to_ticket and ticket_to_number

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
            Self::to_ticket(shift) + &char::from_u32(remainder + 'A' as u32).unwrap().to_string()
        }
    }
}

pub trait ToBallotNumber {
    fn to_number(self) -> BallotNumber;
}

impl ToBallotNumber for TicketString {
    /// Takes a ticket string like "AE" and converts it to an integer like `31`
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

/// Display BallotNumbers to a couple of significant figures and a relevant name.
pub trait PrettifyNumber {
    fn pretty_number(self) -> String;
}

impl PrettifyNumber for BallotNumber {
    fn pretty_number(self) -> String {
        const SCALE: [(BallotNumber, &str); 3] = [
            (1000000000, "billion"),
            (1000000, "million"),
            (1000, "thousand"),
        ];
        if self < SCALE[SCALE.len() - 1].0 {
            self.to_string()
        } else {
            for s in SCALE.iter() {
                if self >= s.0 {
                    return format!("{} {}", self / s.0, s.1);
                }
            }
            String::new()
        }
    }
}

// In the Python, `read_candidates(candsfile)` does the following:

// Generate a dictionary of candidate data from the supplied CSV.
//    `candsfile` must be a file-like object.
//    returns {state: {ticket: {surname:"foo", ballot_given_nm:"bar", ballot_number:123, party:"BAZ"} } } }

// The Rusty way is to define a struct for what gets returned, which seems eminently sensible
// we might need several things

#[derive(PartialEq, Eq, Hash, Debug, Serialize, Deserialize, Copy, Clone, enum_utils::FromStr)]
#[enumeration(case_insensitive)]
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
        match self.to_uppercase().as_str() {
            "ACT" => StateAb::ACT,
            "NSW" => StateAb::NSW,
            "NT" => StateAb::NT,
            "QLD" => StateAb::QLD,
            "SA" => StateAb::SA,
            "TAS" => StateAb::TAS,
            "VIC" => StateAb::VIC,
            "WA" => StateAb::WA,
            _ => panic!("Jurisdiction does not exist!"),
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

pub fn read_candidates<T>(candsfile: T) -> CandsData
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
        let cand_record: CandidateRecord = row.unwrap();

        if cand_record.nom_ty != NominationType::S {
            continue;
        }
        //println!("{:?}", cand_record);

        bigdict
            .entry(cand_record.state_ab)
            .or_insert_with(BallotPaper::new);

        if !bigdict
            .get(&cand_record.state_ab)
            .unwrap()
            .contains_key(&cand_record.ticket)
        {
            // create column
            bigdict
                .get_mut(&cand_record.state_ab)
                .unwrap()
                .insert(cand_record.ticket.clone(), Ticket::new());

            // Create pseudocandidate if needed
            if cand_record.ticket != "UG" {
                bigdict
                    .get_mut(&cand_record.state_ab)
                    .unwrap()
                    .get_mut(&cand_record.ticket)
                    .unwrap()
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
            .unwrap()
            .get_mut(&cand_record.ticket)
            .unwrap()
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

    for (_, state) in bigdict.iter_mut() {
        let ticket_count = state.len();
        let mut ballot_number = (ticket_count - 1) as BallotNumber;

        for tnum in 1..ticket_count {
            let ticket = (tnum as BallotNumber).to_ticket();
            let candidate_count = state.get(&ticket).unwrap().len();
            for cnum in 1..candidate_count {
                // easiest way to skip pseuds
                ballot_number += 1;
                state
                    .get_mut(&ticket)
                    .unwrap()
                    .get_mut(&(cnum as BallotPosition))
                    .unwrap()
                    .ballot_number = ballot_number;
            }
        }
        if state.contains_key("UG") {
            let candidate_count = state.get("UG").unwrap().len();
            for cnum in 1..=candidate_count {
                ballot_number += 1;
                state
                    .get_mut("UG")
                    .unwrap()
                    .get_mut(&(cnum as BallotPosition))
                    .unwrap()
                    .ballot_number = ballot_number;
            }
        }
    }

    // We're done!
    bigdict
}

/// This represents a row in the party csv file.
/// Specifically, the fields we care about.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
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

    let mut rowcounter = 0;

    for result in rdr.deserialize() {
        rowcounter += 1;
        if rowcounter <= 2 {
            continue; // skip useless metadata starter rows
        }
        let pr: PartyRecord = result.unwrap();

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
    pub ballot_number: String,
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
        let mut ballot_number = self.ballot_number.clone();
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
            let s = self.filter.find(&self.ballot_number).unwrap();
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

pub fn filter_candidates(
    candsdict: &CandsData,
    state: &StateAb,
    filter: &str,
) -> Vec<FilteredCandidate> {
    let mut data = Vec::new();
    let filt = regex::RegexBuilder::new(filter)
        .case_insensitive(true)
        .build()
        .unwrap();
    for (tk, cands) in candsdict.get(state).unwrap().iter() {
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
                candsdict.get(state).unwrap().contains_key(filter) & (filter != tk);
            // disregard filters that exactly specify OTHER ticket literals
            if any_match & !disregard_if_ticket_literal {
                data.push(FilteredCandidate {
                    filter: filt.clone(),
                    surname: cv.surname.clone(),
                    ballot_given_nm: cv.ballot_given_nm.clone(),
                    ballot_number: cv.ballot_number.clone().to_string(),
                    party: cv.party.clone().to_string(),
                    ticket: tk.to_string(),
                    cands_matches,
                });
            }
        }
    }
    data
}

trait ReadSeek: Read + Seek {}

/// Opens a file, possibly zipped, for reading.
/// If the zipfile contains more than one file, the first will be returned.
/// Performance note: has to unzip and return the entire file.
pub fn open_csvz<T: 'static>(mut infile: T) -> Box<dyn Read>
where
    T: Read + Seek,
{
    if !is_zip(&mut infile) {
        Box::new(infile)
    } else {
        let mut zippah = zip::ZipArchive::new(infile).expect("error establishing the ZIP");
        let mut zippy = zippah.by_index(0).expect("no file in ZIP");
        // sigh. We're going to need to just go ahead and read the entire thing into memory here
        let zs = zippy.size() as usize;
        let mut bigbuf: Vec<u8> = Vec::with_capacity(zs);
        zippy.read_to_end(&mut bigbuf).expect("Error reading ZIP");
        Box::new(Cursor::new(bigbuf))
    }
}

/// opens blah.csv OR blah.zip
pub fn open_csvz_from_path(inpath: &path::Path) -> Box<dyn Read> {
    use std::ffi::OsStr;
    if inpath.exists() && inpath.is_file() {
        open_csvz(File::open(inpath).unwrap())
    } else {
        let ext = inpath.extension().unwrap_or_else(|| {
            panic!(
                "Could not find {:#?} whether compressed or not",
                inpath.display()
            )
        });
        if ext == OsStr::new("csv") {
            let newpath = inpath.with_extension("zip");
            open_csvz(File::open(newpath).unwrap())
        } else if ext == OsStr::new("csv") {
            let newpath = inpath.with_extension("csv");
            open_csvz(File::open(newpath).unwrap())
        } else {
            panic!(
                "Could not find {:#?} whether compressed or not",
                inpath.display()
            );
        }
    }
}

/// Peeks at the contents to check the magic number
/// slightly adapted from zip-extensions
/// to operate on a `Read+Seek` rather than a full `File`
pub fn is_zip<T>(infile: &mut T) -> bool
where
    T: Read + Seek,
{
    const ZIP_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];
    let pos = infile.seek(SeekFrom::Current(0)).unwrap();
    let mut buffer: [u8; 4] = [0; 4];
    let bytes_read = infile.read(&mut buffer).unwrap();
    infile
        .seek(Start(pos))
        .expect("couldn't seek back to the start after testing whether a file was a ZIP"); // revert
    if bytes_read == buffer.len() && bytes_read == ZIP_SIGNATURE.len() {
        for i in 0..ZIP_SIGNATURE.len() {
            if buffer[i] != ZIP_SIGNATURE[i] {
                return false;
            }
        }
        return true;
    }
    false
}

/// Get a Writer to a file in a ZIP or die trying!
/// Will create a ZIP file with a single inner file, named the same as the ZIP bar the extension.
pub fn get_zip_writer_to_path(outpath: &path::Path, inner_ext: &str) -> zip::ZipWriter<File> {
    let mut outfile = zip::ZipWriter::new(
        File::create(&outpath.with_extension("zip")).expect("Couldn't create new output file"),
    );
    outfile
        .start_file(
            outpath
                .with_extension(inner_ext)
                .file_name()
                .unwrap()
                .to_str()
                .unwrap(),
            zip::write::FileOptions::default(),
        )
        .unwrap();
    outfile
}

// Credit to /u/Ophekkis
// https://www.reddit.com/r/rust/comments/fyjmbv/n00b_question_how_to_get_user_input/fn0d5va/
pub fn input(prompt: &str) -> std::io::Result<String> {
    let mut stdout = stdout();
    write!(&mut stdout, "{}", prompt)?;
    stdout.flush()?;
    let mut response = String::new();
    stdin().read_line(&mut response)?;
    Ok(response.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_conversions() {
        assert_eq!("AE", 31.to_ticket());
        assert_eq!(31, "AE".to_number());
        assert_eq!("123 thousand", 123000.pretty_number());
    }
}
