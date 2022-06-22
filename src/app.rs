//! The main app logic: argument structs and most top-level functions

use std::collections::BTreeMap;
use std::fs::{metadata, File};
use std::path::PathBuf;
use std::time::SystemTime;

use crate::config::*;
use crate::utils::ToStateAb;
use crate::*;
use anyhow::{bail, Context, Result};
use clap::{AppSettings, ArgEnum, Parser, Subcommand, ValueHint};

#[derive(Parser, Debug)]
#[clap(version, about)]
#[clap(global_setting(AppSettings::PropagateVersion))]
#[clap(global_setting(AppSettings::UseLongFormatForHelpSubcommand))]
pub struct Cli {
    #[clap(subcommand)]
    pub command: CliCommands,
}

#[derive(Subcommand, Debug, PartialEq)]
pub enum CliCommands {
    Configure(CliConfigure),
    #[clap(subcommand)]
    Data(CliData),
    Example(CliExample),
    /// View license information and acknowledgements
    License,
    List(CliList),
    /// View project README.md
    Readme,
    Run(CliRun),
    #[clap(subcommand)]
    Upgrade(CliUpgrade),
}

/// Either download all necessary AEC data directly, or examine the URLs to the relevant files.
#[derive(Parser, Debug, PartialEq)]
#[allow(non_snake_case)]
#[clap(
    after_help = "Please note that you'll also need to convert XLSX to CSV manually. At least for now..."
)]
pub enum CliData {
    /// download everything to specified folder
    Download {
        #[clap(value_hint = ValueHint::DirPath)]
        #[clap(parse(from_os_str))]
        DL_FOLDER: PathBuf,
    },
    /// write list of downloads to FILE as HTML, or as plain text to stdout if no file is specified
    Examine {
        #[clap(value_hint = ValueHint::FilePath)]
        #[clap(parse(from_os_str))]
        FILE: Option<PathBuf>,
    },
}

/// Print an example configuration for the specified year, if available
#[derive(Parser, Debug, PartialEq)]
#[allow(non_snake_case)]
pub struct CliExample {
    /// the year of the config
    year: String,
}

/// Upgrade older electoral and geographic data files to be compatible with more recent ones.
#[derive(Parser, Debug, PartialEq)]
pub enum CliUpgrade {
    /// upgrade a preference file to the latest format (e.g. 2016 to 2019)
    Prefs(CliUpgradePrefs),
    /// convert an SA1s-Districts file from old SA1s to new (e.g. 2011 to 2016 ASGS)
    Sa1s(CliUpgradeSa1s),
}

#[derive(Parser, Debug, PartialEq)]
pub struct CliUpgradePrefs {
    /// suffix for when filenames would collide
    #[clap(long, default_value_t = String::from("_to19"))]
    pub suffix: String,

    /// shell-style expression to filter input filenames from directory
    #[clap(long, default_value_t = String::from("aec-senate-formalpreferences*"))]
    pub filter: String,

    /// candidate CSV file
    #[clap(long, value_name = "CANDIDATES_FILE", parse(from_os_str), value_hint = ValueHint::FilePath)]
    pub candidates: PathBuf,

    /// input file or directory
    #[clap(parse(from_os_str), value_hint = ValueHint::AnyPath)]
    pub input: PathBuf,

    /// output file or directory
    #[clap(parse(from_os_str), value_hint = ValueHint::AnyPath)]
    pub output: PathBuf,
}

#[derive(Parser, Debug, PartialEq)]
pub struct CliUpgradeSa1s {
    /// Indicate lack of header row for input file
    #[clap(long)]
    pub no_infile_headers: bool,

    /// Columns should be: 'SA1_7DIGITCODE_old', 'SA1_7DIGITCODE_new', 'RATIO'
    #[clap(parse(from_os_str), value_hint = ValueHint::FilePath)]
    pub correspondence_file: PathBuf,

    /// input file; columns should be 'SA1_Id', 'Dist_Name', 'Pop', 'Pop_Share'
    #[clap(parse(from_os_str), value_hint = ValueHint::FilePath)]
    pub input: PathBuf,

    /// output file; columns will be 'SA1_Id', 'Dist_Name', 'Pop', 'Pop_Share'
    #[clap(parse(from_os_str), value_hint = ValueHint::FilePath)]
    pub output: PathBuf,
}

