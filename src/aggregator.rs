//! Conversion of `SA1s_Aggregator.py`
//!
//! Step-by-step:
//! 1. takes SA1-by-SA1 NPP data
//! 2. takes SA1 population & district split data
//! 3. scales (1) to fit [the totals of] (2)
//! 4. splits (3) according to (2) where necessary
//! 5. aggregates (4) by district

use anyhow::{Context, Result};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{create_dir_all, File};
use std::path::Path;

pub fn aggregate(
    sa1_prefs_path: &Path,
    sa1_districts_path: &Path,
    npp_dists_path: &Path,
    write_js: bool,
    parties: &BTreeMap<String, Vec<String>>,
) -> Result<()> {
    //! 1. Take SA1-by-SA1 NPP data from `sa1_prefs_path`
    //! 2. Take SA1 population & district split data from `sa1_districts_path`
    //! 3. Scale (1) to fit (2) [if 3rd & 4th columns exist in (2)]
    //! 4. Also split (3) according to (2) where necessary/available
    //! 5. Aggregates (4) by district.
    //! 6. Output to `npp_dists_path`

    println!("\tCombining SA1s into Districts");

    // 1. Load up SA1 NPP data
    let mut sa1_prefs: BTreeMap<String, Vec<f64>> = BTreeMap::new();

    let mut sp_rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .has_headers(true)
        .from_path(sa1_prefs_path)
        .with_context(|| {
            format!(
                "Could not find SA1s to preferences file, does this path exist?\n\t{}",
                sa1_prefs_path.display()
            )
        })?;

    for record in sp_rdr.records() {
        let row = record?;
        let id = row.get(0).context("empty row in SA1 prefs file")?;
        let mut numbers = Vec::with_capacity(row.len() - 1);
        for i in 1..row.len() {
            let x: f64 = row
                .get(i)
                .and_then(|x| x.parse::<f64>().ok())
                .unwrap_or(0.0_f64);
            numbers.push(x);
        }
        sa1_prefs.insert(id.to_string(), numbers);
    }

    // 2. Load up SA1 to district data

    let mut districts: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut seen_sa1s: BTreeSet<String> = BTreeSet::new();

    let mut sd_rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .has_headers(true)
        .from_path(sa1_districts_path)
        .with_context(|| {
            format!(
                "Could not find SA1s to districts correspondence file, does this path exist?\n\t{}",
                sa1_districts_path.display()
            )
        })?;

    for record in sd_rdr.records() {
        let row = record?;

        if row.len() < 2 {
            continue;
        }

        let id = row
            .get(0)
            .context("empty row in SA1s-to-districts file")?
            .trim();
        let dist = row
            .get(1)
            .context("empty row in SA1s-to-districts file")?
            .trim();

        // 3. Scale (1) to fit (2)
        // 4. is along for the ride?

        let sa1_npps = match sa1_prefs.get(id) {
            Some(x) => x,
            _ => continue,
        };
        let mut multiplier = 1.0_f64;

        if row.len() >= 3 {
            // Fun fact: we don't actually need `Pop_Share` for anything
            let sa1_total = sa1_npps
                .last()
                .context("missing 'total' field in SA1s-to-districts file")?;
            let sa1_pop = row
                .get(2)
                .and_then(|x| x.parse::<f64>().ok())
                .unwrap_or(0.0_f64);

            if sa1_pop == 0.0_f64 {
                multiplier = 0.0_f64
            } else {
                multiplier = sa1_pop / sa1_total;
            }
        } else {
            // What happens if there are SA1 splits but we don't have info?
            // Hack: just allocate to whichever was seen first for now
            let sa1 = id.to_string();
            if seen_sa1s.contains(&sa1) {
                continue;
            } else {
                seen_sa1s.insert(sa1);
            }
        }
        // 5. Aggregates (4) by district.

        if districts.contains_key(dist) {
            let dist_npps = districts.get_mut(dist).context("TOCTOU in aggregation")?;
            for j in 0..sa1_npps.len() {
                dist_npps[j] += sa1_npps[j] * multiplier;
            }
        } else {
            let mut dist_npps = Vec::with_capacity(sa1_npps.len());
            for s in sa1_npps {
                dist_npps.push(s * multiplier);
            }
            districts.insert(dist.to_string(), dist_npps);
        }
    }
    // println!("{:#?}", districts);

    // 6. Output to `npp_dists_path`

    create_dir_all(
        sa1_prefs_path
            .parent()
            .with_context(|| format!("{} has no parent", sa1_prefs_path.display()))?,
    )?;

    // 6.a CSV

    let mut dist_wtr = csv::Writer::from_path(npp_dists_path)?;

    let mut header = vec![String::from("District")];
    let sp_headers = sp_rdr.headers()?;
    for i in sp_headers.iter().skip(1) {
        header.push(i.to_string());
    }
    eprintln!("{:?}", header);

    dist_wtr
        .write_record(&header)
        .context("error writing npp_dists header")?;

    for (id, row) in districts.iter() {
        let mut out: Vec<String> = Vec::with_capacity(header.len());
        out.push(id.clone());
        for i in row {
            out.push(i.to_string());
        }
        // eprintln!("{:?}", out);
        dist_wtr
            .write_record(out)
            .context("error writing npp_dists line")?;
    }

    dist_wtr.flush().context("error finalising npp_dists")?;

    // 6.b JS
    if write_js {
        // format: {parties : {abbr: full name}, field_names: [], data: {district: [values]}}
        // note that data is our Districts variable
        // and field_names are just the header (well, skipping the district column)
        // and, well, parties are parties
        let out = json!({
            "parties": parties, // empty for now
            "field_names": header[1..],
            "data": districts
        });
        let json_path = npp_dists_path.with_extension("json");
        let json_file =
            File::create(json_path).context("Error creating SA1s aggregate JSON file")?;
        serde_json::to_writer(json_file, &out).context("Error writing SA1s aggregate JSON file")?;
    }

    Ok(())
}
