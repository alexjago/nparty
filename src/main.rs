// nparty: N-Party-Preferred distribution of Australian Senate ballots and subsequent analysis.  
// Copyright (C) 2017-2020  Alex Jago <abjago@abjago.net>.
// Released under the MIT or Apache-2.0 licenses, at your option. 

extern crate csv;
#[macro_use] extern crate serde_derive;
extern crate unicode_segmentation;
extern crate ansi_term;
#[macro_use] extern crate maplit;
extern crate atty;
extern crate itertools;
extern crate zip;
extern crate zip_extensions;
extern crate factorial;
#[macro_use] extern crate clap;
extern crate toml_edit;
#[macro_use] extern crate lazy_static;
extern crate tabwriter;

use clap::{App, load_yaml};

use std::path::Path;

mod utils;
mod term;
mod booths;
mod multiplier;
mod aggregator;
mod config;

fn main() {

    let yml = load_yaml!("cli.yaml");
    let m = App::from(yml).get_matches();

    // eprintln!("{:#?}", &m);

    // Match on various subcommands

    if let Some(sm) = m.subcommand_matches("run") {
        run(sm);
    } else if let Some(sm) = m.subcommand_matches("list") {
        let cfgpath: &Path = Path::new(sm.value_of_os("configfile").expect("Error with configuration-file path."));
        config::list_scenarios(cfgpath);
    }

    

}


// Do the heavy lifting of nparty run so as to keep things clean
fn run(sm: &clap::ArgMatches){
    // eprintln!("{:#?}", &sm);

    let cfgpath: &Path = Path::new(sm.value_of_os("configfile").expect("Error with configuration-file path."));
    // eprintln!("{:#?}", &cfgpath);
    
    // Get data out of config
    let cfg = config::get_scenarios(config::get_cfg_doc_from_path(cfgpath)).unwrap();

    let mut scenario_names : Vec<String> = Vec::new();
    if sm.is_present("scenario"){
        for i in sm.values_of("scenario").expect("error getting scenario values somehow"){
            scenario_names.push(String::from(i));
        }
    } else {
        scenario_names = cfg.keys().cloned().collect();
    }

    for scen_name in scenario_names{
        // Which phase(s)?
        let scenario = cfg.get(&scen_name).expect(&format!("Requested scenario {} not found in configuration file", scen_name));
        // eprintln!("{:#?}", scenario);
        eprintln!("Running Scenario {}", scen_name);

        let _sa1b = scenario.sa1s_breakdown.as_ref();
        let _sa1p = scenario.sa1s_prefs.as_ref();
        let _sa1d = scenario.sa1s_dists.as_ref();
        let _nppd = scenario.npp_dists.as_ref();
        let can_project = _sa1p.is_some() && _sa1b.is_some();
        let can_combine = _sa1p.is_some() && _sa1d.is_some() && _nppd.is_some();

        // TODO: make this intelligent - i.e., don't default to -r
        if sm.is_present("distribute"){
            booths::booth_npps(&scenario.groups, &scenario.state, &scenario.prefs_path, &scenario.polling_places, &scenario.npp_booths);
        } else if sm.is_present("project") && can_project {
            multiplier::project(&scenario.groups, &scenario.state, &scenario.year, &scenario.npp_booths, scenario.sa1s_breakdown.as_ref().unwrap(), scenario.sa1s_prefs.as_ref().unwrap());
        } else if sm.is_present("combine") && can_combine {
            aggregator::aggregate(scenario.sa1s_prefs.as_ref().unwrap(), scenario.sa1s_dists.as_ref().unwrap(), scenario.npp_dists.as_ref().unwrap(), sm.is_present("js"));
        } else { // run all phases
            booths::booth_npps(&scenario.groups, &scenario.state, &scenario.prefs_path, &scenario.polling_places, &scenario.npp_booths);
            if can_project {
                multiplier::project(&scenario.groups, &scenario.state, &scenario.year, &scenario.npp_booths, scenario.sa1s_breakdown.as_ref().unwrap(), scenario.sa1s_prefs.as_ref().unwrap());
            }
            if can_combine{
                aggregator::aggregate(scenario.sa1s_prefs.as_ref().unwrap(), scenario.sa1s_dists.as_ref().unwrap(), scenario.npp_dists.as_ref().unwrap(), sm.is_present("js"));
            }
            
        }
    }
}