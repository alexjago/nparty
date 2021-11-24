use crate::utils::*;
use csv;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
/// This file exists to contain format conversions
use std::result::Result;

// The candidate file format is sufficiently unchanged
// that it doesn't appear to need upgrading.
// We can simply read it with utils::read_candidates()
// However, what that doesn't give us is a mapping from Divisions to States
// We kinda need this for the main event, as anything else would be fragile.
// More problematically, csv::from_reader(rdr) takes ownership of rdr
// so we can only do that once, and I don't want to modify utils::read_candidates()
// or duplicate its code
// So instead we'll read another file with similar information

type DivisionName = String;

pub fn divstate_creator<T>(cands_file: T) -> HashMap<DivisionName, StateAb>
where
    T: Read,
{
    let mut out = HashMap::new();
    #[derive(Debug, Deserialize)]
    #[allow(non_snake_case)]
    struct DivStates {
        state_ab: StateAb,
        div_nm: DivisionName,
    }

    let mut rdr = csv::Reader::from_reader(cands_file);
    for row in rdr.deserialize() {
        let record: DivStates = row.unwrap();
        out.insert(record.div_nm, record.state_ab);
    }
    return out;
}

/// Upgrade a preferences file from 2016 to 2019 format
pub fn upgrade_prefs_16_19(
    infile: &mut dyn Read,
    outfile: &mut dyn Write,
    candsdata: &CandsData,
    divstates: &HashMap<DivisionName, StateAb>,
) {
    #[derive(Debug, Deserialize)]
    #[allow(non_snake_case)]
    struct OldRow {
        ElectorateNm: String,
        VoteCollectionPointNm: String,
        VoteCollectionPointId: String,
        BatchNo: String,
        PaperNo: String,
        Preferences: String,
    }

    let mut inrdr = csv::Reader::from_reader(infile);
    let mut outwtr = csv::WriterBuilder::new()
        .terminator(csv::Terminator::CRLF)
        .from_writer(outfile);

    let mut progress: usize = 0;

    let mut state: StateAb;
    let mut statestring = String::new();

    // big optimisation! https://blog.burntsushi.net/csv/#amortizing-allocations

    for row in inrdr.deserialize() {
        let old: OldRow = row.unwrap();

        if old.ElectorateNm.starts_with("---") {
            // skip random divider line
            continue;
        } else if progress == 0 {
            // on startup we need to write the headers
            let mut header = vec![
                String::from("State"),
                String::from("Division"),
                String::from("Vote Collection Point Name"),
                String::from("Vote Collection Point ID"),
                String::from("Batch No"),
                String::from("Paper No"),
            ];
            // let mut header = vec!["State", "Division", "Vote Collection Point Name", "Vote Collection Point ID", "Batch No", "Paper No"];

            state = divstates[&old.ElectorateNm].clone();
            statestring = state.to_string();

            let mut atls: Vec<String> = Vec::new();
            let mut btls: Vec<String> = Vec::new();

            // and figure out who our candidates are
            // we have a CandsData, and thence a
            let ballot_paper = &candsdata[&state];
            for tnum in 1..ballot_paper.len() {
                let tnum = tnum as u32;
                let tstring = tnum.to_ticket();
                let ticket = &ballot_paper[&tstring];
                // eprintln!("{:#?}", ticket);
                atls.push(format!("{}:{}", tstring, ticket[&0_u32].party));
                for cnum in 1..ticket.len() {
                    let cnum = cnum as u32;
                    btls.push(format!(
                        "{}:{} {}",
                        tstring, ticket[&cnum].surname, ticket[&cnum].ballot_given_nm
                    ));
                }
            }

            {
                // handle UGs
                let ticket = &ballot_paper["UG"];
                for cnum in 1..=ticket.len() {
                    let cnum = cnum as u32;
                    btls.push(format!(
                        "UG:{} {}",
                        ticket[&cnum].surname, ticket[&cnum].ballot_given_nm
                    ));
                }
            }

            header.append(&mut atls);
            header.append(&mut btls);

            // eprintln!("{:#?} {}", &header, header.len());

            outwtr.write_record(header).unwrap();
        }

        let mut prefs: Vec<&str> = old.Preferences.split(",").collect();

        let mut new = vec![
            statestring.as_str(),
            &old.ElectorateNm,
            &old.VoteCollectionPointNm,
            &old.VoteCollectionPointId,
            &old.BatchNo,
            &old.PaperNo,
        ];
        new.append(&mut prefs);

        // eprintln!("{:#?}, {}", new, new.len());

        outwtr.write_record(new).expect("Error writing output file");

        progress += 1;
    }
}
