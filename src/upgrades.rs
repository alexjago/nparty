//! Ballot-file format upgrades and SA1 geography upgrades.
//!
//! Copyright 2017-2023 Alex Jago <alex@abjago.net>
//! Released under the MIT or Apache 2.0 licenses, at your option.

use color_eyre::eyre::{bail, Context, ContextCompat, Result};

use crate::app::{CliUpgradeBooths, CliUpgradeSa1s};
use crate::utils::{
    get_zip_writer_to_path, open_csvz_from_path, read_candidates, CandsData, StateAb, ToTicket,
};
use std::collections::{BTreeMap, HashMap};
use std::fs::{metadata, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::SystemTime;

// The candidate file format is sufficiently unchanged
// that it doesn't appear to need upgrading.
// We can simply read it with read_candidates()
// However, what that doesn't give us is a mapping from Divisions to States
// We kinda need this for the main event, as anything else would be fragile.
// More problematically, csv::from_reader(rdr) takes ownership of rdr
// so we can only do that once, and I don't want to modify read_candidates()
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
    out
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

    // potential big optimisation? https://blog.burntsushi.net/csv/#amortizing-allocations

    for row in inrdr.deserialize() {
        let old: OldRow = row.unwrap();

        if old.ElectorateNm.starts_with("---") {
            // skip random divider line
            continue;
        }
        if progress == 0 {
            // on startup we need to write the headers
            let mut header = vec![
                String::from("State"),
                String::from("Division"),
                String::from("Vote Collection Point Name"),
                String::from("Vote Collection Point ID"),
                String::from("Batch No"),
                String::from("Paper No"),
            ];

            state = divstates[&old.ElectorateNm];
            statestring = state.to_string();

            let mut aboves: Vec<String> = Vec::new();
            let mut belows: Vec<String> = Vec::new();

            // and figure out who our candidates are
            // we have a CandsData, and thence a ...
            let ballot_paper = &candsdata[&state];
            for tnum in 1..ballot_paper.len() {
                let tnum = tnum as u32;
                let tstring = tnum.to_ticket();
                let ticket = &ballot_paper[&tstring];
                aboves.push(format!("{}:{}", tstring, ticket[&0_u32].party));
                for cand_num in 1..ticket.len() {
                    let cand_num = cand_num as u32;
                    belows.push(format!(
                        "{}:{} {}",
                        tstring, ticket[&cand_num].surname, ticket[&cand_num].ballot_given_nm
                    ));
                }
            }

            {
                // handle UGs
                let ticket = &ballot_paper["UG"];
                for cand_num in 1..=ticket.len() {
                    let cand_num = cand_num as u32;
                    belows.push(format!(
                        "UG:{} {}",
                        ticket[&cand_num].surname, ticket[&cand_num].ballot_given_nm
                    ));
                }
            }

            header.append(&mut aboves);
            header.append(&mut belows);

            outwtr.write_record(header).unwrap();
        }

        let mut prefs: Vec<&str> = old.Preferences.split(',').collect();

        let mut new = vec![
            statestring.as_str(),
            &old.ElectorateNm,
            &old.VoteCollectionPointNm,
            &old.VoteCollectionPointId,
            &old.BatchNo,
            &old.PaperNo,
        ];
        new.append(&mut prefs);

        outwtr.write_record(new).expect("Error writing output file");

        progress += 1;

        if progress % 100_000 == 0 {
            eprintln!("{}Upgrade progress... {}", crate::term::TTYJUMP, progress);
        }
    }
}

/// Sniff the era of a CSV stream
/// It's a stream, so be sure it's the start
pub fn era_sniff(infile: &mut dyn Read) -> color_eyre::eyre::Result<usize> {
    let mut inrdr = csv::Reader::from_reader(infile);
    let hdr: Vec<&str> = inrdr.headers()?.into_iter().collect();

    let rez = match hdr.get(0..6).context("Invalid headers.")? {
        ["ElectorateNm", "VoteCollectionPointNm", "VoteCollectionPointId", "BatchNo", "PaperNo", "Preferences"] => {
            2016
        }
        ["State", "Division", "Vote Collection Point Name", "Vote Collection Point ID", "Batch No", "Paper No"] => {
            2019
        }
        _ => bail!("Invalid headers."),
    };
    Ok(rez)
}

