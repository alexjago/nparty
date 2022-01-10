/// nparty-gui: N-Party-Preferred distribution of Australian Senate ballots and subsequent analysis.  
/// Copyright (C) 2017-2022  Alex Jago <abjago@abjago.net>.
/// Released under the MIT or Apache-2.0 licenses, at your option.

#[macro_use]
extern crate serde_derive;
use klask::{Settings, run_derived};

mod aggregator;
mod booths;
mod config;
mod data;
mod multiplier;
mod term;
mod upgrades;
mod utils;
mod app;

use crate::app::*;
use crate::CliCommands::*;

fn main() {
    run_derived(
        Settings::default(),
        |m: Cli| match m.command {
            Configure(sm) => do_configure(sm).expect("Error creating configuration"),
            Data(sm) => match sm {
                CliData::Download {DL_FOLDER} => data::download(&DL_FOLDER),
                CliData::Examine {FILE} => FILE.filter(|x| x.exists()).map_or_else(
                    data::examine_txt,
                    |x| data::examine_html(&x),
                ),
            }
            List(sm) => config::list_scenarios(&sm.configfile).expect("Config error"),
            Run(sm) => run(sm).expect("Error running scenario(s)"),
            Upgrade(sm) => match sm {
                CliUpgrade::Prefs(ssm) => do_upgrade_prefs(ssm).expect("Error upgrading prefs"),
                CliUpgrade::Sa1s(_) => eprintln!("The SA1s upgrade functionality is not implemented yet. Sorry!"),
            },
        }
    );
}
