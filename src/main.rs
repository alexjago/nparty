/// nparty: N-Party-Preferred distribution of Australian Senate ballots and subsequent analysis.  
/// Copyright (C) 2017-2022  Alex Jago <abjago@abjago.net>.
/// Released under the MIT or Apache-2.0 licenses, at your option.
#[macro_use]
extern crate serde_derive;

use anyhow::bail;
use clap::{Parser, IntoApp, AppSettings, Arg};
use config::KnownConfigOptions;
use klask::{Settings, run_app};

mod aggregator;
mod booths;
mod config;
mod data;
mod multiplier;
mod term;
mod upgrades;
mod utils;
mod app;

use app::*;
use app::CliCommands::*;

fn main() -> anyhow::Result<()> {
    let m = Cli::into_app().setting(AppSettings::IgnoreErrors).get_matches();
    if m.is_present("gui") {
        // Polyglot app! See https://github.com/MichalGniadek/klask/issues/22
        let n = Cli::into_app().mut_arg("gui", |_| Arg::new("help"));
        run_app(
            n,
            Settings::default(),
            |_| {}
        );
        Ok(())
    } else {
        actual(Cli::parse())
    }
}

fn actual(m: Cli) -> anyhow::Result<()> {
    match m.command {
        Configure(sm) => do_configure(sm)?,
        Data(sm) => match sm {
            CliData::Download {DL_FOLDER} => data::download(&DL_FOLDER),
            CliData::Examine {FILE} => FILE.filter(|x| x.exists()).map_or_else(
                data::examine_txt,
                |x| data::examine_html(&x),
            ),
        },
        List(sm) => config::list_scenarios(&sm.configfile)?,
        Run(sm) => run(sm)?,
        Upgrade(sm) => match sm {
            CliUpgrade::Prefs(ssm) => do_upgrade_prefs(ssm)?,
            CliUpgrade::Sa1s(_) => bail!("The SA1s upgrade functionality is not implemented yet. Sorry!"),
        },
        // _ => unimplemented!()
    }
    Ok(())
}

