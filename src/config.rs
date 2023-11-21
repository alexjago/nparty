//! Generation and loading of configuration files.

use crate::booths::Parties;
use crate::term::{BOLD, END};
use crate::utils::{
    filter_candidates, input, open_csvz_from_path, read_party_abbrvs, CandsData, FilteredCandidate,
    StateAb,
};
use color_eyre::eyre::{bail, Context, ContextCompat, Result};
use indexmap::{IndexMap, IndexSet};
use itertools::Itertools;
use std::collections::{BTreeMap, HashMap};
use std::fs::read_to_string;
use std::io::Write;
use std::path::{Path, PathBuf};
use tabwriter::TabWriter;
use toml_edit::{ser, Document, Item, TableLike};

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
// pub fn cfgdump(cfgpath: &Path) -> Result<()> {
//     // load 'er up
//     let doc = get_cfg_doc_from_path(cfgpath)?;
//     println!("{:#?}", doc.as_table());
//     Ok(())
// }

/// Does what it says on the tin (or at least, the function signature).
pub fn get_cfg_doc_from_path(cfgpath: &Path) -> Result<Document> {
    read_to_string(cfgpath)
        .context("Config file could not be read")?
        .parse::<Document>()
        .context("Config file could not be parsed")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scenario {
    #[serde(rename = "NAME")]
    pub name: String,
    #[serde(rename = "YEAR")]
    pub year: String,
    #[serde(rename = "POLLING_PLACES_PATH")]
    pub polling_places: PathBuf,
    #[serde(rename = "SA1S_BREAKDOWN_PATH")]
    pub sa1s_breakdown: Option<PathBuf>,
    #[serde(rename = "OUTPUT_DIR")]
    pub output_dir: PathBuf,
    #[serde(rename = "NPP_BOOTHS_FN")]
    pub npp_booths: PathBuf,
    #[serde(rename = "SA1S_PREFS_FN")]
    pub sa1s_prefs: Option<PathBuf>,
    #[serde(rename = "NPP_DISTS_FN")]
    pub npp_dists: Option<PathBuf>,
    #[serde(rename = "PREFS_PATH")]
    pub prefs_path: PathBuf,
    #[serde(rename = "SA1S_DISTS_PATH")]
    pub sa1s_dists: Option<PathBuf>,
    #[serde(rename = "STATE")]
    pub state: StateAb,
    #[serde(rename = "GROUPS")]
    #[serde(with = "indexmap::serde_seq")]
    pub groups: Parties,
    // Optional paths are those for the latter two phases
}

/// Get all the Scenarios, with defaults suitably propogated and paths ready to use!
/// This function can panic (but shouldn't).
pub fn get_scenarios(cfg: &Document) -> Result<BTreeMap<String, Scenario>> {
    let mut out: BTreeMap<String, Scenario> = BTreeMap::new();
    let cfg = cfg.as_table();

    // We pop the contents of [DEFAULT] into a HashMap to avoid existence failure
    let mut defaults: HashMap<&str, &Item> = HashMap::new();
    if cfg.contains_key("DEFAULT") {
        for (key, item) in cfg.get("DEFAULT").unwrap().as_table().unwrap() {
            defaults.insert(key, item);
        }
    }

    for (scenario_key, scenario_raw) in cfg {
        // eprintln!(
        //     "{}\n{}\n{:?}",
        //     scenario_key,
        //     scenario_raw.is_table_like(),
        //     scenario_raw
        // );
        // let scenario = scenario.as_table().context("Couldn't construct scenario table on config load")?;
        let scenario: &dyn TableLike = scenario_raw
            .as_table_like()
            .context("Couldn't construct scenario table on config load")?;
        // Iterating over scenarios
        if scenario_key == "DEFAULT" {
            continue;
        }

        // Fair amount of boilerplate follows!

        // NAME always known from scenario directly
        let name = String::from(scenario_key);

        #[allow(clippy::items_after_statements)]
        /// We are able to abstract out much of the logic into this...
        fn get_attribute<'a, T, F>(
            key: &'a str,
            scenario: &'a dyn TableLike,
            defaults: &'a HashMap<&str, &Item>,
            conversion_fn: F,
        ) -> Option<T>
        where
            F: FnOnce(&'a str) -> T,
        {
            scenario
                .get(key)
                .or_else(|| defaults.get(key).copied())
                .and_then(toml_edit::Item::as_str)
                .map(conversion_fn)
        }

        // Non-Optional: YEAR
        let year =
            get_attribute("YEAR", scenario, &defaults, String::from).context("Missing YEAR")?;

        // Non-Optional paths: POLLING_PLACES_PATH, OUTPUT_DIR, NPP_BOOTHS_FN, PREFS_PATH

        let polling_places =
            get_attribute("POLLING_PLACES_PATH", scenario, &defaults, PathBuf::from)
                .context("Missing POLLING_PLACES_PATH")?;

        let output_dir = get_attribute("OUTPUT_DIR", scenario, &defaults, PathBuf::from)
            .context("Missing OUTPUT_DIR")?;

        let npp_booths = get_attribute("NPP_BOOTHS_FN", scenario, &defaults, PathBuf::from)
            .map(|x| output_dir.clone().join(&name).join(x))
            .context("Missing NPP_BOOTHS_FN")?;

        let prefs_path = get_attribute("PREFS_PATH", scenario, &defaults, PathBuf::from)
            .context("Missing PREFS_PATH")?;

        // Optional Paths: SA1S_BREAKDOWN_PATH, SA1S_PREFS_FN, NPP_DISTS_FN, SA1S_DISTS_PATH

        let sa1s_breakdown =
            get_attribute("SA1S_BREAKDOWN_PATH", scenario, &defaults, PathBuf::from);

        let sa1s_prefs = get_attribute("SA1S_PREFS_FN", scenario, &defaults, PathBuf::from)
            .map(|x| output_dir.clone().join(&name).join(x));

        let npp_dists = get_attribute("NPP_DISTS_FN", scenario, &defaults, PathBuf::from)
            .map(|x| output_dir.clone().join(&name).join(x));

        let sa1s_dists = get_attribute("SA1S_DISTS_PATH", scenario, &defaults, PathBuf::from);

        // Not optional: STATE
        let state: StateAb =
            get_attribute("STATE", scenario, &defaults, StateAb::from).context("Missing STATE")?;

        // Really the only complicated parse is the GROUPS.
        let mut groups: Parties = IndexMap::new();
        if scenario.contains_key("GROUPS") {
            for (group_name, group) in scenario.get("GROUPS").unwrap().as_table().unwrap() {
                let groupvec = group
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| String::from(x.as_str().unwrap()))
                    .collect::<Vec<String>>();
                groups.insert(String::from(group_name), groupvec);
            }
        } else if defaults.contains_key("GROUPS") {
            for (group_name, group) in defaults.get("GROUPS").unwrap().as_table().unwrap() {
                let groupvec = group
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| String::from(x.as_str().unwrap()))
                    .collect::<Vec<String>>();
                groups.insert(String::from(group_name), groupvec);
            }
        } else {
            bail!("Missing GROUPS");
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
pub fn list_scenarios(cfgpath: &Path) -> Result<()> {
    let headers = "Scenario\tPreferred Parties\tPlace\tYear";
    let mut output = Vec::new();
    let doc = get_cfg_doc_from_path(cfgpath)?;
    let scenarios = get_scenarios(&doc)?;
    for (name, scenario) in scenarios {
        let state = scenario.state.to_string();
        let groups = scenario.groups.keys().join(" v. ");
        let year = scenario.year;
        output.push(format!("{name}\t{groups}\t{state}\t{year}"));
    }

    if std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        let mut tw = TabWriter::new(vec![]);
        writeln!(&mut tw, "{headers}")?;
        for i in output {
            writeln!(&mut tw, "{i}")?;
        }
        tw.flush()?;
        let output = String::from_utf8(tw.into_inner()?)?;
        let firstnewline = output.find('\n').unwrap();
        let head = &output[0..firstnewline];
        let body = &output[firstnewline..output.len()];
        println!("{BOLD}{head}{END}{body}");
    } else {
        println!("{headers}");
        for i in output {
            println!("{i}");
        }
    }
    Ok(())
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
    }
    if existing.is_some() {
        let ex = existing?.clone();
        let maybe = input(&format!("Enter {name} [default: {ex:?}]: ")).ok()?;
        if maybe.is_empty() {
            return Some(ex);
        }
        return T::from_str(&maybe).ok();
    }
    loop {
        maybe = input(&format!("Enter {name}: ")).ok()?;
        if maybe.is_empty() {
            if skippable {
                return None;
            }
            continue;
        }
        break;
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
) -> Result<BTreeMap<String, Scenario>> {
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
        let party_details_file = open_csvz_from_path(&party_details)?;
        let party_abbrvs = read_party_abbrvs(party_details_file);

        let state = get_option_cli(
            "state or territory",
            &known_options.state,
            existing.map(|x| &x.state),
            true,
        )
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;

        // now for the tricky bit
        let mut groups = IndexMap::new();

        // Add a Group
        let mut add_group = String::from("Y");

        while add_group.starts_with('Y') || add_group.is_empty() {
            // what is a group but a list of candidates?
            let mut group_cands: IndexSet<String> = IndexSet::new();
            let mut group_parties: IndexSet<String> = IndexSet::new();

            // search-filter candidates in a loop to add to the group
            loop {
                let pattern = input(&format!(
                    "Search in {state} (case-insensitive, regex allowed):\n"
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

                let fc: Vec<FilteredCandidate> = filter_candidates(candidates, state, &pattern);

                if !fc.is_empty() {
                    println!("Selected Candidates for {state}");

                    let mut tw = TabWriter::new(vec![]);
                    for c in &fc {
                        writeln!(&mut tw, "{}", &c.fmt_tty())?;
                    }
                    tw.flush()?;
                    print!("{}", String::from_utf8(tw.into_inner().unwrap()).unwrap());

                    // add candidates
                    let whatdo =
                        input("Add selected candidate[s] to group? [Y]/n: ")?.to_uppercase();
                    if whatdo.starts_with('Y') || whatdo.is_empty() {
                        for cand in &fc {
                            let candstr = if cand.surname.as_str() == "TICKET" {
                                format!("{}:{}", cand.ticket, cand.party)
                            } else {
                                format!("{}:{} {}", cand.ticket, cand.surname, cand.ballot_given_nm)
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
        let keepit = input(&format!("Use suggested scenario code {name} [Y]/n: "))?.to_uppercase();
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

/// Write an entire `BTreeMap` of `Scenarios` back out to TOML
pub fn write_scenarios(input: &BTreeMap<String, Scenario>, outfile: &mut dyn Write) -> Result<()> {
    // we want the top-level tables in the doc to use [key] formatting and for groups to use [key.groups] formatting
    // so the "pretty" formatting gives us that
    // (this is important, because non-pretty results in inline tables)
    let outstring = ser::to_string_pretty(&input).context("Error converting Scenario to TOML")?;
    outfile.write_all(outstring.as_bytes())?;
    Ok(())
}
