use atty;
use itertools::Itertools;
use regex::{Regex, RegexBuilder};
use std::collections::{BTreeMap, HashMap};
use std::fs::{read_to_string, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::result::Result;
use tabwriter::TabWriter;
/// This file translates Configuration.py
/// Generation and loading of configuration files.
use toml_edit::*;

use crate::booths::Parties;
use crate::term::{BOLD, END};
use crate::utils::{
    filter_candidates, input, read_candidates, read_party_abbrvs, CandsData, FilteredCandidate,
    StateAb, ToStateAb,
};

// TODO: long term goals to get back to Python equivalent functionality
// We will support a TOML setup that's otherwise consistent with Python's ConfigParser's
// "basic interpolation" mode. This means there's a special [DEFAULT] section, and then
// other, arbitrarily-named sections after that.
// Interpolation will pull from other keys in that section and then from [DEFAULT] if needed.
// To have an interpolation reference loop is a runtime error.

// But for now, interpolation is way too much effort.
// Let's step back and add that back in once TOML supports it down the line.
// see https://github.com/toml-lang/toml/issues/445
// Or at least put it in a separate crate

// We're keeping defaults though.

/// Quickly dump a configuration from a file
pub fn cfgdump(cfgpath: &Path) {
    // load 'er up
    let doc = get_cfg_doc_from_path(cfgpath);
    println!("{:#?}", doc.as_table());
}

/// Does what it says on the tin (or at least, the function signature).
/// Will panic with relevant error messages if something goes wrong.  
pub fn get_cfg_doc_from_path(cfgpath: &Path) -> Document {
    read_to_string(&cfgpath)
        .expect("Error reading config file")
        .parse::<Document>()
        .expect("Error parsing config file")
}

#[derive(Debug)]
pub struct Scenario {
    pub name: String,
    pub year: String,
    pub polling_places: PathBuf,
    pub sa1s_breakdown: Option<PathBuf>,
    pub output_dir: PathBuf,
    pub npp_booths: PathBuf,
    pub sa1s_prefs: Option<PathBuf>,
    pub npp_dists: Option<PathBuf>,
    pub prefs_path: PathBuf,
    pub sa1s_dists: Option<PathBuf>,
    pub state: StateAb,
    pub groups: Parties,
    // Optional paths are those for the latter two phases
}

/// Get all the Scenarios, with defaults suitably propogated and paths ready to use!
/// This function can panic.
pub fn get_scenarios(cfg: &Document) -> Result<BTreeMap<String, Scenario>, &'static str> {
    let mut out: BTreeMap<String, Scenario> = BTreeMap::new();
    let cfg = cfg.as_table();

    // We pop the contents of [DEFAULT] into a HashMap to avoid existence failure
    let mut defaults: HashMap<&str, &Item> = HashMap::new();
    if cfg.contains_key("DEFAULT") {
        for (key, item) in cfg.get("DEFAULT").unwrap().as_table().unwrap().iter() {
            defaults.insert(key, item);
        }
    }

    for (scenario_key, scenario) in cfg.iter() {
        let scenario = scenario.as_table().unwrap();
        // Iterating over scenarios
        if scenario_key == "DEFAULT" {
            continue;
        }

        // println!("{}, {}", scenario_key, scenario);

        // Large amount of boilerplate follows!
        let name = String::from(scenario_key);

        let year;
        if scenario.contains_key("YEAR") {
            year = String::from(scenario.get("YEAR").unwrap().as_str().unwrap());
        } else if defaults.contains_key("YEAR") {
            year = String::from(defaults.get("YEAR").unwrap().as_str().unwrap());
        } else {
            return Err("Missing YEAR");
        }

        // Non-Optional paths: POLLING_PLACES_PATH, OUTPUT_DIR, NPP_BOOTHS_FN, PREFS_PATH

        let polling_places: PathBuf;
        if scenario.contains_key("POLLING_PLACES_PATH") {
            polling_places = PathBuf::from(
                scenario
                    .get("POLLING_PLACES_PATH")
                    .unwrap()
                    .as_str()
                    .unwrap(),
            );
        } else if defaults.contains_key("POLLING_PLACES_PATH") {
            polling_places = PathBuf::from(
                defaults
                    .get("POLLING_PLACES_PATH")
                    .unwrap()
                    .as_str()
                    .unwrap(),
            );
        } else {
            return Err("Missing POLLING_PLACES_PATH");
        }

        let output_dir: PathBuf;
        if scenario.contains_key("OUTPUT_DIR") {
            output_dir = PathBuf::from(scenario.get("OUTPUT_DIR").unwrap().as_str().unwrap());
        } else if defaults.contains_key("OUTPUT_DIR") {
            output_dir = PathBuf::from(defaults.get("OUTPUT_DIR").unwrap().as_str().unwrap());
        } else {
            return Err("Missing OUTPUT_DIR");
        }

        let mut npp_booths = output_dir.clone();
        if scenario.contains_key("NPP_BOOTHS_FN") {
            npp_booths.push(&name);
            npp_booths.push(scenario.get("NPP_BOOTHS_FN").unwrap().as_str().unwrap());
        } else if defaults.contains_key("NPP_BOOTHS_FN") {
            npp_booths.push(&name);
            npp_booths.push(defaults.get("NPP_BOOTHS_FN").unwrap().as_str().unwrap());
        } else {
            return Err("Missing NPP_BOOTHS_FN");
        }

        let prefs_path: PathBuf;
        if scenario.contains_key("PREFS_PATH") {
            prefs_path = PathBuf::from(scenario.get("PREFS_PATH").unwrap().as_str().unwrap());
        } else if defaults.contains_key("PREFS_PATH") {
            prefs_path = PathBuf::from(defaults.get("PREFS_PATH").unwrap().as_str().unwrap());
        } else {
            return Err("Missing PREFS_PATH");
        }

        // Optional Paths: SA1S_BREAKDOWN_PATH, SA1S_PREFS_FN, NPP_DISTS_FN, SA1S_DISTS_PATH

        let sa1s_breakdown: Option<PathBuf>;
        if scenario.contains_key("SA1S_BREAKDOWN_PATH") {
            sa1s_breakdown = Some(PathBuf::from(
                scenario
                    .get("SA1S_BREAKDOWN_PATH")
                    .unwrap()
                    .as_str()
                    .unwrap(),
            ));
        } else if defaults.contains_key("SA1S_BREAKDOWN_PATH") {
            sa1s_breakdown = Some(PathBuf::from(
                defaults
                    .get("SA1S_BREAKDOWN_PATH")
                    .unwrap()
                    .as_str()
                    .unwrap(),
            ));
        } else {
            sa1s_breakdown = None;
        }

        let sa1s_prefs: Option<PathBuf>;
        let mut _sa1p = output_dir.clone();
        if scenario.contains_key("SA1S_PREFS_FN") {
            _sa1p.push(&name);
            _sa1p.push(scenario.get("SA1S_PREFS_FN").unwrap().as_str().unwrap());
            sa1s_prefs = Some(_sa1p);
        } else if defaults.contains_key("SA1S_PREFS_FN") {
            _sa1p.push(&name);
            _sa1p.push(defaults.get("SA1S_PREFS_FN").unwrap().as_str().unwrap());
            sa1s_prefs = Some(_sa1p);
        } else {
            sa1s_prefs = None;
        }

        let npp_dists: Option<PathBuf>;
        let mut _nppd = output_dir.clone();
        if scenario.contains_key("NPP_DISTS_FN") {
            _nppd.push(&name);
            _nppd.push(scenario.get("NPP_DISTS_FN").unwrap().as_str().unwrap());
            npp_dists = Some(_nppd);
        } else if defaults.contains_key("NPP_DISTS_FN") {
            _nppd.push(&name);
            _nppd.push(defaults.get("NPP_DISTS_FN").unwrap().as_str().unwrap());
            npp_dists = Some(_nppd);
        } else {
            npp_dists = None;
        }

        let sa1s_dists: Option<PathBuf>;
        if scenario.contains_key("SA1S_DISTS_PATH") {
            sa1s_dists = Some(PathBuf::from(
                scenario.get("SA1S_DISTS_PATH").unwrap().as_str().unwrap(),
            ));
        } else if defaults.contains_key("SA1S_DISTS_PATH") {
            sa1s_dists = Some(PathBuf::from(
                defaults.get("SA1S_DISTS_PATH").unwrap().as_str().unwrap(),
            ));
        } else {
            sa1s_dists = None;
        }

        let state: StateAb;

        if scenario.contains_key("STATE") {
            state = scenario
                .get("STATE")
                .unwrap()
                .as_str()
                .unwrap()
                .to_state_ab();
        } else if defaults.contains_key("STATE") {
            state = defaults
                .get("STATE")
                .unwrap()
                .as_str()
                .unwrap()
                .to_state_ab();
        } else {
            return Err("Missing STATE");
        }

        // Really the only complicated parse is the GROUPS.

        let mut groups: Parties = BTreeMap::new();
        if scenario.contains_key("GROUPS") {
            for (group_name, group) in scenario.get("GROUPS").unwrap().as_table().unwrap().iter() {
                let groupvec = group
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| String::from(x.as_str().unwrap()))
                    .collect::<Vec<String>>();
                groups.insert(String::from(group_name), groupvec);
            }
        } else if defaults.contains_key("GROUPS") {
            for (group_name, group) in defaults.get("GROUPS").unwrap().as_table().unwrap().iter() {
                let groupvec = group
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| String::from(x.as_str().unwrap()))
                    .collect::<Vec<String>>();
                groups.insert(String::from(group_name), groupvec);
            }
        } else {
            return Err("Missing GROUPS");
        }

        out.insert(
            String::from(&name),
            Scenario {
                name,
                year,
                polling_places,
                sa1s_breakdown,
                output_dir,
                npp_booths,
                sa1s_prefs,
                npp_dists,
                prefs_path,
                sa1s_dists,
                state,
                groups,
            },
        );
    }

    return Ok(out);
}

