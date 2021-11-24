/// nparty: N-Party-Preferred distribution of Australian Senate ballots and subsequent analysis.  
/// Copyright (C) 2017-2020  Alex Jago <abjago@abjago.net>.
/// Released under the MIT or Apache-2.0 licenses, at your option.
extern crate csv;
#[macro_use]
extern crate serde_derive;
extern crate ansi_term;
extern crate unicode_segmentation;
#[macro_use]
extern crate maplit;
extern crate atty;
extern crate factorial;
extern crate itertools;
extern crate zip;
extern crate zip_extensions;
#[macro_use]
extern crate clap;
extern crate toml_edit;
#[macro_use]
extern crate lazy_static;
extern crate glob;
extern crate ron;
extern crate tabwriter;
extern crate url;

use clap::{load_yaml, App};

use std::ffi::{OsStr, OsString};
use std::fs::{read_dir, File};
use std::path::{Path, PathBuf};

mod aggregator;
mod booths;
mod config;
mod data;
mod multiplier;
mod term;
mod upgrades;
mod utils;

fn main() {
    let yml = load_yaml!("cli.yaml");
    let m = App::from(yml).get_matches();

    // eprintln!("{:#?}", &m);

    // Match on various subcommands

    if let Some(sm) = m.subcommand_matches("run") {
        run(sm);
    } else if let Some(sm) = m.subcommand_matches("list") {
        let cfgpath: &Path = Path::new(
            sm.value_of_os("configfile")
                .expect("Error with configuration-file path."),
        );
        config::list_scenarios(cfgpath);
    } else if let Some(sm) = m.subcommand_matches("data") {
        if sm.is_present("download") {
            let dldir: &Path = Path::new(
                sm.value_of_os("download")
                    .expect("Error with download path."),
            );
            data::download(dldir);
        // TODO: prompt to also upgrade
        } else {
            // examine
            let argy = sm.value_of("examine").expect("Invalid file.");
            if argy == "-" {
                data::examine_txt();
            } else {
                let filey: &Path = Path::new(sm.value_of_os("examine").expect("Invalid file."));
                data::examine_html(filey);
            }
        }
    } else if let Some(sm) = m.subcommand_matches("upgrade") {
        if let Some(ssm) = sm.subcommand_matches("prefs") {
            do_upgrade_prefs(ssm);
        } else {
            todo!();
        }
    }
}

// Do the heavy lifting of nparty run so as to keep things clean
fn run(sm: &clap::ArgMatches) {
    // eprintln!("{:#?}", &sm);

    let cfgpath: &Path = Path::new(
        sm.value_of_os("configfile")
            .expect("Error with configuration-file path."),
    );
    // eprintln!("{:#?}", &cfgpath);

    // Get data out of config
    let cfg = config::get_scenarios(&config::get_cfg_doc_from_path(cfgpath)).unwrap();

    let mut scenario_names: Vec<String> = Vec::new();
    if sm.is_present("scenario") {
        for i in sm
            .values_of("scenario")
            .expect("error getting scenario values somehow")
        {
            scenario_names.push(String::from(i));
        }
    } else {
        scenario_names = cfg.keys().cloned().collect();
    }

    for scen_name in scenario_names {
        // Which phase(s)?
        let scenario = cfg.get(&scen_name).expect(&format!(
            "Requested scenario {} not found in configuration file",
            scen_name
        ));
        // eprintln!("{:#?}", scenario);
        eprintln!("Running Scenario {}", scen_name);

        let _sa1b = scenario.sa1s_breakdown.as_ref();
        let _sa1p = scenario.sa1s_prefs.as_ref();
        let _sa1d = scenario.sa1s_dists.as_ref();
        let _nppd = scenario.npp_dists.as_ref();
        let can_project = _sa1p.is_some() && _sa1b.is_some();
        let can_combine = _sa1p.is_some() && _sa1d.is_some() && _nppd.is_some();

        // TODO: make this intelligent - i.e., don't default to -r
        if sm.is_present("distribute") {
            booths::booth_npps(
                &scenario.groups,
                &scenario.state,
                &scenario.prefs_path,
                &scenario.polling_places,
                &scenario.npp_booths,
            );
        } else if sm.is_present("project") && can_project {
            multiplier::project(
                &scenario.groups,
                &scenario.state,
                &scenario.year,
                &scenario.npp_booths,
                scenario.sa1s_breakdown.as_ref().unwrap(),
                scenario.sa1s_prefs.as_ref().unwrap(),
            );
        } else if sm.is_present("combine") && can_combine {
            aggregator::aggregate(
                scenario.sa1s_prefs.as_ref().unwrap(),
                scenario.sa1s_dists.as_ref().unwrap(),
                scenario.npp_dists.as_ref().unwrap(),
                sm.is_present("js"),
            );
        } else {
            // run all phases
            booths::booth_npps(
                &scenario.groups,
                &scenario.state,
                &scenario.prefs_path,
                &scenario.polling_places,
                &scenario.npp_booths,
            );
            if can_project {
                multiplier::project(
                    &scenario.groups,
                    &scenario.state,
                    &scenario.year,
                    &scenario.npp_booths,
                    scenario.sa1s_breakdown.as_ref().unwrap(),
                    scenario.sa1s_prefs.as_ref().unwrap(),
                );
            }
            if can_combine {
                aggregator::aggregate(
                    scenario.sa1s_prefs.as_ref().unwrap(),
                    scenario.sa1s_dists.as_ref().unwrap(),
                    scenario.npp_dists.as_ref().unwrap(),
                    sm.is_present("js"),
                );
            }
        }
    }
}

