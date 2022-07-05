//! The SA1s-to-districts **combination** phase.
//!   
//! (1) Take SA1-by-SA1 NPP data  
//! (2) Take SA1 population & district split data  
//! (3) Scale (1) to fit (2) [if 3rd & 4th columns exist in (2)]  
//! (4) Also split (3) according to (2) where necessary/available  
//! (5) Aggregate (4) by district.  
//! (6) Write to file(s)  
use color_eyre::eyre::{Context, ContextCompat, Result};
use csv::{StringRecord, StringRecordsIntoIter};
use indexmap::IndexMap;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{create_dir_all, File};
use std::io::{self, Write};
use std::path::Path;
use tracing::info;

type Sa1Prefs = BTreeMap<String, Vec<f64>>;

/// Load up SA1 NPP data (step 1)
/// Returns both the data keyed by the first column (SA1 ID), and the file headers
fn load_sa1_prefs(sa1_prefs_path: &Path) -> Result<(Sa1Prefs, StringRecord)> {
    let mut sa1_prefs: BTreeMap<String, Vec<f64>> = BTreeMap::new();

    let mut sa1_prefs_rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .has_headers(true)
        .from_path(sa1_prefs_path)
        .with_context(|| {
            format!(
                "Could not find SA1s to preferences file, does this path exist?\n\t{}",
                sa1_prefs_path.display()
            )
        })?;

    for record in sa1_prefs_rdr.records() {
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

    let sa1_headers = sa1_prefs_rdr.headers()?.clone();

    Ok((sa1_prefs, sa1_headers))
}

type Sa1DistsRdr = StringRecordsIntoIter<File>;

/// 2a. Load up SA1 to district data as an iterator over a file
fn get_sa1_districts(sa1_districts_path: &Path) -> Result<Sa1DistsRdr> {
    let rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .has_headers(true)
        .from_path(sa1_districts_path)
        .with_context(|| {
            format!(
                "Could not find SA1s to districts correspondence file, does this path exist?\n\t{}",
                sa1_districts_path.display()
            )
        })?
        .into_records();
    Ok(rdr)
}

/// 6a. Output CSV to `npp_dists_path`
fn write_aggregate_csv(
    npp_dists_path: &Path,
    districts: &Sa1Prefs,
    header: &[String],
) -> Result<()> {
    create_dir_all(
        npp_dists_path
            .parent()
            .with_context(|| format!("{} has no parent", npp_dists_path.display()))?,
    )?;

    let mut dist_wtr = csv::Writer::from_path(npp_dists_path)?;

    dist_wtr
        .write_record(header)
        .context("error writing npp_dists header")?;

    for (id, row) in districts {
        let mut out: Vec<String> = Vec::with_capacity(header.len());
        out.push(id.clone());
        for i in row {
            out.push(i.to_string());
        }
        // trace!("{:?}", out);
        dist_wtr
            .write_record(out)
            .context("error writing npp_dists line")?;
    }

    dist_wtr.flush().context("error finalising npp_dists")?;

    Ok(())
}

/// 6b. Output to `npp_dists_path` (but as .json rather than .csv)
fn write_aggregate_js(
    npp_dists_path: &Path,
    districts: &Sa1Prefs,
    parties: &IndexMap<String, Vec<String>>,
    header: &[String],
) -> Result<()> {
    create_dir_all(
        npp_dists_path
            .parent()
            .with_context(|| format!("{} has no parent", npp_dists_path.display()))?,
    )?;

    // 6.b JS
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
    let json_file = File::create(json_path).context("Error creating SA1s aggregate JSON file")?;
    serde_json::to_writer(json_file, &out).context("Error writing SA1s aggregate JSON file")?;

    Ok(())
}

/// Perform the actual summation (steps 2b through 5)
fn make_districts(
    sa1_prefs: &Sa1Prefs,
    sa1_dists_rdr: Sa1DistsRdr,
) -> Result<BTreeMap<String, Vec<f64>>> {
    // 2b. Load up SA1 to district data

    let mut districts: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut seen_sa1s: BTreeSet<String> = BTreeSet::new();

    for record in sa1_dists_rdr {
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
                multiplier = 0.0_f64;
            } else {
                multiplier = sa1_pop / sa1_total;
            }
        } else {
            // What happens if there are SA1 splits but we don't have info?
            // Hack: just allocate to whichever was seen first for now
            let sa1 = id.to_string();
            if seen_sa1s.contains(&sa1) {
                continue;
            }
            seen_sa1s.insert(sa1);
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
    // trace!("{:#?}", districts);
    Ok(districts)
}

pub fn aggregate(
    sa1_prefs_path: &Path,
    sa1_districts_path: &Path,
    npp_dists_path: &Path,
    write_js: bool,
    parties: &IndexMap<String, Vec<String>>,
) -> Result<()> {
    //! 1. Take SA1-by-SA1 NPP data from `sa1_prefs_path`
    //! 2. Take SA1 population & district split data from `sa1_districts_path`
    //! 3. Scale (1) to fit (2) [if 3rd & 4th columns exist in (2)]
    //! 4. Also split (3) according to (2) where necessary/available
    //! 5. Aggregates (4) by district.
    //! 6. Output to `npp_dists_path`

    // TODO convert all of the above to streams for WASM compatibility
    // [x] factored out IO code
    // [x] factored out calculation code
    // [ ] handle WASM IO

    info!("\tCombining SA1s into Districts");

    let (sa1_prefs, sp_headers) = load_sa1_prefs(sa1_prefs_path)?;

    let sa1_dists_rdr = get_sa1_districts(sa1_districts_path)?;

    let districts = make_districts(&sa1_prefs, sa1_dists_rdr)?;

    // 6. Output to `npp_dists_path`

    let mut header = vec![String::from("District")];
    for i in sp_headers.iter().skip(1) {
        header.push(i.to_string());
    }

    write_aggregate_csv(npp_dists_path, &districts, &header)?;

    if write_js {
        write_aggregate_js(npp_dists_path, &districts, parties, &header)?;
    }

    info!("\t\tDone!");
    io::stderr().flush()?;

    Ok(())
}