/// Generate a configuration file interactively, possibly using an existing file as a basis.
#[derive(Parser, Debug, PartialEq)]
#[clap(
    after_help = "Note: Options marked * will be asked for interactively if not specified. (Other options are helpful, but not required.)"
)]
pub struct CliConfigure {
    /// * Year of the election
    #[clap(long)]
    pub year: Option<String>,

    /// * State or Territory
    #[clap(long)]
    pub state: Option<String>,

    /// * Polling Places file
    #[clap(long, parse(from_os_str), value_hint = ValueHint::FilePath)]
    pub polling_places: Option<PathBuf>,

    /// * Polling Places to SA1s breakdown file
    #[clap(long, parse(from_os_str), value_hint = ValueHint::FilePath)]
    pub sa1s_breakdown: Option<PathBuf>,

    /// * Output Directory
    #[clap(long, parse(from_os_str), value_hint = ValueHint::DirPath)]
    pub output_dir: Option<PathBuf>,

    /// An existing configuration file to take defaults from
    #[clap(long, parse(from_os_str), value_name = "OLD_CONFIG", value_hint = ValueHint::FilePath)]
    pub from: Option<PathBuf>,

    /// The AEC's 'Political Parties' CSV
    #[clap(long, parse(from_os_str), value_hint = ValueHint::FilePath)]
    pub party_details: Option<PathBuf>,

    /// AEC candidate CSV file
    #[clap(parse(from_os_str), value_name = "CANDS_FILE", value_hint = ValueHint::FilePath)]
    pub candidates: PathBuf,

    /// The configuration file to generate.
    #[clap(parse(from_os_str), value_name = "NEW_CONFIG", value_hint = ValueHint::FilePath)]
    pub configfile: PathBuf,
}

/// List scenarios from the configuration file.
#[derive(Parser, Debug, PartialEq)]
#[clap(
    after_help = "Scenario tables are printed to standard output. If that's a terminal, they'll be pretty-printed with elastic tabstops. If that's a pipe or file, they'll be tab-separated to make further processing as straightforward as possible."
)]
pub struct CliList {
    /// The configuration file to list scenarios from
    #[clap(parse(from_os_str), value_hint = ValueHint::FilePath)]
    pub configfile: PathBuf,
}

/// Run scenarios from the configuration file.
#[derive(Parser, Debug, PartialEq)]
pub struct CliRun {
    /// Run a specific phase of analysis
    #[clap(long, arg_enum, default_value_t = CliRunPhase::All)]
    pub phase: CliRunPhase,

    /// Also output JavaScript from the combination phase, for the website predictor
    #[clap(long)]
    pub js: bool,

    /// Run a SPECIFIC scenario from the configuration file (can be given multiple times to run several scenarios)
    #[clap(long, short)]
    pub scenario: Option<Vec<String>>,

    /// The configuration file to run
    #[clap(parse(from_os_str), value_hint = ValueHint::FilePath)]
    pub configfile: PathBuf,
}

#[derive(ArgEnum, Debug, PartialEq, Clone)]
pub enum CliRunPhase {
    /// Run all phases (default)
    All,
    /// Perform ONLY the party-preferred distribution phase
    Distribute,
    /// Perform ONLY the polling-places to SA1s projection phase
    Project,
    /// Perform ONLY the SA1s to districts combination phase
    Combine,
}

#[derive(Parser, Debug, PartialEq)]
pub struct CliRunCombine {}

// Do the heavy lifting of nparty run so as to keep things clean
pub fn run(args: CliRun) -> anyhow::Result<()> {
    let cfgpath = args.configfile;

    // Get data out of config
    let cfg = config::get_scenarios(&config::get_cfg_doc_from_path(&cfgpath)?)?;

    let scenario_names: Vec<String> = args
        .scenario
        .unwrap_or_else(|| cfg.keys().cloned().collect());

    for scen_name in scenario_names {
        let scenario = cfg.get(&scen_name).with_context(|| {
            format!(
                "Requested scenario {} not found in configuration file.",
                scen_name
            )
        })?;
        eprintln!("Running Scenario {}", scen_name);
        // eprintln!("{:#?}", scenario);

        let sa1b = scenario.sa1s_breakdown.as_ref();
        let sa1p = scenario.sa1s_prefs.as_ref();
        let sa1d = scenario.sa1s_dists.as_ref();
        let nppd = scenario.npp_dists.as_ref();
        let can_project = sa1p.is_some()
            && sa1b.is_some()
            && (args.phase == CliRunPhase::All || args.phase == CliRunPhase::Project);
        let can_combine = sa1p.is_some()
            && sa1d.is_some()
            && nppd.is_some()
            && (args.phase == CliRunPhase::All || args.phase == CliRunPhase::Combine);
        let can_distribute =
            args.phase == CliRunPhase::All || args.phase == CliRunPhase::Distribute;

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
            .context("Could not perform projection phase; stopping.")?;
        }
        if can_combine {
            aggregator::aggregate(
                sa1p.unwrap(),
                sa1d.unwrap(),
                nppd.unwrap(),
                args.js,
                &scenario.groups,
            )
            .context("Could not perform combination phase; stopping.")?;
        }
    }
    eprintln!("Done!");
    Ok(())
}