fn do_upgrade_prefs(sm: &clap::ArgMatches) {
    let candspath: &Path = Path::new(
        sm.value_of_os("candidates")
            .expect("Error with candidates-file path."),
    );
    let inpath: &Path = Path::new(sm.value_of_os("input").expect("Error with input path."));
    let outpath: &Path = Path::new(sm.value_of_os("output").expect("Error with output path."));
    let suffix: &OsStr = sm.value_of_os("suffix").unwrap();
    let filter: &str = sm.value_of("filter").unwrap();

    // TODO: era sniffing

    // todo: work for directories

    let mut paths: Vec<(PathBuf, PathBuf)> = Vec::new();

    if inpath.is_dir() {
        if !outpath.is_dir() {
            eprintln!("Error: input path is a directory but output path is not.");
            // this is OK to use here because we don't have any non-trivial state
            std::process::exit(1);
        // but if we write cleanly then `paths` will be empty anyway
        } else {
            // get list of input files from directory walk
            // also need to filter
            // never mind read_dir, we want `glob`

            // let ips : Vec<PathBuf> = read_dir(inpath).unwrap().map(|x| x.unwrap().path())
            //                             .filter(|x| file_filter(x, filter)).collect();

            let mut query = String::from(inpath.to_str().unwrap());
            query.push_str(filter);

            let ips: Vec<PathBuf> = glob::glob(&query)
                .unwrap()
                .filter_map(|x| Result::ok(x))
                .map(|x| PathBuf::from(x))
                .collect();

            if inpath == outpath {
                // need to upgrade in place
                // i.e. apply suffix to opaths
                for ip in ips {
                    let mut op_fstem = ip.file_stem().unwrap().to_os_string();
                    op_fstem.push(suffix);
                    let ext = ip.extension().unwrap();
                    let op = ip.clone().with_file_name(op_fstem).with_extension(ext);
                    paths.push((ip, op));
                }
            } else {
                // don't need to upgrade in place
                for ip in ips {
                    let op = outpath.join(ip.file_name().unwrap());
                    paths.push((ip, op));
                }
            }
        }
    } else {
        let ip = PathBuf::from(inpath);
        if outpath.is_dir() {
            paths.push((ip, outpath.join(inpath.file_name().unwrap())));
        } else {
            paths.push((ip, PathBuf::from(outpath)));
        }
    }

    for (ipath, opath) in &paths {
        let candsdata =
            utils::read_candidates(File::open(&candspath).expect("Couldn't open candidates file"));
        let divstates = upgrades::divstate_creator(
            File::open(&candspath).expect("Couldn't open candidates file"),
        );

        eprintln!("ipath: {} \t opath: {}", ipath.display(), opath.display());

        upgrades::upgrade_prefs_16_19(
            &mut utils::open_csvz_from_path(&ipath),
            &mut utils::get_zip_writer_to_path(&opath, "csv"),
            &candsdata,
            &divstates,
        );
    }
}
