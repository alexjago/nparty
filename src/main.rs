//! nparty: N-Party-Preferred distribution of Australian Senate ballots and subsequent analysis.  
//! Copyright (C) 2017-2022  Alex Jago <abjago@abjago.net>.
//! Released under the MIT or Apache-2.0 licenses, at your option.
#[macro_use]
extern crate serde_derive;

use clap::Parser;

mod aggregator;
mod app;
mod booths;
mod config;
mod data;
mod multiplier;
mod term;
mod upgrades;
mod utils;
use app::Cli;

fn main() -> anyhow::Result<()> {
    app::actual(Cli::parse())
}
