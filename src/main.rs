/// nparty: N-Party-Preferred distribution of Australian Senate ballots and subsequent analysis.  
/// Copyright (C) 2017-2022  Alex Jago <abjago@abjago.net>.
/// Released under the MIT or Apache-2.0 licenses, at your option.
#[macro_use]
extern crate serde_derive;

use anyhow::{bail, Context, Result};
use clap::{App, AppSettings, Parser, Subcommand};
use config::{KnownConfigOptions, Scenario};
use utils::ToStateAb;

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::{metadata, File};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

mod aggregator;
mod booths;
mod config;
mod data;
mod multiplier;
mod term;
mod upgrades;
mod utils;

#[derive(Parser, Debug)]
#[clap(version, about)]
#[clap(global_setting(AppSettings::PropagateVersion))]
#[clap(global_setting(AppSettings::UseLongFormatForHelpSubcommand))]
#[clap(global_setting(AppSettings::ArgRequiredElseHelp))]
struct Cli {
    #[clap(subcommand)]
    command: CliCommands,
}

#[derive(Subcommand, Debug)]
enum CliCommands {
    Configure(CliConfigure),
    #[clap(subcommand)]
    Data(CliData),
    List(CliList),
    Run(CliRun),
    #[clap(subcommand)]
    Upgrade(CliUpgrade)
}

/// Either download all necessary AEC data directly, or examine the URLs to the relevant files.
#[derive(Subcommand, Debug)]
#[allow(non_snake_case)]
#[clap(after_help="Please note that you'll also need to convert XLSX to CSV manually. At least for now...")]
enum CliData {
    /// download everything to specified folder
    Download {
        #[clap(parse(from_os_str))]
        DL_FOLDER: PathBuf
    },
    /// write list of downloads to FILE as HTML, or pass `-` to output plain text to stdout instead
    Examine {
        #[clap(parse(from_os_str))]
        FILE: PathBuf
    },
}

/// Upgrade electoral and geographic data files published in older formats to use the latest format.
#[derive(Subcommand, Debug)]
enum CliUpgrade {
    /// upgrade a preference file to the latest format
    Prefs {
        /// suffix for when filenames would collide
        #[clap(long, default_value_t = String::from("_to19"))]
        suffix: String,

        /// shell-style expression to filter input filenames from directory
        #[clap(long, default_value_t = String::from("aec-senate-formalpreferences*"))]
        filter: String,

        /// candidate CSV file
        #[clap(long, value_name="CANDIDATES_FILE", parse(from_os_str))]
        candidates: PathBuf,

        /// input file or directory
        #[clap(parse(from_os_str))]
        input: PathBuf,

        /// output file or directory
        #[clap(parse(from_os_str))]
        output: PathBuf,
    },

    /// convert an SA1s-Districts file from old SA1s to new
    Sa1s {
        /// Indicate lack of header row for input file
        #[clap(long)]
        no_infile_headers: bool,

        /// Columns should be: 'SA1_7DIGITCODE_old', 'SA1_7DIGITCODE_new', 'RATIO'
        #[clap(parse(from_os_str))]
        correspondence_file: PathBuf,

        /// input file; columns should be 'SA1_Id', 'Dist_Name', 'Pop', 'Pop_Share'
        #[clap(parse(from_os_str))]
        input: PathBuf,

        /// output file; columns will be 'SA1_Id', 'Dist_Name', 'Pop', 'Pop_Share'
        #[clap(parse(from_os_str))]
        output: PathBuf,
        
    }
}

/// Upgrade electoral and geographic data files published in older formats to use the latest format.
#[derive(Parser, Debug)]
#[clap(after_help = "Note: Options marked * will be asked for interactively if not specified. (Other options are helpful, but not required.)")]
struct CliConfigure {
    /// * Year of the election
    #[clap(long)]
    year: Option<String>,

    /// * State or Territory
    #[clap(long)]
    state: Option<String>,
    