pub struct Defaults {
    pub scen_items: Scenario,
    pub data_dir: Option<PathBuf>,
    pub dist_dir: Option<PathBuf>,
}

pub fn get_defaults(cfg: &Document) -> Result<Defaults, &'static str> {
    let mut out: Defaults;
    let mut scen_items: Scenario;
    let cfg = cfg.as_table();

    // We pop the contents of [DEFAULT] into a HashMap to avoid existence failure
    let mut defaults: HashMap<&str, &Item> = HashMap::new();
    if cfg.contains_key("DEFAULT") {
        for (key, item) in cfg.get("DEFAULT").unwrap().as_table().unwrap().iter() {
            defaults.insert(key, item);
        }
    }

    for (scenario_key, scenario) in cfg.iter() {
        let scenario = scenario.as_table().unwrap();
        // Iterating over scenarios
        if scenario_key == "DEFAULT" {
            continue;
        }
    }

    //    return Ok(out);
    todo!()
}
/// this function handles `nparty list`
pub fn list_scenarios(cfgpath: &Path) {
    let headers = "Scenario\tPreferred Parties\tPlace\tYear";
    let mut output = Vec::new();
    let doc = get_cfg_doc_from_path(&cfgpath);
    let scenarios = get_scenarios(&doc).unwrap();
    for (name, scenario) in scenarios {
        let state = scenario.state.to_string();
        let groups = scenario.groups.keys().join(" v. ");
        let year = scenario.year;
        output.push(format!("{}\t{}\t{}\t{}", name, groups, state, year));
    }

    if atty::is(atty::Stream::Stdout) {
        let mut tw = TabWriter::new(vec![]);
        write!(&mut tw, "{}\n", headers).unwrap();
        for i in output {
            write!(&mut tw, "{}\n", i).unwrap();
        }
        tw.flush().unwrap();
        let output = String::from_utf8(tw.into_inner().unwrap()).unwrap();
        let firstnewline = output.find("\n").unwrap();
        let hline = &output[0..firstnewline];
        let rline = &output[firstnewline..output.len()];
        println!("{}{}{}{}", BOLD, hline, END, rline);
    } else {
        println!("{}", headers);
        for i in output {
            println!("{}", i);
        }
    }
}

