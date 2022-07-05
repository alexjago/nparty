//! The booths-to-SA1s projection phase.
use super::booths::{group_combos, Parties};
use super::utils::StateAb;
use color_eyre::eyre::{bail, Context, ContextCompat, Result};
use std::collections::BTreeMap;
use std::fs::create_dir_all;
/// This file corresponds to `SA1s_Multiplier.py`

/// The AEC have given us a
/// "this many people from this SA1 voted at this booth"
/// spreadsheet. This is almost tailor made for projecting Senate results
/// onto state electoral boundaries.

/// We are basically performing a matrix product.
/// [sa1s; booths] * [booths; orders] = [sa1s; orders]
/// except that [sa1s; booths] is so sparse as to be represented a little differently.
use std::path::Path;
use tracing::info;

/// Convert a header to a column index
#[allow(non_camel_case_types)]
enum sfl {
    year = 0,
    state_ab = 1,
    div_nm = 2,
    SA1_id = 3,
    #[allow(dead_code)]
    pp_id = 4,
    pp_nm = 5,
    votes = 6,
}

type BoothRecords = BTreeMap<String, (Vec<String>, Vec<f64>)>;

/// *** Load up NPP Booth Data ***
/// this is the [booths; orders] matrix equivalent
/// Also, we calculate the output length here for reasons
fn load_npp_booths(
    combinations: &[String],
    npp_booths_path: &Path,
) -> Result<(BoothRecords, usize)> {
    let mut boothsfields = vec![
        String::from("ID"),
        String::from("Division"),
        String::from("Booth"),
        String::from("Latitude"),
        String::from("Longitude"),
    ];
    boothsfields.extend_from_slice(combinations);
    boothsfields.push(String::from("Total"));

    let mut booths: BoothRecords = BTreeMap::new();

    let mut booths_rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .has_headers(true)
        .from_path(npp_booths_path)?;

    // Maybe we can deserialize to boothsfields?
    // That's what we want to do...
    // well, we can mostly do that.

    for record in booths_rdr.records() {
        let row = record?;
        let divbooth = row[1].to_owned() + "_" + &row[2];
        let mut boothmeta: Vec<String> = Vec::with_capacity(5);

        for i in 0..5 {
            boothmeta.push(row[i].to_string());
        }

        let mut boothvotes: Vec<f64> = Vec::with_capacity(combinations.len());

        for i in 5..row.len() {
            let val = row[i].parse::<f64>().unwrap_or(0.0);
            boothvotes.push(val);
        }
        if row.len() < boothsfields.len() {
            boothvotes.resize(boothsfields.len(), 0.0);
        }

        booths.insert(divbooth, (boothmeta, boothvotes));
    }
    Ok((booths, boothsfields.len()))
}

/// Actually write the output
fn write_sa1_prefs(
    sa1_prefs_path: &Path,
    combinations: &[String],
    outputn: BTreeMap<String, Vec<f64>>,
    outlen: usize,
) -> Result<()> {
    // having summed it all up...

    create_dir_all(
        sa1_prefs_path
            .parent()
            .context("couldn't perform path conversion")?,
    )?;
    let mut sa1_wtr = csv::Writer::from_path(sa1_prefs_path)?;

    let mut header = vec![String::from("SA1_id")];
    header.extend_from_slice(&combinations);
    header.push(String::from("Total"));
    sa1_wtr
        .write_record(header)
        .context("error writing SA1_prefs header")?;

    for (id, row) in &outputn {
        let mut out: Vec<String> = Vec::with_capacity(outlen);
        out.push(id.clone());
        for i in row {
            out.push(i.to_string());
        }
        sa1_wtr
            .write_record(out)
            .context("error writing SA1_prefs line")?;
    }

    sa1_wtr.flush().context("error finalising SA1_prefs")?;
    Ok(())
}

pub fn project(
    parties: &Parties,
    state: StateAb,
    year: &str,
    npp_booths_path: &Path,
    sa1_breakdown_path: &Path,
    sa1_prefs_path: &Path,
) -> Result<()> {
    info!("\tProjecting results onto SA1s");

    let combinations = {
        // potential soundness issue: is this going to work out the same way?
        // BTreeMap for Parties in general should fix that
        let mut partykeys: Vec<&str> = Vec::new();
        for k in parties.keys() {
            partykeys.push(k);
        }
        group_combos(&partykeys)
    };

    // *** Load up NPP-Booth data ***
    let (booths, outlen) = load_npp_booths(&combinations, npp_booths_path)?;

    // *** Load up SA1 data ***
    // This is the [sa1s; booths] matrix equivalent
    // Since it's so sparse we prefer a map to an array

    let mut sa1_rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .has_headers(true)
        .from_path(sa1_breakdown_path)?;

    let mut outputn: BTreeMap<String, Vec<f64>> = BTreeMap::new(); // Our numerical ultimate output. Indexed by ID

    let mut row = csv::StringRecord::new();
    while sa1_rdr.read_record(&mut row)? {
        let id = row
            .get(sfl::SA1_id as usize)
            .context("Missing SA1_id field in record")?
            .to_owned();

        if row
            .get(sfl::state_ab as usize)
            .context("Missing state_ab field in record")?
            != state.to_string()
        {
            // All SA1s nationwide are in the one file - so any row with the wrong state can be safely skipped.
            continue;
        }
        if row
            .get(sfl::year as usize)
            .context("Missing year field in record")?
            != year
        {
            // However, the wrong year is definitely cause for concern. Bail.
            bail!(
                "Problem in `{}`: Unsupported election year: {}. Exiting.",
                sa1_breakdown_path.display(),
                year
            );
        }

        let sa1_booth_votes: f64 = row
            .get(sfl::votes as usize)
            .and_then(|x| x.parse::<f64>().ok())
            .unwrap_or(0.0_f64);

        let divbooth = row[sfl::div_nm as usize].to_owned() + "_" + &row[sfl::pp_nm as usize];

        if let Some((_, boothvotes)) = &booths.get(&divbooth) {
            // Rarely, there's no entry if no formal votes at a booth
            // ... or if the prior checks aren't sufficient
            let boothtotal = boothvotes
                .last()
                .with_context(|| format!("No vote records for {}", &divbooth))?;

            let mut output_row = outputn
                .get(&id)
                .cloned()
                .unwrap_or_else(|| vec![0.0_f64; combinations.len() + 1]);

            if *boothtotal != 0.0_f64 {
                for (i, w) in boothvotes.iter().enumerate() {
                    *output_row.get_mut(i).unwrap() += w * sa1_booth_votes / boothtotal;
                    // doing it in one go produces slightly different results to the Python,
                    // which is concerning...
                }
            }
            outputn.insert(id, output_row);
        }
    }

    // Actually write the output
    write_sa1_prefs(sa1_prefs_path, &combinations, outputn, outlen)?;

    info!("\t\tDone!");
    Ok(())
}