    /// * Polling Places file
    #[clap(long, parse(from_os_str))]
    polling_places: Option<PathBuf>,
    
    /// * Polling Places to SA1s breakdown file
    #[clap(long, parse(from_os_str))]
    sa1s_breakdown: Option<PathBuf>,
    
    /// * Output Directory
    #[clap(long, parse(from_os_str))]
    output_dir: Option<PathBuf>,

    /// An existing configuration file to take defaults from
    #[clap(long, parse(from_os_str), value_name = "OLD_CONFIG")]
    from: Option<PathBuf>,

    /// The AEC's 'Political Parties' CSV
    #[clap(long, parse(from_os_str))]
    party_details: Option<PathBuf>,

    /// AEC candidate CSV file
    #[clap(parse(from_os_str), value_name = "CANDS_FILE")]
    candidates: PathBuf,

    /// AEC candidate CSV file
    #[clap(parse(from_os_str), value_name = "NEW_CONFIG")]
    configfile: PathBuf,
}

/// List scenarios from the configuration file.
#[derive(Parser, Debug)]
#[clap(after_help = "Scenario tables are printed to standard output. If that's a terminal, they'll be pretty-printed with elastic tabstops. If that's a pipe or file, they'll be tab-separated to make further processing as straightforward as possible.")]
struct CliList {
    /// The configuration file to list scenarios from
    #[clap(parse(from_os_str))]
    configfile: PathBuf,
}

/// Run scenarios from the configuration file.
#[derive(Parser, Debug)]
#[clap(after_help = "Note: You probably don't need to worry about [-c | -d | -p].")]
struct CliRun {
    /// Also output JavaScript from the combination stage, for the website predictor. Ignored if [-d | -p].
    #[clap(long, conflicts_with_all(&["distribute", "project"]))]
    js: bool,

    /// Perform ONLY the party-preferred distribution phase
    #[clap(long, short, conflicts_with_all(&["js", "combine", "project"]))]
    distribute: bool,

    /// Perform ONLY the polling-places to SA1s projection phase
    #[clap(long, short, conflicts_with_all(&["js", "distribute", "project"]))]
    combine: bool,

    /// Perform ONLY the SA1s to districts combination phase
    #[clap(long, short, conflicts_with_all(&["js", "combine", "distribute"]))]
    project: bool,
    
    /// Run a SPECIFIC scenario from the configuration file (can be given multiple times to run several scenarios)
    #[clap(long, short)]
    scenario: Option<Vec<String>>,

    /// The configuration file to run
    #[clap(parse(from_os_str))]
    configfile: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let m = Cli::parse();

    eprintln!("{:#?}", m);

    let yml = todo!();
    let m = App::from(yml).get_matches();

    // eprintln!("{:#?}", &m);

    // Match on various subcommands

    if let Some(sm) = m.subcommand_matches("run") {
        run(sm)?;
    } else if let Some(sm) = m.subcommand_matches("list") {
        let cfgpath: &Path = Path::new(
            sm.value_of_os("configfile")
                .context("Error with configuration-file path.")?,
        );
        config::list_scenarios(cfgpath)?;
    } else if let Some(sm) = m.subcommand_matches("data") {
        if sm.is_present("download") {
            let dldir: &Path = Path::new(
                sm.value_of_os("download")
                    .context("Error with download path.")?,
            );
            data::download(dldir);
        // TODO: prompt to also upgrade
        } else {
            // examine
            let argy = sm.value_of("examine").context("Invalid file.")?;
            if argy == "-" {
                data::examine_txt();
            } else {
                let filey: &Path = Path::new(sm.value_of_os("examine").context("Invalid file.")?);
                data::examine_html(filey);
            }
        }
    } else if let Some(sm) = m.subcommand_matches("upgrade") {
        if let Some(ssm) = sm.subcommand_matches("prefs") {
            do_upgrade_prefs(ssm)?;
        } else {
            bail!("this functionality has not yet been implemented");
        }
    } else if let Some(sm) = m.subcommand_matches("configure") {
        do_configure(sm)?;
    } else {
        bail!("this functionality has not yet been implemented");
    }
    Ok(())
}