pub struct KnownConfigOptions {
    pub data_dir: Option<PathBuf>,
    pub dist_dir: Option<PathBuf>,
    pub output_dir: Option<PathBuf>,
    pub party_details: Option<PathBuf>,
    pub polling_places: Option<PathBuf>,
    pub sa1s_breakdown: Option<PathBuf>,
    pub year: Option<String>,
}

// Now for the big show: gotta generate a thing. Possibly from an existing thing.
// Plan of attack: we need a way to take a bunch of Scenarios and override a Document with it
// Then we have three main functions:
// [x] Turn a Document into Scenarios
// [ ] Create a new Scenario from CLI input
// [ ] Update a Document from Scenarios

/// `cli_scenarios()` is about creating one or more Scenarios interactively
/// Previously with `get_scenarios()` and `get_defaults()` we read them from a toml_edit::Document
/// Then with `patch_scenarios()` we shall incorporate the new scenarios into an existing toml_edit::Document
/// (and factor out a Defaults section)
pub fn cli_scenarios(
    existing: &Option<Scenario>,
    candidates: &CandsData,
    known_options: KnownConfigOptions,
) -> Result<BTreeMap<String, Scenario>, String> {
    //let mut out = BTreeMap::new();

    let mut new_scen: String = input("Define a new Scenario? [Y]/n: ")
        .expect("kthxbai")
        .to_uppercase();
    while new_scen.starts_with('Y') || new_scen.len() == 0 {
        // let mut scenario: Scenario etc etc

        // data dir: ultimately we want to leverage this for a list of filenames
        // let data_dir = known_options.data_dir;
        // dist dir dict?

        // state

        // party details

        // paths paths paths

        // filtering

        // scenario code
        let mut scen_code = String::new();

        // out.insert(scen_code, scenario);
        // go again?
        new_scen = input("Define another new Scenario? [Y]/n: ")
            .expect("kthxbai")
            .to_uppercase();
    }

    //    Ok(out)
    todo!()
}