pub fn do_upgrade_prefs(args: CliUpgradePrefs) -> anyhow::Result<()> {
    let candspath = args.candidates;
    let inpath = args.input;
    let outpath = args.output;
    let suffix = args.suffix;
    let filter = args.filter;

    let mut paths: Vec<(PathBuf, PathBuf)> = Vec::new();

    if inpath.is_dir() {
        if !outpath.is_dir() {
            bail!("Input path is a directory but output path is not.");
        } else {
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
        }
    } else {
        let ip = inpath.clone();
        if outpath.is_dir() {
            paths.push((
                ip,
                outpath.join(&inpath.file_name().context("no file name")?),
            ));
        } else {
            paths.push((ip, outpath));
        }
    }

    for (ipath, opath) in &paths {
        let candsdata = utils::read_candidates(
            File::open(&candspath).context("Couldn't open candidates file")?,
        )?;
        let divstates = upgrades::divstate_creator(
            File::open(&candspath).context("Couldn't open candidates file")?,
        );

        // eprintln!("ipath: {} \t opath: {}", ipath.display(), opath.display());

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

pub fn do_configure(args: CliConfigure) -> anyhow::Result<()> {
    // requireds
    let candspath = args.candidates;
    let outpath = args.configfile;

    // (semi)optionals
    let from_scen = args.from;
    let output_dir = args.output_dir;
    let party_details = args.party_details;
    let polling_places = args.polling_places;
    let sa1s_breakdown = args.sa1s_breakdown;
    let year = args.year;
    let state = args.state.map(|x| x.to_state_ab());

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
    // eprintln!("{:#?}", out);

    let mut outfile = File::create(outpath)?;
    config::write_scenarios(out, &mut outfile)?;
    Ok(())
}

pub fn print_example_config(args: CliExample) -> anyhow::Result<()> {
    match args.year.as_ref() {
        "2016" => println!("{}", include_str!("../example_config_2016.toml")),
        "2019" => println!("{}", include_str!("../example_config.toml")),
        _ => bail!(
            "No example available for the year {}. Sorry about that!",
            args.year
        ),
    };
    Ok(())
}

pub fn print_readme() -> anyhow::Result<()> {
    let readme = include_str!("../README.md");
    println!("{}", readme);
    Ok(())
}

/// Before releasing, run
///     cargo-license --avoid-build-deps --avoid-dev-deps -a -t > src/dependencies.txt
pub fn print_license() -> anyhow::Result<()> {
    println!(include_str!("license-preface.txt"));
    println!("\nDependencies of nparty are listed as follows:\n");
    println!(include_str!("dependencies.txt"));
    Ok(())
}

/// Runs the command
pub fn actual(m: Cli) -> anyhow::Result<()> {
    use CliCommands::*;
    match m.command {
        Configure(sm) => do_configure(sm)?,
        Data(sm) => match sm {
            CliData::Download { DL_FOLDER } => data::download(&DL_FOLDER)?,
            CliData::Examine { FILE } => FILE
                .filter(|x| x.exists())
                .map_or_else(data::examine_txt, |x| data::examine_html(&x)),
        },
        Example(sm) => print_example_config(sm)?,
        License => print_license()?,
        List(sm) => config::list_scenarios(&sm.configfile)?,
        Readme => print_readme()?,
        Run(sm) => run(sm)?,
        Upgrade(sm) => match sm {
            CliUpgrade::Prefs(ssm) => do_upgrade_prefs(ssm)?,
            CliUpgrade::Sa1s(ssm) => upgrades::do_upgrade_sa1s(ssm)?,
        },
    }
    Ok(())
}