// Do the heavy lifting of nparty run so as to keep things clean
fn run(sm: &clap::ArgMatches) -> anyhow::Result<()> {
    let cfgpath: &Path = Path::new(
        sm.value_of_os("configfile")
            .context("Error with configuration-file path.")?,
    );

    // Get data out of config
    let cfg = config::get_scenarios(&config::get_cfg_doc_from_path(cfgpath)?)?;

    let mut scenario_names: Vec<String> = Vec::new();
    if sm.is_present("scenario") {
        for i in sm
            .values_of("scenario")
            .context("error getting scenario values somehow")?
        {
            scenario_names.push(String::from(i));
        }
    } else {
        scenario_names = cfg.keys().cloned().collect();
    }

    for scen_name in scenario_names {
        let scenario = cfg.get(&scen_name).with_context(|| {
            format!(
                "Requested scenario {} not found in configuration file.",
                scen_name
            )
        })?;
        eprintln!("Running Scenario {}", scen_name);
        eprintln!("{:#?}", scenario);

        let sa1b = scenario.sa1s_breakdown.as_ref();
        let sa1p = scenario.sa1s_prefs.as_ref();
        let sa1d = scenario.sa1s_dists.as_ref();
        let nppd = scenario.npp_dists.as_ref();
        let can_project = sa1p.is_some()
            && sa1b.is_some()
            && !sm.is_present("distribute")
            && !sm.is_present("combine");
        let can_combine = sa1p.is_some()
            && sa1d.is_some()
            && nppd.is_some()
            && !sm.is_present("distribute")
            && !sm.is_present("project");
        let can_distribute =
            sm.is_present("distribute") || (!sm.is_present("combine") && !sm.is_present("project"));

        if can_distribute {
            booths::booth_npps(
                &scenario.groups,
                &scenario.state,
                &scenario.prefs_path,
                &scenario.polling_places,
                &scenario.npp_booths,
            )
            .context("Could not perform distribution step; stopping.")?;
        }
        if can_project {
            multiplier::project(
                &scenario.groups,
                &scenario.state,
                &scenario.year,
                &scenario.npp_booths,
                sa1b.unwrap(),
                sa1p.unwrap(),
            )
            .context("Could not perform projection step; stopping.")?;
        }
        if can_combine {
            aggregator::aggregate(
                sa1p.unwrap(),
                sa1d.unwrap(),
                nppd.unwrap(),
                sm.is_present("js"),
                &scenario.groups,
            )
            .context("Could not perform combination step; stopping.")?;
        }
    }
    Ok(())
}