/// Performs the `upgrade sa1s` subcommand.
pub fn do_upgrade_sa1s(args: CliUpgradeSa1s) -> color_eyre::eyre::Result<()> {
    // 1. Read the correspondence file into a map

    #[derive(Debug)]
    struct CorrespondenceRow {
        old: String,
        new: String,
        ratio: f64,
    }
    // "RATIO of SA1_7DIGITCODE_old is in SA1_7DIGITCODE_new"
    let mut corrs: BTreeMap<String, Vec<(String, f64)>> = BTreeMap::new();
    let mut cf = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_path(&args.correspondence_file)?;
    for record in cf.records() {
        let r = record?;
        // positional deserialise
        let row = CorrespondenceRow {
            old: r[0].to_string(),
            new: r[1].to_string(),
            ratio: r[2].parse::<f64>().ok().unwrap_or(0.0_f64),
        };
        corrs.entry(row.old).or_default().push((row.new, row.ratio));
    }

    // 2. Read and convert the input file

    #[derive(Debug, Serialize)]
    #[allow(non_snake_case)]
    struct Sa1sDist {
        SA1_Id: String,
        Dist_Name: String,
        Pop: f64,
        Pop_Share: f64,
    }

    // {NEW_SA1 : {DIST : Pop}}
    let mut converted: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();

    let mut oldf = csv::ReaderBuilder::new()
        .has_headers(!args.no_infile_headers)
        .from_path(&args.input)?;

    // Previously, we deserialised by position, not by header name
    //

    for record in oldf.records() {
        let r = record?;
        // positional deserialisation because we may only have 2 columns
        let row = Sa1sDist {
            SA1_Id: r[0].to_string(),
            Dist_Name: r[1].to_string(),
            Pop: r.get(2).and_then(|x| x.parse::<f64>().ok()).unwrap_or(0.0),
            Pop_Share: 0.0,
        };
        // "RATIO of SA1_7DIGITCODE_old is in SA1_7DIGITCODE_new"
        let old_sa1 = row.SA1_Id.clone();
        if let Some(split) = corrs.get(&old_sa1) {
            for (new_sa1, ratio) in split {
                let e = converted
                    .entry(new_sa1.clone())
                    .or_default()
                    .entry(row.Dist_Name.clone())
                    .or_default();
                *e += row.Pop * ratio;
                // we'll have to fill in PopShare later
            }
        }
    }

    // 3. Finalise and write results
    let mut outf = csv::WriterBuilder::new()
        .has_headers(true)
        .from_path(args.output)?;
    for (new, dists) in converted {
        let mut poptotal: f64 = dists.values().sum();
        if poptotal == 0.0 {
            poptotal = 1.0;
        }

        for (d, p) in dists {
            outf.serialize(Sa1sDist {
                SA1_Id: new.clone(),
                Dist_Name: d,
                Pop: p,
                Pop_Share: p / poptotal,
            })?;
        }
        outf.flush()?;
    }
    Ok(())
}

/// Performs the `upgrade prefs` subcommand.
pub fn do_upgrade_prefs(args: crate::app::CliUpgradePrefs) -> color_eyre::eyre::Result<()> {
    let candspath = args.candidates;
    let inpath = args.input;
    let outpath = args.output;
    let suffix = args.suffix;
    let filter = args.filter;

    let mut paths: Vec<(PathBuf, PathBuf)> = Vec::new();

    if inpath.is_dir() {
        if outpath.is_dir() {
            let mut query: String = inpath
                .to_str()
                .map(String::from)
                .context("Path conversion error")?;
            query.push_str(&filter);

            let ips: Vec<PathBuf> = glob::glob(&query)?.filter_map(Result::ok).collect();

            if inpath == outpath {
                // need to upgrade in place
                // i.e. apply suffix to opaths
                for ip in ips {
                    let mut op_fstem = ip.file_stem().context("No file name")?.to_os_string();
                    op_fstem.push(&suffix);
                    let ext = ip.extension().context("No file extension")?;
                    let op = ip.clone().with_file_name(op_fstem).with_extension(ext);
                    paths.push((ip, op));
                }
            } else {
                // don't need to upgrade in place
                for ip in ips {
                    let op = outpath.join(ip.file_name().context("No file name")?);
                    paths.push((ip, op));
                }
            }
        } else {
            bail!("Input path is a directory but output path is not.");
        }
    } else {
        let ip = inpath.clone();
        if outpath.is_dir() {
            paths.push((
                ip,
                outpath.join(inpath.file_name().context("no file name")?),
            ));
        } else {
            paths.push((ip, outpath));
        }
    }

    for (ipath, opath) in &paths {
        let candsdata =
            read_candidates(File::open(&candspath).context("Couldn't open candidates file")?)?;
        let divstates =
            divstate_creator(File::open(&candspath).context("Couldn't open candidates file")?);

        // eprintln!("ipath: {} \t opath: {}", ipath.display(), opath.display());

        let era = era_sniff(&mut open_csvz_from_path(ipath)?)
            .context("Error determining era of input.")?;

        if era == 2016 {
            // Test if upgrade already exists
            let im = metadata(ipath).context("In-path doesn't seem to exist?")?;
            let om = metadata(opath);
            let out_time = om.as_ref().map_or(SystemTime::UNIX_EPOCH, |x| {
                x.modified().unwrap_or(SystemTime::UNIX_EPOCH)
            });
            let in_time = im.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            if out_time > in_time {
                // todo: consider testing it's the correct era
                eprintln!("Upgrade already exists; skipping");
                continue;
            }
            eprintln!("Upgrading...");
            upgrade_prefs_16_19(
                &mut open_csvz_from_path(ipath)?,
                &mut get_zip_writer_to_path(opath, "csv")?,
                &candsdata,
                &divstates,
            );
        } else {
            eprintln!("No upgrade available - is it already the latest?");
        }
    }
    Ok(())
}

