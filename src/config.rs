//! This file translates Configuration.py
//! Generation and loading of configuration files.

use itertools::Itertools;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::read_to_string;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::result::Result;
use tabwriter::TabWriter;
use toml_edit::*;

use crate::booths::Parties;
use crate::term::{BOLD, END};
use crate::utils::{
    filter_candidates, input, open_csvz_from_path, read_party_abbrvs, CandsData, FilteredCandidate,
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
// pub fn cfgdump(cfgpath: &Path) {
//     // load 'er up
//     let doc = get_cfg_doc_from_path(cfgpath);
//     println!("{:#?}", doc.as_table());
// }

/// Does what it says on the tin (or at least, the function signature).
/// Will panic with relevant error messages if something goes wrong.  
pub fn get_cfg_doc_from_path(cfgpath: &Path) -> Document {
    read_to_string(&cfgpath)
        .expect("Error reading config file")
        .parse::<Document>()
        .expect("Error parsing config file")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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
/// This function can panic (but shouldn't).
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

    Ok(out)
}

// pub struct Defaults {
//     pub scen_items: Scenario,
//     pub data_dir: Option<PathBuf>,
//     pub dist_dir: Option<PathBuf>,
// }

/// this function handles `nparty list`
pub fn list_scenarios(cfgpath: &Path) {
    let headers = "Scenario\tPreferred Parties\tPlace\tYear";
    let mut output = Vec::new();
    let doc = get_cfg_doc_from_path(cfgpath);
    let scenarios = get_scenarios(&doc).unwrap();
    for (name, scenario) in scenarios {
        let state = scenario.state.to_string();
        let groups = scenario.groups.keys().join(" v. ");
        let year = scenario.year;
        output.push(format!("{}\t{}\t{}\t{}", name, groups, state, year));
    }

    if atty::is(atty::Stream::Stdout) {
        let mut tw = TabWriter::new(vec![]);
        writeln!(&mut tw, "{}", headers).unwrap();
        for i in output {
            writeln!(&mut tw, "{}", i).unwrap();
        }
        tw.flush().unwrap();
        let output = String::from_utf8(tw.into_inner().unwrap()).unwrap();
        let firstnewline = output.find('\n').unwrap();
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
    pub sa1s_dists: Option<PathBuf>,
    pub prefs_path: Option<PathBuf>,
    pub output_dir: Option<PathBuf>,
    pub party_details: Option<PathBuf>,
    pub polling_places: Option<PathBuf>,
    pub sa1s_breakdown: Option<PathBuf>,
    pub year: Option<String>,
    pub state: Option<StateAb>,
}

fn get_option_cli<T>(
    name: &str,
    known: &Option<T>,
    existing: Option<&T>,
    skippable: bool,
) -> Option<T>
where
    T: std::fmt::Debug + std::str::FromStr + Clone,
    <T as std::str::FromStr>::Err: std::fmt::Debug,
{
    //! Combine options!
    let mut maybe;
    if known.is_some() {
        return known.clone();
    } else if existing.is_some() {
        let ex = existing.unwrap().clone();
        let maybe = input(&format!("Enter {} [default: {:?}]: ", name, ex)).ok()?;
        if maybe.is_empty() {
            return Some(ex);
        } else {
            return T::from_str(&maybe).ok();
        }
    } else {
        loop {
            maybe = input(&format!("Enter {}: ", name)).ok()?;
            if maybe.is_empty() {
                if skippable {
                    return None;
                }
                continue;
            } else {
                break;
            }
        }
    }
    T::from_str(&maybe).ok()
}

// Now for the big show: gotta generate a thing. Possibly from an existing thing.
// Plan of attack: we need a way to take a bunch of Scenarios and override a Document with it
// Then we have three main functions:
// [x] Turn a Document into Scenarios
// [-] Create a new Scenario from CLI input
// [ ] Update a Document from Scenarios

// `cli_scenarios()` is about creating one or more Scenarios interactively
// Previously with `get_scenarios()` and `get_defaults()` we read them from a toml_edit::Document
// Then with `patch_scenarios()` we shall incorporate the new scenarios into an existing toml_edit::Document
// (and factor out a Defaults section)

pub fn cli_scenarios(
    existing: Option<&Scenario>,
    candidates: &CandsData,
    known_options: &KnownConfigOptions,
) -> Result<BTreeMap<String, Scenario>, std::io::Error> {
    let mut out = BTreeMap::new();
    let mut new_scen: String = input("Define a new Scenario? [Y]/n: ")?.to_uppercase();
    while new_scen.starts_with('Y') || new_scen.is_empty() {
        let year = get_option_cli(
            "year",
            &known_options.year,
            existing.map(|x| &x.year),
            false,
        )
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;

        let polling_places = get_option_cli(
            "polling-places path",
            &known_options.polling_places,
            existing.map(|x| &x.polling_places),
            false,
        )
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;

        let sa1s_breakdown = get_option_cli(
            "polling-places to SA1s path",
            &known_options.sa1s_breakdown,
            existing.and_then(|x| x.sa1s_breakdown.as_ref()),
            true,
        );

        // this needs to have the scenario name too
        let output_dir = get_option_cli(
            "output directory",
            &known_options.output_dir,
            existing.map(|x| &x.output_dir),
            false,
        )
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;

        let sa1s_dists = get_option_cli(
            "SA1s-to-districts file path",
            &known_options.sa1s_dists,
            existing.and_then(|x| x.sa1s_dists.as_ref()),
            true,
        );

        let prefs_path = get_option_cli(
            "preferences file path",
            &known_options.prefs_path,
            existing.map(|x| &x.prefs_path),
            false,
        )
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;

        let party_details = get_option_cli(
            "party-details file path",
            &known_options.party_details,
            None,
            false,
        )
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
        let party_details_file = open_csvz_from_path(&party_details);
        let party_abbrvs = read_party_abbrvs(party_details_file);

        let state = get_option_cli(
            "state or territory",
            &known_options.state,
            existing.map(|x| &x.state),
            true,
        )
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;

        // now for the tricky bit
        let mut groups = Parties::new();

        // Add a Group
        let mut add_group = String::from("Y");

        while add_group.starts_with('Y') || add_group.is_empty() {
            // what is a group but a list of candidates?
            let mut group_cands: HashSet<String> = HashSet::new();
            let mut group_parties: HashSet<String> = HashSet::new();

            // search-filter candidates in a loop to add to the group
            loop {
                let pattern = input(&format!(
                    "Search in {} (case-insensitive, regex allowed):\n",
                    state
                ))?;

                if pattern.is_empty() {
                    let done = input("Finished adding to group? [Y]/n: ")?.to_uppercase();
                    if done.starts_with('Y') || done.is_empty() {
                        // name and insert the group
                        // TODO once we have party abbrs back online: name groups by party abbr where available
                        let suggested_name = group_parties.iter().join("");
                        let group_name =
                            get_option_cli("group name", &None, Some(&suggested_name), false)
                                .ok_or_else(|| {
                                    std::io::Error::from(std::io::ErrorKind::NotFound)
                                })?;
                        groups.insert(group_name, group_cands.into_iter().collect());
                        break;
                    }
                }

                let fc: Vec<FilteredCandidate> = filter_candidates(candidates, &state, &pattern);

                if !fc.is_empty() {
                    println!("Selected Candidates for {}", state);

                    let mut tw = TabWriter::new(vec![]);
                    for c in &fc {
                        writeln!(&mut tw, "{}", &c.fmt_tty()).expect("couldn't write selection");
                    }
                    tw.flush().unwrap();
                    print!("{}", String::from_utf8(tw.into_inner().unwrap()).unwrap());

                    // add candidates
                    let whatdo =
                        input("Add selected candidate[s] to group? [Y]/n: ")?.to_uppercase();
                    if whatdo.starts_with('Y') || whatdo.is_empty() {
                        for cand in &fc {
                            let candstr = match cand.surname.as_str() {
                                "TICKET" => format!("{}:{}", cand.ticket, cand.party),
                                _ => format!(
                                    "{}:{} {}",
                                    cand.ticket, cand.surname, cand.ballot_given_nm
                                ),
                            };
                            group_cands.insert(candstr);
                            group_parties.insert(
                                party_abbrvs.get(&cand.party).unwrap_or(&cand.party).clone(),
                            );
                        }
                    }
                }
            } // end of add-candidates loop
            add_group = input("Add a new group? [Y]/n")?.to_uppercase();
            if !(add_group.starts_with('Y') || add_group.is_empty()) {
                break;
            }
        }

        // scenario code name
        let mut name = format!(
            "{}_{}PP_{}",
            state,
            groups.keys().len(),
            groups.keys().join("")
        );
        let keepit =
            input(&format!("Use suggested scenario code {} [Y]/n: ", name))?.to_uppercase();
        if !(keepit.starts_with('Y') || keepit.is_empty()) {
            name = String::new();
            while name.is_empty() {
                name = input("Please type a short code to name the new Scenario: ")?;
            }
        }

        // I see no reason to go to the CLI on these. Generator == Defaults Are Fine Here
        let npp_booths = PathBuf::from(&output_dir)
            .join(&name)
            .join("NPP_Booths.csv");
        let sa1s_prefs = Some(
            PathBuf::from(&output_dir)
                .join(&name)
                .join("SA1s_Prefs.csv"),
        );
        let npp_dists = Some(PathBuf::from(&output_dir).join(&name).join("NPP_Dists.csv"));

        let scenario = Scenario {
            name: name.clone(),
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
        };

        out.insert(name.clone(), scenario);
        // go again?
        new_scen = input("Define another new Scenario? [Y]/n: ")?.to_uppercase();
    }

    Ok(out)
}

// TODO: function to write scenarios back out
pub fn write_scenarios(
    input: BTreeMap<String, Scenario>,
    outfile: &mut dyn Write,
) -> Result<(), std::io::Error> {
    let outdoc: Document =
        ser::to_document(&input).expect("Error converting Scenarios to TOML document");
    outfile.write_all(outdoc.to_string().as_bytes())?;
    Ok(())
}