fn do_upgrade_prefs(sm: &clap::ArgMatches) -> anyhow::Result<()> {
    let candspath: &Path = Path::new(
        sm.value_of_os("candidates")
            .context("Error with candidates-file path, does it exist?")?,
    );
    let inpath: &Path = Path::new(sm.value_of_os("input").context("Error with input path.")?);
    let outpath: &Path = Path::new(
        sm.value_of_os("output")
            .context("Error with output path.")?,
    );
    let suffix: &OsStr = sm.value_of_os("suffix").context("missing suffix")?;
    let filter: &str = sm.value_of("filter").context("missing filter")?;

    let mut paths: Vec<(PathBuf, PathBuf)> = Vec::new();

    if inpath.is_dir() {
        if !outpath.is_dir() {
            bail!("Input path is a directory but output path is not.");
        } else {
            let mut query: String = inpath
                .to_str()
                .map(String::from)
                .context("Path conversion error")?;
            query.push_str(filter);

            let ips: Vec<PathBuf> = glob::glob(&query)?.filter_map(Result::ok).collect();

            if inpath == outpath {
                // need to upgrade in place
                // i.e. apply suffix to opaths
                for ip in ips {
                    let mut op_fstem = ip.file_stem().context("No file name")?.to_os_string();
                    op_fstem.push(suffix);
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
        }
    } else {
        let ip = PathBuf::from(inpath);
        if outpath.is_dir() {
            paths.push((
                ip,
                outpath.join(inpath.file_name().context("no file name")?),
            ));
        } else {
            paths.push((ip, PathBuf::from(outpath)));
        }
    }

    for (ipath, opath) in &paths {
        let candsdata = utils::read_candidates(
            File::open(&candspath).context("Couldn't open candidates file")?,
        )?;
        let divstates = upgrades::divstate_creator(
            File::open(&candspath).context("Couldn't open candidates file")?,
        );

        eprintln!("ipath: {} \t opath: {}", ipath.display(), opath.display());

        let era = upgrades::era_sniff(&mut utils::open_csvz_from_path(ipath)?)
            .context("Error determining era of input.")?;

        if era == 2016 {
            // Test if upgrade already exists
            let im = metadata(&ipath).context("In-path doesn't seem to exist?")?;
            let om = metadata(&opath);
            let otime = om.as_ref().map_or(SystemTime::UNIX_EPOCH, |x| {
                x.modified().unwrap_or(SystemTime::UNIX_EPOCH)
            });
            let itime = im.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            if otime > itime {
                // todo: consider testing it's the correct era
                eprintln!("Upgrade already exists; skipping");
                continue;
            } else {
                eprintln!("Upgrading...");
                upgrades::upgrade_prefs_16_19(
                    &mut utils::open_csvz_from_path(ipath)?,
                    &mut utils::get_zip_writer_to_path(opath, "csv")?,
                    &candsdata,
                    &divstates,
                );
            }
        } else {
            eprintln!("No upgrade available - is it already the latest?");
        }
    }
    Ok(())
}

fn do_configure(sm: &clap::ArgMatches) -> anyhow::Result<()> {
    // requireds
    let candspath = Path::new(
        sm.value_of_os("candidates")
            .context("Error with candidates-file path.")?,
    );
    let outpath = Path::new(
        sm.value_of_os("configfile")
            .context("Error with output path.")?,
    );

    // (semi)optionals
    let _datadir = sm.value_of_os("data_dir").map(PathBuf::from);
    let _distdir = sm.value_of_os("dist_dir").map(PathBuf::from);
    let from_scen = sm.value_of_os("from").map(PathBuf::from);
    let output_dir = sm.value_of_os("output_dir").map(PathBuf::from);
    let party_details = sm.value_of_os("party_details").map(PathBuf::from);
    let polling_places = sm.value_of_os("polling_places").map(PathBuf::from);
    let sa1s_breakdown = sm.value_of_os("sa1s_breakdown").map(PathBuf::from);
    let year = sm.value_of("year").map(String::from);
    let state = sm.value_of("state").map(|x| x.to_state_ab());

    let kco = KnownConfigOptions {
        sa1s_dists: None,
        prefs_path: None,
        output_dir,
        party_details,
        polling_places,
        sa1s_breakdown,
        year,
        state,
    };

    let existings: BTreeMap<String, Scenario> = match from_scen {
        Some(p) => config::get_scenarios(&config::get_cfg_doc_from_path(&p)?)?,
        None => BTreeMap::new(),
    };

    let existing = existings.values().next();

    let candsfile = File::open(candspath)?;
    let candidates = utils::read_candidates(candsfile)?;

    let out = config::cli_scenarios(existing, &candidates, &kco)
        .context("Configuration could not be created.")?;
    eprintln!("{:#?}", out);

    let mut outfile =
        File::create(outpath)?;
    config::write_scenarios(out, &mut outfile)?;
    Ok(())
}
