/// nparty: N-Party-Preferred distribution of Australian Senate ballots and subsequent analysis.  
/// Copyright (C) 2017-2022  Alex Jago <abjago@abjago.net>.
/// Released under the MIT or Apache-2.0 licenses, at your option.
#[macro_use]
extern crate serde_derive;

use clap::{AppSettings, Arg, IntoApp, Parser};
use klask::{run_app, Settings};

mod aggregator;
mod app;
mod booths;
mod config;
mod data;
mod multiplier;
mod term;
mod upgrades;
mod utils;

use app::CliCommands::*;
use app::*;

fn main() -> anyhow::Result<()> {
    let m = Cli::into_app()
        .setting(AppSettings::IgnoreErrors)
        .get_matches();
    if m.is_present("gui") {
        // Polyglot app! See https://github.com/MichalGniadek/klask/issues/22
        let n = Cli::into_app().mut_arg("gui", |_| Arg::new("help"));
        run_app(
            n,
            Settings {
                custom_font: Some(std::borrow::Cow::Borrowed(include_bytes!(
                    r"SourceCodePro-Medium.ttf"
                ))),
                ..Default::default()
            },
            |_| {},
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
            CliData::Download { DL_FOLDER } => data::download(&DL_FOLDER),
            CliData::Examine { FILE } => FILE
                .filter(|x| x.exists())
                .map_or_else(data::examine_txt, |x| data::examine_html(&x)),
        },
        Example(sm) => print_example_config(sm)?,
        License => print_license()?,
        List(sm) => config::list_scenarios(&sm.configfile)?,
        Readme(sm) => print_readme(sm)?,
        Run(sm) => run(sm)?,
        Upgrade(sm) => match sm {
            CliUpgrade::Prefs(ssm) => do_upgrade_prefs(ssm)?,
            CliUpgrade::Sa1s(ssm) => upgrades::do_upgrade_sa1s(ssm)?,
        },
        _ => unreachable!(),
    }
    Ok(())
}
