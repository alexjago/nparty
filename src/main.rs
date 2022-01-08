/// nparty: N-Party-Preferred distribution of Australian Senate ballots and subsequent analysis.  
/// Copyright (C) 2017-2022  Alex Jago <abjago@abjago.net>.
/// Released under the MIT or Apache-2.0 licenses, at your option.
extern crate csv;
#[macro_use]
extern crate serde_derive;
extern crate ansi_term;
extern crate atty;
extern crate clap;
extern crate factorial;
extern crate glob;
extern crate itertools;
extern crate ron;
extern crate tabwriter;
extern crate toml_edit;
extern crate unicode_segmentation;
extern crate url;
extern crate zip;
extern crate zip_extensions;

use clap::{load_yaml, App};
use anyhow::{bail, Context, Result};
use config::{KnownConfigOptions, Scenario};
use utils::ToStateAb;

use std::collections::BTreeMap;
use std::ffi::OsStr;
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

fn main() -> anyhow::Result<()> {
    let yml = load_yaml!("cli.yaml");
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
        config::list_scenarios(cfgpath);
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
            todo!();
        }
    } else if let Some(sm) = m.subcommand_matches("configure") {
        do_configure(sm)?;
    } else {
        bail!("Not yet implemented");
    }
    Ok(())
}

// Do the heavy lifting of nparty run so as to keep things clean
fn run(sm: &clap::ArgMatches) -> anyhow::Result<()> {
    // eprintln!("{:#?}", &sm);

    let cfgpath: &Path = Path::new(
        sm.value_of_os("configfile")
            .context("Error with configuration-file path.")?,
    );
    // eprintln!("{:#?}", &cfgpath);

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
        // Which phase(s)?
        let scenario = cfg.get(&scen_name).unwrap_or_else(|| {
            panic!(
                "Requested scenario {} not found in configuration file",
                scen_name
            )
        });
        // eprintln!("{:#?}", scenario);
        eprintln!("Running Scenario {}", scen_name);

        let sa1b = scenario.sa1s_breakdown.as_ref();
        let sa1p = scenario.sa1s_prefs.as_ref();
        let sa1d = scenario.sa1s_dists.as_ref();
        let nppd = scenario.npp_dists.as_ref();
        let can_project = sa1p.is_some() && sa1b.is_some();
        let can_combine = sa1p.is_some() && sa1d.is_some() && nppd.is_some();

        // TODO: make this intelligent - i.e., don't default to -r
        if sm.is_present("distribute") {
            booths::booth_npps(
                &scenario.groups,
                &scenario.state,
                &scenario.prefs_path,
                &scenario.polling_places,
                &scenario.npp_booths,
            )?;
        } else if sm.is_present("project") && can_project {
            multiplier::project(
                &scenario.groups,
                &scenario.state,
                &scenario.year,
                &scenario.npp_booths,
                sa1b.unwrap(),
                sa1p.unwrap(),
            )?;
        } else if sm.is_present("combine") && can_combine {
            aggregator::aggregate(
                sa1p.unwrap(),
                sa1d.unwrap(),
                nppd.unwrap(),
                sm.is_present("js"),
                &scenario.groups,
            )?;
        } else {
            // run all phases
            booths::booth_npps(
                &scenario.groups,
                &scenario.state,
                &scenario.prefs_path,
                &scenario.polling_places,
                &scenario.npp_booths,
            )?;
            if can_project {
                multiplier::project(
                    &scenario.groups,
                    &scenario.state,
                    &scenario.year,
                    &scenario.npp_booths,
                    sa1b.unwrap(),
                    sa1p.unwrap(),
                )?;
            }
            if can_combine {
                aggregator::aggregate(
                    sa1p.unwrap(),
                    sa1p.unwrap(),
                    nppd.unwrap(),
                    sm.is_present("js"),
                    &scenario.groups,
                )?;
            }
        }
    }

    Ok(())
}

fn do_upgrade_prefs(sm: &clap::ArgMatches) -> anyhow::Result<()>{
    let candspath: &Path = Path::new(
        sm.value_of_os("candidates")
            .context("Error with candidates-file path.")?,
    );
    let inpath: &Path = Path::new(sm.value_of_os("input").context("Error with input path.")?);
    let outpath: &Path = Path::new(sm.value_of_os("output").context("Error with output path.")?);
    let suffix: &OsStr = sm.value_of_os("suffix").context("missing suffix")?;
    let filter: &str = sm.value_of("filter").context("missing filter")?;

    let mut paths: Vec<(PathBuf, PathBuf)> = Vec::new();

    if inpath.is_dir() {
        if !outpath.is_dir() {
            bail!("Error: input path is a directory but output path is not.");
        } else {
            let mut query: String = inpath.to_str().map(String::from).context("Path conversion error")?;
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
            paths.push((ip, outpath.join(inpath.file_name().context("no file name")?)));
        } else {
            paths.push((ip, PathBuf::from(outpath)));
        }
    }

    for (ipath, opath) in &paths {
        let candsdata =
            utils::read_candidates(File::open(&candspath).context("Couldn't open candidates file")?);
        let divstates = upgrades::divstate_creator(
            File::open(&candspath).context("Couldn't open candidates file")?,
        );

        eprintln!("ipath: {} \t opath: {}", ipath.display(), opath.display());

        let era = upgrades::era_sniff(&mut utils::open_csvz_from_path(ipath))
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
                    &mut utils::open_csvz_from_path(ipath),
                    &mut utils::get_zip_writer_to_path(opath, "csv"),
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

    let candsfile = File::open(candspath).context("Error opening candidates file")?;
    let candidates = utils::read_candidates(candsfile);

    let out =
        config::cli_scenarios(existing, &candidates, &kco).context("Error creating configuration")?;
    eprintln!("{:#?}", out);

    let mut outfile = File::create(outpath).context("Error opening configuration file for writing")?;
    config::write_scenarios(out, &mut outfile).context("Error writing configuration file")?;
    Ok(())
}