/// Performs the `upgrade booths` subcommand.
/// TODO: is this pretty basic SQL? Anything else? Well, upgrading SA1s is too but prefs is more complex
///     select year, `state_ab`, `div_nm`, new as `ccd_id`, `pp_id`, `pp_nm`, sum(votes * ratio)
///     from Booths, Corrs
///     join on Corrs.old = `Booths.ccd_id`
///     group by year, `state_ab`, `div_nm`, new, `pp_id`, `pp_nm`;
/// Well, except that we consider Corrs' rows by position, rather than by name
/// Also one file might be in 7 digit codes and the other in 11 digit codes
pub fn do_upgrade_booths(args: CliUpgradeBooths) -> color_eyre::eyre::Result<()> {
    #![allow(clippy::too_many_lines)]
    // 1. Read the correspondence file into a map

    struct CorrespondenceRow {
        old: String,
        new: String,
        ratio: f64,
    }
    // "RATIO of SA1_7DIGITCODE_old is in SA1_7DIGITCODE_new"

    // We expect an SA1 code to be either 7 digits or 11 digits (ASGS 1 or 2)
    // or just 11 digits (ASGS 3) with the AEC typically using the 7 digit codes
    // since strings of different lengths don't usually compare equal, we will
    // construct three parallel lookup tables depending on length
    let mut corrs_7: BTreeMap<String, Vec<(String, f64)>> = BTreeMap::new();
    let mut corrs_11: BTreeMap<String, Vec<(String, f64)>> = BTreeMap::new();
    let mut corrs_all: BTreeMap<String, Vec<(String, f64)>> = BTreeMap::new();

    let mut cf = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_path(&args.correspondence_file)?;
    for record in cf.records() {
        let r = record?;
        // positional deserialise
        let row = CorrespondenceRow {
            old: r[0].to_string(),
            new: r[1].to_string(),
            ratio: r[2].parse::<f64>().ok().unwrap_or(0.0_f64),
        };

        // the 7 digit code is [ST 1] [SA2 4] [SA1 2]
        // the 11 digit code is [ST 1] [SA4 2] [SA3 2] [SA2 4] [SA1 2]
        // So if we have an 11 digit code we can create a 7 digit code too
        match row.old.len() {
            7 => corrs_7
                .entry(row.old.clone())
                .or_default()
                .push((row.new.clone(), row.ratio)),
            11 => {
                corrs_11
                    .entry(row.old.clone())
                    .or_default()
                    .push((row.new.clone(), row.ratio));
                // also push a truncated code
                let trunc = format!("{}{}", &row.old[..1], &row.old[5..]);
                corrs_7
                    .entry(trunc)
                    .or_default()
                    .push((row.new.clone(), row.ratio));
            }
            _ => {}
        };
        corrs_all
            .entry(row.old)
            .or_default()
            .push((row.new, row.ratio));
    }

    // eprintln!("{} entries in   7-digit correspondence file", corrs_7.len());
    // eprintln!(
    //     "{} entries in  11-digit correspondence file",
    //     corrs_11.len()
    // );
    // eprintln!(
    //     "{} entries in any-digit correspondence file",
    //     corrs_all.len()
    // );

    // 2. Read and convert the input file

    #[derive(Debug, Serialize, Deserialize, Clone)]
    struct Sa1sBooth {
        year: String,
        state_ab: String,
        div_nm: String,
        #[serde(alias = "SA1_id")]
        ccd_id: String,
        pp_id: String,
        pp_nm: String,
        votes: f64,
    }

    impl Ord for Sa1sBooth {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            (
                &self.year,
                &self.state_ab,
                &self.div_nm,
                &self.ccd_id,
                &self.pp_id,
                &self.pp_nm,
            )
                .cmp(&(
                    &other.year,
                    &other.state_ab,
                    &other.div_nm,
                    &other.ccd_id,
                    &other.pp_id,
                    &other.pp_nm,
                ))
        }
    }

    impl PartialOrd for Sa1sBooth {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }

    impl PartialEq for Sa1sBooth {
        fn eq(&self, other: &Self) -> bool {
            (
                &self.year,
                &self.state_ab,
                &self.div_nm,
                &self.ccd_id,
                &self.pp_id,
                &self.pp_nm,
            )
                .eq(&(
                    &other.year,
                    &other.state_ab,
                    &other.div_nm,
                    &other.ccd_id,
                    &other.pp_id,
                    &other.pp_nm,
                ))
        }
    }

    impl Eq for Sa1sBooth {}

    // {NEW_SA1 : {DivBooth: Votes}}
    let mut converted: BTreeMap<String, BTreeMap<Sa1sBooth, f64>> = BTreeMap::new();

    let mut oldf = csv::ReaderBuilder::new()
        .has_headers(!args.no_infile_headers)
        .from_path(&args.input)?;

    // Previously, we deserialised by position, not by header name

    // let mut recordcount = 0;
    // let mut desercount = 0;
    let mut unmatchcount = 0;
    let mut unmatchvote = 0_f64;

    for record in oldf.records() {
        // recordcount += 1;
        let r = record?;
        let row: Sa1sBooth = r.deserialize(None)?;

        // desercount += 1;

        // "RATIO of SA1_7DIGITCODE_old is in SA1_7DIGITCODE_new"
        // we need to deal with the possibility of the correspondence key being the other length
        // the 7 digit code is [ST 1] [SA2 4] [SA1 2]
        // the 11 digit code is [ST 1] [SA4 2] [SA3 2] [SA2 4] [SA1 2]
        let old_sa1 = row.ccd_id.clone();
        let (old_sa1_7, old_sa1_11) = match old_sa1.len() {
            7 => (Some(old_sa1.clone()), None),
            11 => (
                Some(format!("{}{}", &old_sa1[..1], &old_sa1[5..])),
                Some(old_sa1.clone()),
            ),
            _ => (None, None),
        };

        if let Some(split) = old_sa1_11
            .and_then(|k| corrs_11.get(&k))
            .or_else(|| old_sa1_7.and_then(|k| corrs_7.get(&k)))
            .or_else(|| corrs_all.get(&old_sa1))
        {
            for (new_sa1, ratio) in split {
                let mut updated = row.clone();
                updated.ccd_id = new_sa1.clone();
                updated.votes = row.votes * ratio;
                let e = converted
                    .entry(new_sa1.clone())
                    .or_default()
                    .entry(updated)
                    .or_default();
                *e += row.votes * ratio;
                // we'll have to fill in PopShare later
            }
        } else {
            // eprintln!("Could not find a match for:\n{row:?}");
            unmatchcount += 1;
            unmatchvote += row.votes;
        }
    }

    // eprintln!("{recordcount} old entries attempted conversion");
    // eprintln!("{desercount} old entries deserialized successfully");
    eprintln!("{unmatchcount} old entries could not be matched, totalling {unmatchvote} votes. These are probably voters with no fixed address or similar, especially if the ratio of entries to voters is close to 1.");
    // eprintln!("{} entries in converted tree", converted.len());

    // 3. Finalise and write results
    let mut outf = csv::WriterBuilder::new()
        .has_headers(true)
        .from_path(args.output)?;
    for (_, dists) in converted {
        // let mut poptotal: f64 = dists.values().sum();
        // if poptotal == 0.0 {
        // poptotal = 1.0;
        // }

        for (d, p) in dists {
            let mut outbooth = d.clone();
            outbooth.votes = p;
            outf.serialize(outbooth)?;
        }
        outf.flush()?;
    }
    Ok(())
}
