/// Projection phase
/// This file corresponds to `SA1s_Multiplier.py`

/// The AEC have given us a
/// "this many people from this SA1 voted at this booth"
/// spreadsheet. This is almost tailor made for projecting Senate results
/// onto state electoral boundaries.

/// We are basically performing a matrix product.
/// [sa1s; booths] * [booths; orders] = [sa1s; orders]
/// except that [sa1s; booths] is so sparse as to be represented a little differently.


use std::path::Path;
use std::process;
use std::collections::BTreeMap;
use super::booths::{group_combos, Parties};
use super::utils::StateAb;
use std::fs::create_dir_all;

const SA1_FIELDS : [&str; 7] = ["year","state_ab", "div_nm", "SA1_id", "pp_id", "pp_nm", "votes"];

fn sfl(input: &str) -> usize {
    match input {
        "year" => 0,
        "state_ab" => 1,
        "div_nm" => 2,
        "SA1_id" => 3,
        "pp_id" => 4,
        "pp_nm" => 5,
        "votes" => 6,
        _ => unreachable!(),
    }
}


type BoothRecords = BTreeMap<String, (Vec<String>, Vec<f64>)>;

pub fn project(parties: &Parties, state: &StateAb, year: &str, npp_booths_path: &Path, sa1_breakdown_path: &Path, sa1_prefs_path: &Path){
    eprintln!("\tProjecting results onto SA1s");
    // potential soundness issue: is this going to work out the same way?
    // BTreeMap for Parties in general should fix that
    let mut partykeys: Vec<&str> = Vec::new();
    for k in parties.keys(){
        partykeys.push(&k)
    }
    let combinations = group_combos(&partykeys);
    //println!("Combinations:\n{:#?}", combinations);


    // *** Load up Booth Data ***
    // this is the [booths; orders] matrix equivalent

    let mut boothsfields = vec![String::from("ID"), String::from("Division"), String::from("Booth"), String::from("Latitude"), String::from("Longitude")];
    boothsfields.append(&mut combinations.clone());
    boothsfields.push(String::from("Total"));

    let mut booths : BoothRecords = BTreeMap::new();

    let mut booths_rdr = csv::ReaderBuilder::new().flexible(true).has_headers(true).from_path(npp_booths_path).unwrap();

    // Maybe we can deserialize to boothsfields?
    // That's what we want to do...
    // well, we can mostly do that.

    for record in booths_rdr.records() {
        let row = record.unwrap();
        let divbooth = row[1].to_owned() + "_" + &row[2];
        let mut boothmeta :  Vec<String> = Vec::with_capacity(5);

        for i in 0..5{
            boothmeta.push(row[i].to_string());
        }

        let mut boothvotes : Vec<f64> = Vec::with_capacity(combinations.len());

        for i in 5..row.len(){
            let val = match row[i].parse::<f64>() {
                Ok(x) => x,
                Err(_) => 0.0,
            };
            boothvotes.push(val);
        }
        if row.len() < boothsfields.len() {
            for _ in row.len()..boothsfields.len() {
                boothvotes.push(0.0);
            }
        }

        booths.insert(divbooth, (boothmeta, boothvotes));
    }

    // *** Load up SA1 data ***
    // This is the [sa1s; booths] matrix equivalent
    // Since it's so sparse we

    let mut sa1_rdr = csv::ReaderBuilder::new().flexible(true).has_headers(true).from_path(sa1_breakdown_path).unwrap();

    let mut outputn : BTreeMap<String, Vec<f64>> = BTreeMap::new(); // Our numerical ultimate output. Indexed by ID

    for record in sa1_rdr.records(){
        let row = record.unwrap();

        let id = row.get(sfl("SA1_id")).unwrap().to_owned();

        let divbooth = row[sfl("div_nm")].to_owned() + "_" + &row[sfl("pp_nm")];

        if !(row.get(sfl("state_ab")).unwrap() == &state.to_string()) {
            continue;
            // All SA1s nationwide are in the one file - so any row with the wrong state can be safely skipped.
        } else if !(row.get(sfl("year")).unwrap() == year) {
            println!("Problem in `{}`: Unsupported election year: {}. Exiting.", sa1_breakdown_path.display(), year);
            process::exit(1); // However, the wrong year is definitely cause for concern. Bail.
        }

        let mut output_row : Vec<f64> = vec![0.0_f64; combinations.len()+1]; // one more for total
        if outputn.contains_key(&id){
            output_row = outputn.get(&id).unwrap().clone();
        }

        let sa1_booth_votes : f64 = match row.get(sfl("votes")).unwrap().parse::<f64>() {
            Ok(x) => x,
            Err(_) => 0.0,
        };

        if !&booths.contains_key(&divbooth){
            continue;
        }

        let boothvotes = &booths.get(&divbooth).unwrap().1;
        let boothtotal = boothvotes.last().unwrap();

        if *boothtotal != 0.0_f64 {
            for i in 0..boothvotes.len() {
                *output_row.get_mut(i).unwrap() += boothvotes[i] * sa1_booth_votes / boothtotal;
                // doing it in one go produces slightly different results to the Python,
                // which is concerning...
            }
        }

        outputn.insert(id, output_row);

    }

    // having summed it all up...
    let outlen = boothsfields.len();

    create_dir_all(sa1_prefs_path.parent().unwrap()).unwrap();
    let mut sa1_wtr = csv::Writer::from_path(sa1_prefs_path).unwrap();

    let mut header = vec![String::from("SA1_id")];
    header.append(&mut combinations.clone());
    header.push(String::from("Total"));
    sa1_wtr.write_record(header).expect("error writing SA1_prefs header");

    for (id, row) in outputn.iter(){
        let mut out: Vec<String> = Vec::with_capacity(outlen);
        out.push(id.clone());
        for i in row {
            out.push(i.to_string());
        }
        sa1_wtr.write_record(out).expect("error writing SA1_prefs line");
    }

    sa1_wtr.flush().expect("error finalising SA1_prefs");

}
